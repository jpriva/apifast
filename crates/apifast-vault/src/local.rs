//! Local secrets: the default kind. The environment file keeps only the
//! name (`secret: true`); the value lives on this machine, in
//! `vault/<collection-id>.vault` under the data directory, sealed with the
//! master key (docs/06).
//!
//! ```json
//! {"format": 1, "secrets": [
//!   {"environment": "prod", "name": "token", "value": "enc:v1:..."}
//! ]}
//! ```
//!
//! Each value is its own envelope, bound to its collection, environment and
//! name: a value moved to another entry does not open.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use apifast_app::{SecretStore, StoreError};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::keys::owner_only;
use crate::{
    Aad, Key, KeyBackend, KeyFile, KeyStoreError, SystemKeyring, existing_master_key, master_key,
    open, seal,
};

const FORMAT: u64 = 1;

/// One entry of the local vault, without its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretEntry {
    /// Its environment.
    pub environment: String,
    /// Its variable.
    pub name: String,
}

/// The local vault of one collection.
#[derive(Debug, Clone)]
pub struct LocalVault {
    file: PathBuf,
    collection: String,
}

/// An entry as stored: environment, name and envelope.
type Stored = (String, String, String);

impl LocalVault {
    /// The vault of `collection` in `vault/` under `data_dir`.
    ///
    /// # Errors
    ///
    /// If `collection` is not a valid collection id.
    pub fn in_dir(data_dir: &Path, collection: &str) -> Result<Self, KeyStoreError> {
        crate::collection::check_id(data_dir, collection)?;
        Ok(Self {
            file: data_dir.join("vault").join(format!("{collection}.vault")),
            collection: collection.to_owned(),
        })
    }

    /// The vault's file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    fn error(&self, message: &dyn std::fmt::Display) -> KeyStoreError {
        KeyStoreError::File {
            path: self.file.clone(),
            message: message.to_string(),
        }
    }

    fn aad(&self, environment: &str, name: &str) -> Aad {
        Aad::variable(&self.collection, environment, name)
    }

    fn read(&self) -> Result<Vec<Stored>, KeyStoreError> {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(self.error(&e)),
        };
        let doc: Value = serde_json::from_str(&text).map_err(|e| self.error(&e))?;
        if doc.get("format").and_then(Value::as_u64) != Some(FORMAT) {
            return Err(self.error(&"written by a newer version of apifast"));
        }
        let entries = doc.get("secrets").and_then(Value::as_array);
        Ok(entries
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let text = |key| Some(entry.get(key)?.as_str()?.to_owned());
                Some((text("environment")?, text("name")?, text("value")?))
            })
            .collect())
    }

    fn write(&self, entries: &[Stored]) -> Result<(), KeyStoreError> {
        let secrets: Vec<Value> = entries
            .iter()
            .map(|(environment, name, value)| {
                json!({"environment": environment, "name": name, "value": value})
            })
            .collect();
        let text = serde_json::to_string_pretty(&json!({"format": FORMAT, "secrets": secrets}))
            .map_err(|e| self.error(&e))?;
        let dir = self.file.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(|e| self.error(&e))?;
        let partial = self.file.with_extension("vault.partial");
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

    /// The value of secret `name` of `environment`, opened with `master`.
    ///
    /// # Errors
    ///
    /// If the vault cannot be read, or the value does not open.
    pub fn get(
        &self,
        master: &Key,
        environment: &str,
        name: &str,
    ) -> Result<Option<Zeroizing<String>>, KeyStoreError> {
        let Some((_, _, envelope)) = self
            .read()?
            .into_iter()
            .find(|(e, n, _)| e == environment && n == name)
        else {
            return Ok(None);
        };
        let bytes = open(master, &envelope, &self.aad(environment, name))?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| KeyStoreError::Damaged)?;
        Ok(Some(Zeroizing::new(text)))
    }

    /// Stores `value` as secret `name` of `environment`, replacing any.
    ///
    /// # Errors
    ///
    /// If the vault cannot be written.
    pub fn set(
        &self,
        master: &Key,
        environment: &str,
        name: &str,
        value: &str,
    ) -> Result<(), KeyStoreError> {
        let envelope = seal(master, value.as_bytes(), &self.aad(environment, name))?;
        let mut entries = self.read()?;
        entries.retain(|(e, n, _)| !(e == environment && n == name));
        entries.push((environment.to_owned(), name.to_owned(), envelope));
        self.write(&entries)
    }

    /// Forgets secret `name` of `environment`. Returns whether it was there.
    ///
    /// # Errors
    ///
    /// If the vault cannot be read or written.
    pub fn remove(&self, environment: &str, name: &str) -> Result<bool, KeyStoreError> {
        let mut entries = self.read()?;
        let before = entries.len();
        entries.retain(|(e, n, _)| !(e == environment && n == name));
        if entries.len() == before {
            return Ok(false);
        }
        self.write(&entries)?;
        Ok(true)
    }

    /// Every entry, without values: to find the ones no variable uses.
    ///
    /// # Errors
    ///
    /// If the vault cannot be read.
    pub fn entries(&self) -> Result<Vec<SecretEntry>, KeyStoreError> {
        Ok(self
            .read()?
            .into_iter()
            .map(|(environment, name, _)| SecretEntry { environment, name })
            .collect())
    }
}

