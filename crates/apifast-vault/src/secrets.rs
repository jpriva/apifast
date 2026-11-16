//! The secrets of one collection as apifast uses them: local ones first,
//! then shared ones, each opened with its key.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use apifast_app::{SecretStore, SharedSecretFiles, StoreError};
use apifast_domain::Environment;

use crate::{
    Aad, CollectionKeyStore, Key, KeyBackend, KeyFile, KeyStoreError, LocalVault, SystemKeyring,
    check_challenge, existing_master_key, master_key, new_challenge, open, seal,
};

/// Where a stored secret lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretKind {
    /// Only on this machine, sealed with the master key.
    Local,
    /// In the collection, sealed with the collection's key.
    Shared,
}

/// A stored secret that no variable uses: its variable is gone or is no
/// longer secret, as after editing the collection with Bruno.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Orphan {
    /// Its environment.
    pub environment: String,
    /// Its variable.
    pub name: String,
    /// Where it is stored.
    pub kind: SecretKind,
}

/// The secrets of one collection. The master key is read the first time a
/// secret is, and never made by reading.
pub struct VaultSecrets<K = SystemKeyring, F = KeyFile> {
    collection: String,
    local: LocalVault,
    shared: Arc<dyn SharedSecretFiles>,
    collection_keys: CollectionKeyStore,
    keyring: K,
    file: F,
    master: Mutex<Option<Key>>,
    collection_key: Mutex<Option<Key>>,
}

