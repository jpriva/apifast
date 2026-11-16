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

fn env(name: &str, variables: Vec<apifast_domain::Variable>) -> apifast_domain::Environment {
    apifast_domain::Environment::new(name, variables)
}

#[test]
fn stored_secrets_no_secret_variable_uses_are_orphans_and_can_be_forgotten() {
    use apifast_domain::Variable;
    use apifast_vault::{Orphan, SecretKind};

    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.set_local("prod", "token", "t").unwrap();
    alice.secrets.set_local("prod", "gone", "g").unwrap();
    alice.secrets.share("prod", "plain_now", "p").unwrap();
    alice.secrets.set_local("old_env", "token", "o").unwrap();

    let environments = [env(
        "prod",
        vec![
            Variable::secret("token"),
            // Bruno turned it into a plain variable.
            Variable::text("plain_now", "visible"),
        ],
    )];
    let orphans = alice.secrets.orphans(&environments).unwrap();
    let orphan = |environment: &str, name: &str, kind| Orphan {
        environment: environment.into(),
        name: name.into(),
        kind,
    };
    assert_eq!(
        orphans,
        [
            orphan("old_env", "token", SecretKind::Local),
            orphan("prod", "gone", SecretKind::Local),
            orphan("prod", "plain_now", SecretKind::Shared),
        ]
    );
    for orphan in &orphans {
        alice
            .secrets
            .forget(&orphan.environment, &orphan.name, orphan.kind)
            .unwrap();
    }
    assert_eq!(alice.secrets.orphans(&environments).unwrap(), []);
    assert_eq!(
        alice.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t")
    );
}

#[test]
fn renaming_an_environment_seals_its_secrets_again_under_the_new_name() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.set_local("prod", "token", "t0k").unwrap();
    alice.secrets.share("prod", "key", "k3y").unwrap();
    alice.secrets.set_local("dev", "token", "dev").unwrap();

    alice
        .secrets
        .rename_environment("prod", "production")
        .unwrap();
    let secret = |env: &str, name: &str| alice.secrets.secret(env, name).unwrap();
    assert_eq!(secret("production", "token").as_deref(), Some("t0k"));
    assert_eq!(secret("production", "key").as_deref(), Some("k3y"));
    assert_eq!(secret("prod", "token"), None);
    assert_eq!(secret("prod", "key"), None);
    assert_eq!(
        secret("dev", "token").as_deref(),
        Some("dev"),
        "others untouched"
    );
    // Still shared: the team sees it under the new name.
    assert!(
        collection
            .shared_secret("production", "key")
            .unwrap()
            .is_some()
    );
}

#[test]
fn the_key_is_passed_as_text_and_a_typo_or_another_key_is_refused() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    let bob = machine(&collection);
    assert_eq!(alice.secrets.export_collection_key().unwrap(), None);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    let text = alice.secrets.export_collection_key().unwrap().unwrap();
    assert!(text.starts_with("AFK-"));

    let mut typo: Vec<char> = text.chars().collect();
    typo[5] = if typo[5] == 'X' { 'Y' } else { 'X' };
    let typo: String = typo.into_iter().collect();
    assert!(matches!(
        bob.secrets.import_collection_key(&typo),
        Err(KeyStoreError::Shareable(_))
    ));
    let other = apifast_vault::shareable_key(&Key::generate().unwrap());
    assert_eq!(
        bob.secrets.import_collection_key(&other).unwrap_err(),
        KeyStoreError::WrongCollectionKey
    );

    bob.secrets.import_collection_key(&text).unwrap();
    assert_eq!(
        bob.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
}

#[test]
fn rotating_seals_everything_again_and_the_team_needs_the_new_key() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    let bob = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    alice.secrets.share("dev", "key", "k3y").unwrap();
    bob.secrets
        .import_collection_key(&alice.secrets.export_collection_key().unwrap().unwrap())
        .unwrap();
    let old_challenge = collection.vault_challenge().unwrap().unwrap();
    let old_token = collection.shared_secret("prod", "token").unwrap().unwrap();

    let new_key = alice.secrets.rotate_collection_key().unwrap();
    assert_eq!(
        alice.secrets.export_collection_key().unwrap(),
        Some(new_key.clone())
    );
    assert_ne!(
        collection.vault_challenge().unwrap().unwrap(),
        old_challenge
    );
    assert_ne!(
        collection.shared_secret("prod", "token").unwrap().unwrap(),
        old_token
    );
    assert_eq!(
        alice.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
    assert_eq!(
        alice.secrets.secret("dev", "key").unwrap().as_deref(),
        Some("k3y")
    );
    assert!(!alice.dir.path().join("keys/col_1.previous.key").exists());

    // Bob's old key no longer opens the new files, until he takes the new one.
    assert!(bob.secrets.secret("prod", "token").is_err());
    bob.secrets.import_collection_key(&new_key).unwrap();
    assert_eq!(
        bob.secrets.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
}

#[test]
fn a_rotation_cut_short_loses_nothing_and_can_run_again() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    alice.secrets.share("prod", "key", "k3y").unwrap();
    let old = alice.collection_key();
    let master = Key::from_bytes(
        alice
            .keyring
            .0
            .lock()
            .unwrap()
            .clone()
            .unwrap()
            .try_into()
            .unwrap(),
    );

    // As if the process died after the new key was stored and only one
    // value was sealed again.
    let keys = CollectionKeyStore::in_dir(alice.dir.path());
    let new = Key::generate().unwrap();
    keys.save_previous(&master, "col_1", &old).unwrap();
    keys.save(&master, "col_1", &new).unwrap();
    let aad = apifast_vault::Aad::variable("col_1", "prod", "token");
    let resealed = apifast_vault::seal(&new, b"t0k", &aad).unwrap();
    collection
        .set_shared_secret("prod", "token", &resealed)
        .unwrap();

    let after_crash = machine_with(&collection, &alice);
    assert_eq!(
        after_crash.secret("prod", "token").unwrap().as_deref(),
        Some("t0k")
    );
    assert_eq!(
        after_crash.secret("prod", "key").unwrap().as_deref(),
        Some("k3y")
    );

    after_crash.rotate_collection_key().unwrap();
    assert_eq!(
        after_crash.secret("prod", "key").unwrap().as_deref(),
        Some("k3y")
    );
    assert!(!alice.dir.path().join("keys/col_1.previous.key").exists());
}

#[test]
fn only_a_machine_with_the_key_can_rotate_it() {
    let collection = Arc::new(MemorySharedSecrets::default());
    let alice = machine(&collection);
    alice.secrets.share("prod", "token", "t0k").unwrap();
    let bob = machine(&collection);
    assert_eq!(
        bob.secrets.rotate_collection_key().unwrap_err(),
        KeyStoreError::NoCollectionKey
    );
}

/// The vault of `machine` again, as after a restart: nothing kept in memory.
fn machine_with(
    collection: &Arc<MemorySharedSecrets>,
    machine: &Machine,
) -> VaultSecrets<FakeKeyring, KeyFile> {
    VaultSecrets::with_backends(
        machine.dir.path(),
        "col_1",
        collection.clone(),
        machine.keyring.clone(),
        KeyFile::in_dir(machine.dir.path()),
    )
    .unwrap()
}