/// The secrets of one collection, as the send needs them: the master key is
/// read the first time a secret is, and never made by reading.
pub struct VaultSecrets<K = SystemKeyring, F = KeyFile> {
    local: LocalVault,
    keyring: K,
    file: F,
    master: Mutex<Option<Key>>,
}

impl VaultSecrets {
    /// The secrets of `collection`, with the master key in the system
    /// keyring or in its file under `data_dir`.
    ///
    /// # Errors
    ///
    /// If `collection` is not a valid collection id.
    pub fn for_collection(data_dir: &Path, collection: &str) -> Result<Self, KeyStoreError> {
        Self::with_backends(
            data_dir,
            collection,
            SystemKeyring,
            KeyFile::in_dir(data_dir),
        )
    }
}

impl<K: KeyBackend, F: KeyBackend> VaultSecrets<K, F> {
    /// The secrets of `collection` with the master key in `keyring`, or in
    /// `file`. For tests, a fake keyring.
    ///
    /// # Errors
    ///
    /// If `collection` is not a valid collection id.
    pub fn with_backends(
        data_dir: &Path,
        collection: &str,
        keyring: K,
        file: F,
    ) -> Result<Self, KeyStoreError> {
        Ok(Self {
            local: LocalVault::in_dir(data_dir, collection)?,
            keyring,
            file,
            master: Mutex::new(None),
        })
    }

    /// The local vault.
    pub fn local(&self) -> &LocalVault {
        &self.local
    }

    /// The master key if there is one. A failure is not kept, so a locked
    /// keyring is asked again next time.
    fn existing_master(&self) -> Result<Option<Key>, KeyStoreError> {
        let mut master = self
            .master
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if master.is_none() {
            *master = existing_master_key(&self.keyring, &self.file)?.map(|(key, _)| key);
        }
        Ok(master.clone())
    }

    /// Stores `value` as the local secret `name` of `environment`, making
    /// the master key if there is none yet.
    ///
    /// # Errors
    ///
    /// If the master key cannot be read or made, or the vault written.
    pub fn set_local(
        &self,
        environment: &str,
        name: &str,
        value: &str,
    ) -> Result<(), KeyStoreError> {
        let master = if let Some(master) = self.existing_master()? {
            master
        } else {
            let (master, _) = master_key(&self.keyring, &self.file)?;
            *self
                .master
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(master.clone());
            master
        };
        self.local.set(&master, environment, name, value)
    }
}

impl<K: KeyBackend + Sync + Send, F: KeyBackend + Sync + Send> SecretStore for VaultSecrets<K, F> {
    fn secret(&self, environment: &str, name: &str) -> Result<Option<String>, StoreError> {
        let store = |e: KeyStoreError| StoreError(e.to_string());
        // No master key: no secret was ever stored on this machine.
        let Some(master) = self.existing_master().map_err(store)? else {
            return Ok(None);
        };
        Ok(self
            .local
            .get(&master, environment, name)
            .map_err(store)?
            .map(|value| value.as_str().to_owned()))
    }
}
