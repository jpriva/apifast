//! A file of values sealed with the master key (docs/06): each value is its
//! own envelope, bound to its name, so a value copied under another name
//! does not open. OAuth 2.0 tokens and the proxy's password live in such
//! files, under the data directory, never in what is shared.
//!
//! ```json
//! {"format": 1, "values": {"proxy_password": "enc:v1:..."}}
//! ```

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::keys::owner_only;
use crate::{Aad, Key, KeyBackend, KeyFile, SystemKeyring, master_key, open, seal};

const FORMAT: u64 = 1;

/// Why a sealed file cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot use `{path}`: {message}")]
pub struct SealedError {
    /// The file.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

/// Named values sealed with the master key, in one file.
pub struct SealedValues<K = SystemKeyring, F = KeyFile> {
    file: PathBuf,
    /// Binds the values to what they are for: `oauth2-token`, `settings`.
    purpose: String,
    /// The key under which the values are kept in the file.
    section: &'static str,
    keyring: K,
    key_file: F,
    master: Mutex<Option<Key>>,
}

impl SealedValues {
    /// The values in `file`, for `purpose`, with the master key in `keyring`
    /// or in its file under `data_dir`.
    pub fn new(file: PathBuf, purpose: &str, data_dir: &Path, keyring: SystemKeyring) -> Self {
        Self::with_backends(file, purpose, keyring, KeyFile::in_dir(data_dir))
    }
}

impl<K: KeyBackend, F: KeyBackend> SealedValues<K, F> {
    /// As [`SealedValues::new`], with the master key in `keyring` or
    /// `key_file`.
    pub fn with_backends(file: PathBuf, purpose: &str, keyring: K, key_file: F) -> Self {
        Self::in_section(file, purpose, "values", keyring, key_file)
    }

    /// As [`SealedValues::with_backends`], keeping the values under
    /// `section` in the file.
    pub(crate) fn in_section(
        file: PathBuf,
        purpose: &str,
        section: &'static str,
        keyring: K,
        key_file: F,
    ) -> Self {
        Self {
            file,
            purpose: purpose.to_owned(),
            section,
            keyring,
            key_file,
            master: Mutex::new(None),
        }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    fn error(&self, message: &dyn std::fmt::Display) -> SealedError {
        SealedError {
            path: self.file.clone(),
            message: message.to_string(),
        }
    }

    fn master(&self) -> Result<Key, SealedError> {
        let mut master = self.master.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(key) = master.as_ref() {
            return Ok(key.clone());
        }
        let (key, _) = master_key(&self.keyring, &self.key_file).map_err(|e| self.error(&e))?;
        *master = Some(key.clone());
        Ok(key)
    }

    fn aad(&self, name: &str) -> Aad {
        Aad::other(&self.purpose, name)
    }

    fn read(&self) -> Result<Map<String, Value>, SealedError> {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(e) => return Err(self.error(&e)),
        };
        let doc: Value = serde_json::from_str(&text).map_err(|e| self.error(&e))?;
        if doc.get("format").and_then(Value::as_u64) != Some(FORMAT) {
            return Err(self.error(&"written by a newer version of apifast"));
        }
        Ok(doc
            .get(self.section)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default())
    }

    fn write(&self, values: &Map<String, Value>) -> Result<(), SealedError> {
        let text = serde_json::to_string_pretty(&json!({"format": FORMAT, self.section: values}))
            .map_err(|e| self.error(&e))?;
        let dir = self.file.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(|e| self.error(&e))?;
        let partial = self.file.with_extension("partial");
        owner_only(&partial)
            .and_then(|mut file| {
                file.write_all(text.as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&partial, &self.file))
            .map_err(|e| {
                let _ = fs::remove_file(&partial);
                self.error(&e)
            })
    }

    /// The value `name`. One that no longer opens (another master key)
    /// is as good as none.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, or the master key cannot be had.
    pub fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SealedError> {
        let values = self.read()?;
        let Some(envelope) = values.get(name).and_then(Value::as_str) else {
            return Ok(None);
        };
        let master = self.master()?;
        Ok(open(&master, envelope, &self.aad(name)).ok())
    }

    /// Keeps `value` as `name`.
    ///
    /// # Errors
    ///
    /// If the file cannot be written, or the master key cannot be had.
    pub fn set(&self, name: &str, value: &[u8]) -> Result<(), SealedError> {
        let master = self.master()?;
        let envelope = seal(&master, value, &self.aad(name)).map_err(|e| self.error(&e))?;
        let mut values = self.read()?;
        values.insert(name.to_owned(), Value::String(envelope));
        self.write(&values)
    }

    /// Forgets `name`.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    pub fn remove(&self, name: &str) -> Result<(), SealedError> {
        let mut values = self.read()?;
        if values.remove(name).is_some() {
            self.write(&values)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_sealed_and_bound_to_their_name() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.sealed");
        let values = SealedValues::new(file.clone(), "settings", dir.path(), SystemKeyring::off());
        assert_eq!(values.get("proxy_password").unwrap(), None);
        values.set("proxy_password", b"hunter2").unwrap();
        assert_eq!(
            values
                .get("proxy_password")
                .unwrap()
                .as_deref()
                .map(Vec::as_slice),
            Some(&b"hunter2"[..])
        );
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("hunter2"));
        let doc: Value = serde_json::from_str(&text).unwrap();
        let moved = json!({"format": 1, "values": {"other": doc["values"]["proxy_password"]}});
        fs::write(&file, moved.to_string()).unwrap();
        assert_eq!(values.get("other").unwrap(), None);
        values.remove("other").unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("other"));
    }
}