impl VaultSecrets {
    /// The secrets of `collection`, whose shared ones are in `shared`, with
    /// the master key in the system keyring or in its file under `data_dir`.
    ///
    /// # Errors
    ///
    /// If `collection` is not a valid collection id.
    pub fn for_collection(
        data_dir: &Path,
        collection: &str,
        shared: Arc<dyn SharedSecretFiles>,
    ) -> Result<Self, KeyStoreError> {
        Self::with_backends(
            data_dir,
            collection,
            shared,
            SystemKeyring,
            KeyFile::in_dir(data_dir),
        )
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn files(e: &apifast_app::RepositoryError) -> KeyStoreError {
    KeyStoreError::Collection(e.to_string())
}

impl<K: KeyBackend, F: KeyBackend> VaultSecrets<K, F> {
    /// As [`VaultSecrets::for_collection`], with the master key in
    /// `keyring` or in `file`. For tests, a fake keyring.
    ///
    /// # Errors
    ///
    /// If `collection` is not a valid collection id.
    pub fn with_backends(
        data_dir: &Path,
        collection: &str,
        shared: Arc<dyn SharedSecretFiles>,
        keyring: K,
        file: F,
    ) -> Result<Self, KeyStoreError> {
        Ok(Self {
            collection: collection.to_owned(),
            local: LocalVault::in_dir(data_dir, collection)?,
            shared,
            collection_keys: CollectionKeyStore::in_dir(data_dir),
            keyring,
            file,
            master: Mutex::new(None),
            collection_key: Mutex::new(None),
        })
    }

    /// The local vault.
    pub fn local(&self) -> &LocalVault {
        &self.local
    }

    /// The master key if there is one. A failure is not kept, so a locked
    /// keyring is asked again next time.
    fn existing_master(&self) -> Result<Option<Key>, KeyStoreError> {
        let mut master = locked(&self.master);
        if master.is_none() {
            *master = existing_master_key(&self.keyring, &self.file)?.map(|(key, _)| key);
        }
        Ok(master.clone())
    }

    /// The master key, made if there is none yet.
    fn master_or_new(&self) -> Result<Key, KeyStoreError> {
        if let Some(master) = self.existing_master()? {
            return Ok(master);
        }
        let (master, _) = master_key(&self.keyring, &self.file)?;
        *locked(&self.master) = Some(master.clone());
        Ok(master)
    }

    /// This machine's key of the collection, if it has one.
    fn existing_collection_key(&self) -> Result<Option<Key>, KeyStoreError> {
        let mut key = locked(&self.collection_key);
        if key.is_none()
            && let Some(master) = self.existing_master()?
        {
            *key = self.collection_keys.load(&master, &self.collection)?;
        }
        Ok(key.clone())
    }

    /// The collection's key, made the first time a secret is shared, with
    /// its challenge.
    ///
    /// # Errors
    ///
    /// If the collection already has a challenge (someone shared a secret)
    /// but this machine does not have its key: the key must come from the
    /// team, not be made again.
    fn collection_key_or_new(&self) -> Result<Key, KeyStoreError> {
        if let Some(key) = self.existing_collection_key()? {
            return Ok(key);
        }
        if self
            .shared
            .vault_challenge()
            .map_err(|e| files(&e))?
            .is_some()
        {
            return Err(KeyStoreError::NoCollectionKey);
        }
        let master = self.master_or_new()?;
        let key = Key::generate()?;
        let challenge = new_challenge(&key, &self.collection)?;
        self.collection_keys.save(&master, &self.collection, &key)?;
        self.shared
            .set_vault_challenge(&challenge)
            .map_err(|e| files(&e))?;
        *locked(&self.collection_key) = Some(key.clone());
        Ok(key)
    }

    /// Stores `value` as the local secret `name` of `environment`: only on
    /// this machine. A shared value of it is removed.
    ///
    /// # Errors
    ///
    /// If the master key cannot be read or made, or a file written.
    pub fn set_local(
        &self,
        environment: &str,
        name: &str,
        value: &str,
    ) -> Result<(), KeyStoreError> {
        let master = self.master_or_new()?;
        self.local.set(&master, environment, name, value)?;
        self.shared
            .remove_shared_secret(environment, name)
            .map_err(|e| files(&e))?;
        Ok(())
    }

    /// Stores `value` as the shared secret `name` of `environment`, sealed
    /// with the collection's key in the collection's files. A local value
    /// of it is removed.
    ///
    /// # Errors
    ///
    /// If the keys cannot be read or made (see [`KeyStoreError`]), or a file
    /// written.
    pub fn share(&self, environment: &str, name: &str, value: &str) -> Result<(), KeyStoreError> {
        let key = self.collection_key_or_new()?;
        let aad = Aad::variable(&self.collection, environment, name);
        let envelope = seal(&key, value.as_bytes(), &aad)?;
        self.shared
            .set_shared_secret(environment, name, &envelope)
            .map_err(|e| files(&e))?;
        self.local.remove(environment, name)?;
        Ok(())
    }

    /// Takes `key` as the collection's key, as given by the team: checked
    /// against the challenge, then stored wrapped with the master key.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError::WrongCollectionKey`] if it does not open the
    /// challenge; or if it cannot be stored.
    pub fn use_collection_key(&self, key: Key) -> Result<(), KeyStoreError> {
        let challenge = self.shared.vault_challenge().map_err(|e| files(&e))?;
        if let Some(challenge) = challenge {
            check_challenge(&key, &self.collection, &challenge)
                .map_err(|_| KeyStoreError::WrongCollectionKey)?;
        }
        let master = self.master_or_new()?;
        self.collection_keys.save(&master, &self.collection, &key)?;
        *locked(&self.collection_key) = Some(key);
        Ok(())
    }

    /// The stored secrets that no secret variable of `environments` uses,
    /// sorted.
    ///
    /// # Errors
    ///
    /// If the vault or the collection's files cannot be read.
    pub fn orphans(&self, environments: &[Environment]) -> Result<Vec<Orphan>, KeyStoreError> {
        let used = |environment: &str, name: &str| {
            environments.iter().any(|env| {
                env.name == environment
                    && env
                        .variables
                        .iter()
                        .any(|var| var.secret && var.name == name)
            })
        };
        let local = self
            .local
            .entries()?
            .into_iter()
            .map(|e| (e.environment, e.name, SecretKind::Local));
        let shared = self
            .shared
            .shared_secret_entries()
            .map_err(|e| files(&e))?
            .into_iter()
            .map(|(environment, name)| (environment, name, SecretKind::Shared));
        let mut orphans: Vec<Orphan> = local
            .chain(shared)
            .filter(|(environment, name, _)| !used(environment, name))
            .map(|(environment, name, kind)| Orphan {
                environment,
                name,
                kind,
            })
            .collect();
        orphans.sort();
        Ok(orphans)
    }

    /// Removes a stored secret, such as an orphan.
    ///
    /// # Errors
    ///
    /// If the vault or the collection's files cannot be written.
    pub fn forget(
        &self,
        environment: &str,
        name: &str,
        kind: SecretKind,
    ) -> Result<(), KeyStoreError> {
        match kind {
            SecretKind::Local => self.local.remove(environment, name).map(drop),
            SecretKind::Shared => self
                .shared
                .remove_shared_secret(environment, name)
                .map(drop)
                .map_err(|e| files(&e)),
        }
    }

    /// Seals again the secrets of environment `from` for its new name `to`:
    /// their associated data hold the environment's name, so without this
    /// they would no longer open. Each stays local or shared.
    ///
    /// # Errors
    ///
    /// If a key is missing (a shared secret without the collection's key on
    /// this machine), or a file cannot be read or written. Secrets already
    /// moved stay moved; the rest stay under `from`.
    pub fn rename_environment(&self, from: &str, to: &str) -> Result<(), KeyStoreError> {
        let local: Vec<String> = (self.local.entries()?.into_iter())
            .filter(|e| e.environment == from)
            .map(|e| e.name)
            .collect();
        if !local.is_empty() {
            let master = self.master_or_new()?;
            for name in local {
                if let Some(value) = self.local.get(&master, from, &name)? {
                    self.local.set(&master, to, &name, &value)?;
                    self.local.remove(from, &name)?;
                }
            }
        }
        let shared: Vec<String> = (self.shared.shared_secret_entries())
            .map_err(|e| files(&e))?
            .into_iter()
            .filter(|(environment, _)| environment == from)
            .map(|(_, name)| name)
            .collect();
        for name in shared {
            let Some(value) = self.value(from, &name)? else {
                continue;
            };
            self.share(to, &name, &value)?;
            self.forget(from, &name, SecretKind::Shared)?;
        }
        Ok(())
    }

    fn value(&self, environment: &str, name: &str) -> Result<Option<String>, KeyStoreError> {
        if let Some(master) = self.existing_master()?
            && let Some(value) = self.local.get(&master, environment, name)?
        {
            return Ok(Some(value.as_str().to_owned()));
        }
        let Some(envelope) = self
            .shared
            .shared_secret(environment, name)
            .map_err(|e| files(&e))?
        else {
            return Ok(None);
        };
        let Some(key) = self.existing_collection_key()? else {
            return Err(KeyStoreError::SharedWithoutKey(name.to_owned()));
        };
        let aad = Aad::variable(&self.collection, environment, name);
        let bytes = open(&key, &envelope, &aad)?;
        String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|_| KeyStoreError::Damaged)
    }
}

impl<K, F> SecretStore for VaultSecrets<K, F>
where
    K: KeyBackend + Send + Sync,
    F: KeyBackend + Send + Sync,
{
    fn secret(&self, environment: &str, name: &str) -> Result<Option<String>, StoreError> {
        self.value(environment, name)
            .map_err(|e| StoreError(e.to_string()))
    }
}
