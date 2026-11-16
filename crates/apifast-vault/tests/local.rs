//! Local secrets: sealed values on this machine, read for a send without
//! ever making a key, and never kept in the clear.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use apifast_app::SecretStore;
use apifast_app::memory::MemorySharedSecrets;
use apifast_domain::Secret;
use apifast_vault::{
    Key, KeyBackend, KeyFile, KeyStoreError, LocalVault, SecretEntry, VaultSecrets,
};

/// A keyring in memory that can be locked.
#[derive(Default)]
struct FakeKeyring {
    locked: AtomicBool,
    stored: Mutex<Option<Vec<u8>>>,
    reads: Mutex<usize>,
}

impl KeyBackend for &FakeKeyring {
    fn available(&self) -> bool {
        true
    }

    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        *self.reads.lock().unwrap() += 1;
        if self.locked.load(Ordering::Relaxed) {
            return Err(KeyStoreError::Keyring("locked".into()));
        }
        Ok(self.stored.lock().unwrap().clone())
    }

    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError> {
        *self.stored.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn values_are_sealed_on_disk_and_open_only_with_the_master_key() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::in_dir(dir.path(), "col_1").unwrap();
    let master = Key::generate().unwrap();
    vault.set(&master, "prod", "token", "t0k-plain").unwrap();
    vault.set(&master, "dev", "token", "dev-t0k").unwrap();
    vault.set(&master, "prod", "token", "t0k-new").unwrap();

    assert_eq!(
        vault
            .get(&master, "prod", "token")
            .unwrap()
            .unwrap()
            .as_str(),
        "t0k-new"
    );
    assert!(vault.get(&master, "prod", "other").unwrap().is_none());
    let text = std::fs::read_to_string(vault.file()).unwrap();
    assert!(!text.contains("t0k"), "never in the clear: {text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(vault.file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(
        vault
            .get(&Key::generate().unwrap(), "prod", "token")
            .is_err()
    );

    assert_eq!(
        vault.entries().unwrap(),
        [
            SecretEntry {
                environment: "dev".into(),
                name: "token".into()
            },
            SecretEntry {
                environment: "prod".into(),
                name: "token".into()
            }
        ]
    );
    assert!(vault.remove("dev", "token").unwrap());
    assert!(!vault.remove("dev", "token").unwrap());
}

#[test]
fn a_value_moved_to_another_entry_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::in_dir(dir.path(), "col_1").unwrap();
    let master = Key::generate().unwrap();
    vault.set(&master, "prod", "token", "t0k").unwrap();
    let text = std::fs::read_to_string(vault.file()).unwrap();
    std::fs::write(vault.file(), text.replace("\"prod\"", "\"dev\"")).unwrap();
    assert!(vault.get(&master, "dev", "token").is_err());
}

#[test]
fn reading_secrets_never_makes_a_master_key() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::default();
    let secrets = VaultSecrets::with_backends(
        dir.path(),
        "col_1",
        Arc::new(MemorySharedSecrets::default()),
        &keyring,
        KeyFile::in_dir(dir.path()),
    )
    .unwrap();
    assert_eq!(secrets.secret("prod", "token").unwrap(), None);
    assert!(keyring.stored.lock().unwrap().is_none());
    assert!(!dir.path().join("master.key").exists());

    secrets.set_local("prod", "token", "t0k").unwrap();
    assert!(
        keyring.stored.lock().unwrap().is_some(),
        "made when storing"
    );
    assert_eq!(
        secrets
            .secret("prod", "token")
            .unwrap()
            .as_ref()
            .map(Secret::expose),
        Some("t0k")
    );
}

#[test]
fn a_locked_keyring_is_an_error_and_is_asked_again_once_unlocked() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::default();
    let secrets = VaultSecrets::with_backends(
        dir.path(),
        "col_1",
        Arc::new(MemorySharedSecrets::default()),
        &keyring,
        KeyFile::in_dir(dir.path()),
    )
    .unwrap();
    secrets.set_local("prod", "token", "t0k").unwrap();
    let later = VaultSecrets::with_backends(
        dir.path(),
        "col_1",
        Arc::new(MemorySharedSecrets::default()),
        &keyring,
        KeyFile::in_dir(dir.path()),
    )
    .unwrap();
    keyring.locked.store(true, Ordering::Relaxed);
    assert!(later.secret("prod", "token").is_err());
    keyring.locked.store(false, Ordering::Relaxed);
    assert_eq!(
        later
            .secret("prod", "token")
            .unwrap()
            .as_ref()
            .map(Secret::expose),
        Some("t0k")
    );
    // Once read, the key is kept: no more trips to the keyring.
    let reads = *keyring.reads.lock().unwrap();
    later.secret("prod", "token").unwrap();
    assert_eq!(*keyring.reads.lock().unwrap(), reads);
}
