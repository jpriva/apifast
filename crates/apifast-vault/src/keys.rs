//! Where the master key lives: the system keyring, or a file readable only
//! by its owner when there is no keyring (docs/06, ADR 0005).
//!
//! The file is a fallback for systems without a keyring, never for a keyring
//! that fails: if the keyring is there but locked or broken, that is an
//! error. Falling back would make a new key in the file, and the secrets
//! sealed with the key in the keyring would no longer open.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::{KEY_LEN, Key, VaultError};

/// Service name of apifast's entries in the keyring.
pub const KEYRING_SERVICE: &str = "apifast";
/// Name of the master key's entry in the keyring.
pub const KEYRING_ENTRY: &str = "master-key";
/// Name of the fallback file in the data directory.
pub const KEY_FILE: &str = "master.key";

/// Why the master key could not be read or stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyStoreError {
    /// The keyring is there but refused: locked, or broken. Nothing falls
    /// back to the file, so no second key is made.
    #[error("the system keyring cannot be used: {0}")]
    Keyring(String),
    /// The fallback file cannot be read or written.
    #[error("cannot use `{path}`: {message}")]
    File {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// What is stored is not a key.
    #[error("the stored master key is damaged")]
    Damaged,
    /// The collection's files cannot be read or written.
    #[error("{0}")]
    Collection(String),
    /// The collection has shared secrets, but this machine does not have
    /// its key: it must come from the team.
    #[error("this collection already shares secrets: ask your team for its key")]
    NoCollectionKey,
    /// A shared secret is needed, but this machine does not have the
    /// collection's key.
    #[error(
        "secret `{0}` is shared with the team, and this machine does not have the collection's key"
    )]
    SharedWithoutKey(String),
    /// The key given is not the collection's: it does not open the
    /// challenge.
    #[error("that is not this collection's key")]
    WrongCollectionKey,
    /// A key pasted as people pass it cannot be read.
    #[error(transparent)]
    Shareable(#[from] crate::ShareableKeyError),
    /// A new key could not be made.
    #[error(transparent)]
    Vault(#[from] VaultError),
}

/// Where the master key was found or put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLocation {
    /// In the system keyring.
    Keyring,
    /// In the fallback file: anyone who copies the user's data directory
    /// gets it. The interface says so.
    File,
}

/// A place that can hold one key, such as an entry of the system keyring.
pub trait KeyBackend {
    /// Whether this place exists on this system at all.
    fn available(&self) -> bool;

    /// The key's bytes, or `None` if none is stored yet.
    ///
    /// # Errors
    ///
    /// If the place refuses, such as a locked keyring.
    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError>;

    /// Stores the key's bytes.
    ///
    /// # Errors
    ///
    /// If the place refuses.
    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError>;
}

/// The master key's entry in the system keyring (Secret Service, Keychain or
/// Credential Manager).
#[derive(Debug, Default)]
pub struct SystemKeyring;

impl SystemKeyring {
    fn entry() -> Result<keyring::Entry, KeyStoreError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ENTRY)
            .map_err(|e| KeyStoreError::Keyring(e.to_string()))
    }
}

impl KeyBackend for SystemKeyring {
    fn available(&self) -> bool {
        keyring::Entry::store_status().is_ok()
    }

    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        match Self::entry()?.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyStoreError::Keyring(e.to_string())),
        }
    }

    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError> {
        Self::entry()?
            .set_secret(bytes)
            .map_err(|e| KeyStoreError::Keyring(e.to_string()))
    }
}

/// The fallback file, readable and writable only by its owner.
#[derive(Debug, Clone)]
pub struct KeyFile {
    path: PathBuf,
}

impl KeyFile {
    /// The file [`KEY_FILE`] in `data_dir`.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(KEY_FILE),
        }
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn error(&self, e: &io::Error) -> KeyStoreError {
        KeyStoreError::File {
            path: self.path.clone(),
            message: e.to_string(),
        }
    }
}

impl KeyBackend for KeyFile {
    fn available(&self) -> bool {
        true
    }

    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                // Others must not read it, even if something widened its
                // permissions since.
                restrict(&self.path).map_err(|e| self.error(&e))?;
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.error(&e)),
        }
    }

    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(|e| self.error(&e))?;
        // Written aside with its final permissions, then renamed: the key is
        // never in a file others can read, nor half written.
        let partial = self.path.with_extension("key.partial");
        let written = owner_only(&partial).and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
        written
            .and_then(|()| fs::rename(&partial, &self.path))
            .map_err(|e| {
                let _ = fs::remove_file(&partial);
                self.error(&e)
            })
    }
}

/// A new file that only its owner can read and write.
pub(crate) fn owner_only(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    restrict(path)?;
    Ok(file)
}

/// Makes `path` readable and writable only by its owner.
fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn key_from(bytes: &[u8]) -> Result<Key, KeyStoreError> {
    let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyStoreError::Damaged)?;
    Ok(Key::from_bytes(bytes))
}

/// The master key if there is one already, without making one: reading
/// secrets must not create keys (nor prompt the keyring) for a user who has
/// none.
///
/// # Errors
///
/// If the keyring is there but refuses, the file cannot be read, or what is
/// stored is not a key.
pub fn existing_master_key(
    keyring: &impl KeyBackend,
    file: &impl KeyBackend,
) -> Result<Option<(Key, KeyLocation)>, KeyStoreError> {
    if keyring.available()
        && let Some(bytes) = keyring.get()?
    {
        return Ok(Some((key_from(&bytes)?, KeyLocation::Keyring)));
    }
    match file.get()? {
        Some(bytes) => Ok(Some((key_from(&bytes)?, KeyLocation::File))),
        None => Ok(None),
    }
}

/// The master key: from the keyring, else from the file, else a new one put
/// in the keyring, or in the file when there is no keyring.
///
/// # Errors
///
/// If the keyring is there but refuses, the file cannot be used, or what is
/// stored is not a key.
pub fn master_key(
    keyring: &impl KeyBackend,
    file: &impl KeyBackend,
) -> Result<(Key, KeyLocation), KeyStoreError> {
    // A key made while there was no keyring stays in its file: secrets were
    // sealed with it.
    if let Some(found) = existing_master_key(keyring, file)? {
        return Ok(found);
    }
    let has_keyring = keyring.available();
    let key = Key::generate()?;
    if has_keyring {
        keyring.set(key.as_bytes())?;
        Ok((key, KeyLocation::Keyring))
    } else {
        file.set(key.as_bytes())?;
        Ok((key, KeyLocation::File))
    }
}
