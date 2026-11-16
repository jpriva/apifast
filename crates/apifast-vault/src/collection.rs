//! Collection keys: one per collection, wrapped with the master key in
//! `keys/<collection-id>.key` under the data directory, and the challenge
//! that tells whether a key is the collection's (docs/06).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::keys::owner_only;
use crate::{Aad, KEY_LEN, Key, KeyStoreError, VaultError, open, seal};

/// Bytes of random text sealed as a challenge.
const CHALLENGE_LEN: usize = 32;

fn key_aad(collection: &str) -> Aad {
    Aad::other(collection, "collection-key")
}

fn challenge_aad(collection: &str) -> Aad {
    Aad::other(collection, "challenge")
}

/// A sealed challenge for `collection`: random bytes only `key` opens. It
/// goes in `.apifast/collection.yml`, so whoever clones the collection can
/// tell whether the key they were given is the right one.
///
/// # Errors
///
/// If the system gives no randomness.
pub fn new_challenge(key: &Key, collection: &str) -> Result<String, VaultError> {
    let mut text = [0; CHALLENGE_LEN];
    getrandom::fill(&mut text).map_err(|e| VaultError::Randomness(e.to_string()))?;
    seal(key, &text, &challenge_aad(collection))
}

/// Whether `key` is the key of `collection`, by opening its challenge.
///
/// # Errors
///
/// [`VaultError::Rejected`] if it is not; [`VaultError::NotAnEnvelope`] or
/// [`VaultError::UnknownVersion`] if the challenge cannot be read.
pub fn check_challenge(key: &Key, collection: &str, challenge: &str) -> Result<(), VaultError> {
    open(key, challenge, &challenge_aad(collection)).map(drop)
}

/// Checks that `collection` is an id that names a file inside `dir`, so a
/// damaged one cannot point outside it.
pub(crate) fn check_id(dir: &Path, collection: &str) -> Result<(), KeyStoreError> {
    let safe = !collection.is_empty()
        && collection
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    if safe {
        Ok(())
    } else {
        Err(KeyStoreError::File {
            path: dir.to_owned(),
            message: format!("`{collection}` is not a collection id"),
        })
    }
}

/// Which key of a collection a file holds.
#[derive(Debug, Clone, Copy)]
enum Slot {
    /// The one in use.
    Current,
    /// The one before a rotation, kept until the rotation finishes.
    Previous,
}

/// The collection keys of this user, each wrapped with the master key.
#[derive(Debug, Clone)]
pub struct CollectionKeyStore {
    dir: PathBuf,
}

impl CollectionKeyStore {
    /// The keys in `keys/` under `data_dir`.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("keys"),
        }
    }

    /// The file of `collection`'s key in `slot`.
    fn file(&self, collection: &str, slot: Slot) -> Result<PathBuf, KeyStoreError> {
        check_id(&self.dir, collection)?;
        let name = match slot {
            Slot::Current => format!("{collection}.key"),
            Slot::Previous => format!("{collection}.previous.key"),
        };
        Ok(self.dir.join(name))
    }

    fn error(path: &Path, e: &io::Error) -> KeyStoreError {
        KeyStoreError::File {
            path: path.to_owned(),
            message: e.to_string(),
        }
    }

    /// The key of `collection`, unwrapped with `master`; `None` if this user
    /// has none yet.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, or does not open with `master` (another
    /// master key, or a file copied from another collection).
    pub fn load(&self, master: &Key, collection: &str) -> Result<Option<Key>, KeyStoreError> {
        self.load_slot(master, collection, Slot::Current)
    }

    /// The key `collection` had before a rotation that did not finish;
    /// `None` once it finished.
    ///
    /// # Errors
    ///
    /// As [`Self::load`].
    pub fn load_previous(
        &self,
        master: &Key,
        collection: &str,
    ) -> Result<Option<Key>, KeyStoreError> {
        self.load_slot(master, collection, Slot::Previous)
    }

    fn load_slot(
        &self,
        master: &Key,
        collection: &str,
        slot: Slot,
    ) -> Result<Option<Key>, KeyStoreError> {
        let path = self.file(collection, slot)?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Self::error(&path, &e)),
        };
        let bytes = open(master, text.trim(), &key_aad(collection))?;
        let bytes: [u8; KEY_LEN] = bytes[..].try_into().map_err(|_| KeyStoreError::Damaged)?;
        Ok(Some(Key::from_bytes(bytes)))
    }

    /// Stores `key` as the key of `collection`, wrapped with `master`,
    /// replacing any before.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    pub fn save(&self, master: &Key, collection: &str, key: &Key) -> Result<(), KeyStoreError> {
        self.save_slot(master, collection, key, Slot::Current)
    }

    /// Keeps `key` as the one `collection` had before a rotation, until the
    /// rotation finishes.
    ///
    /// # Errors
    ///
    /// As [`Self::save`].
    pub fn save_previous(
        &self,
        master: &Key,
        collection: &str,
        key: &Key,
    ) -> Result<(), KeyStoreError> {
        self.save_slot(master, collection, key, Slot::Previous)
    }

    fn save_slot(
        &self,
        master: &Key,
        collection: &str,
        key: &Key,
        slot: Slot,
    ) -> Result<(), KeyStoreError> {
        let path = self.file(collection, slot)?;
        let wrapped = seal(master, key.as_bytes(), &key_aad(collection))?;
        fs::create_dir_all(&self.dir).map_err(|e| Self::error(&self.dir, &e))?;
        let partial = path.with_extension("key.partial");
        owner_only(&partial)
            .and_then(|mut file| {
                file.write_all(wrapped.as_bytes())?;
                file.write_all(b"\n")?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&partial, &path))
            .map_err(|e| {
                let _ = fs::remove_file(&partial);
                Self::error(&path, &e)
            })
    }

    /// Whether this user has a key of `collection`, without opening it.
    pub fn exists(&self, collection: &str) -> bool {
        self.file(collection, Slot::Current)
            .is_ok_and(|path| path.exists())
    }

    /// Forgets the key of `collection`. Forgetting one that is not there is
    /// fine.
    ///
    /// # Errors
    ///
    /// If the file cannot be removed.
    pub fn remove(&self, collection: &str) -> Result<(), KeyStoreError> {
        self.remove_slot(collection, Slot::Current)
    }

    /// Forgets the key `collection` had before a rotation, once it
    /// finished.
    ///
    /// # Errors
    ///
    /// As [`Self::remove`].
    pub fn remove_previous(&self, collection: &str) -> Result<(), KeyStoreError> {
        self.remove_slot(collection, Slot::Previous)
    }

    fn remove_slot(&self, collection: &str, slot: Slot) -> Result<(), KeyStoreError> {
        let path = self.file(collection, slot)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Self::error(&path, &e)),
        }
    }
}
