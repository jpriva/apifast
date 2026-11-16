//! Shared secrets between two machines: the collection's files go through
//! git; each machine has its own data directory and keyring.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::{Arc, Mutex};

use apifast_app::memory::MemorySharedSecrets;
use apifast_app::{SecretStore, SharedSecretFiles};
use apifast_vault::{CollectionKeyStore, Key, KeyBackend, KeyFile, KeyStoreError, VaultSecrets};

/// A keyring in memory; clones share it.
#[derive(Clone, Default)]
struct FakeKeyring(Arc<Mutex<Option<Vec<u8>>>>);

impl KeyBackend for FakeKeyring {
    fn available(&self) -> bool {
        true
    }

    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError> {
        *self.0.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
}

/// One machine: its data directory and keyring, on the shared collection.
struct Machine {
    dir: tempfile::TempDir,
    keyring: FakeKeyring,
    secrets: VaultSecrets<FakeKeyring, KeyFile>,
}

fn machine(collection: &Arc<MemorySharedSecrets>) -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::default();
    let secrets = VaultSecrets::with_backends(
        dir.path(),
        "col_1",
        collection.clone(),
        keyring.clone(),
        KeyFile::in_dir(dir.path()),
    )
    .unwrap();
    Machine {
        dir,
        keyring,
        secrets,
    }
}

impl Machine {
    /// This machine's key of the collection, as the team would pass it.
    fn collection_key(&self) -> Key {
        let master = Key::from_bytes(
            self.keyring
                .0
                .lock()
                .unwrap()
                .clone()
                .unwrap()
                .try_into()
                .unwrap(),
        );
        CollectionKeyStore::in_dir(self.dir.path())
            .load(&master, "col_1")
            .unwrap()
            .unwrap()
    }
}

#[test]
fn sharing_makes_the_collection_key_and_its_challenge_once() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.set_local("prod", "token", "t0k").unwrap();
    alice.secrets.share("prod", "token", "t0k").unwrap();
    alice.secrets.share("prod", "key", "k3y").unwrap();

    let challenge = collection.vault_challenge().unwrap().unwrap();
    let envelope = collection.shared_secret("prod", "token").unwrap().unwrap();
    assert!(envelope.starts_with("enc:v1:") && !envelope.contains("t0k"));
    assert!(
        alice.secrets.local().entries().unwrap().is_empty(),
        "moved, not copied"
    );
    assert_eq!(
        alice.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
    // Sharing again keeps the same key and challenge.
    assert_eq!(collection.vault_challenge().unwrap().unwrap(), challenge);
}

#[test]
fn a_teammate_needs_the_collection_key_and_only_the_right_one_is_taken() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();

    let bob = machine(&collection);
    let error = bob.secrets.secret("prod", "token").unwrap_err();
    assert!(
        error.0.contains("does not have the collection's key"),
        "{}",
        error.0
    );
    // Bob cannot start a second key for a collection that has one.
    assert_eq!(
        bob.secrets.share("prod", "other", "x").unwrap_err(),
        KeyStoreError::NoCollectionKey
    );
    assert_eq!(
        bob.secrets
            .use_collection_key(Key::generate().unwrap())
            .unwrap_err(),
        KeyStoreError::WrongCollectionKey
    );

    bob.secrets
        .use_collection_key(alice.collection_key())
        .unwrap();
    assert_eq!(
        bob.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
    bob.secrets.share("prod", "other", "x").unwrap();
    assert_eq!(
        alice.secrets.secret("prod", "other").unwrap().as_deref(),
        Some("x")
    );
}

#[test]
fn making_a_shared_secret_local_removes_it_from_the_collection() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    alice.secrets.set_local("prod", "token", "mine").unwrap();
    assert_eq!(collection.shared_secret("prod", "token").unwrap(), None);
    assert_eq!(
        alice.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("mine")
    );
}

#[test]
fn a_shared_value_moved_to_another_variable_does_not_open() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    let envelope = collection.shared_secret("prod", "token").unwrap().unwrap();
    collection
        .set_shared_secret("prod", "password", &envelope)
        .unwrap();
    assert!(alice.secrets.secret("prod", "password").is_err());
}
