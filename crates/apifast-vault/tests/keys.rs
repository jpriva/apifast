//! Where the master key lives: the keyring first, the file only without a
//! keyring, and never a second key when the keyring refuses.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::cell::RefCell;

use apifast_vault::{KeyBackend, KeyFile, KeyLocation, KeyStoreError, master_key};

/// A keyring in memory: present or not, refusing or not.
#[derive(Default)]
struct FakeKeyring {
    absent: bool,
    locked: bool,
    stored: RefCell<Option<Vec<u8>>>,
}

impl KeyBackend for FakeKeyring {
    fn available(&self) -> bool {
        !self.absent
    }

    fn get(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        if self.locked {
            return Err(KeyStoreError::Keyring("locked".into()));
        }
        Ok(self.stored.borrow().clone())
    }

    fn set(&self, bytes: &[u8]) -> Result<(), KeyStoreError> {
        if self.locked {
            return Err(KeyStoreError::Keyring("locked".into()));
        }
        *self.stored.borrow_mut() = Some(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn a_new_key_goes_to_the_keyring_and_is_found_there_again() {
    let dir = tempfile::tempdir().unwrap();
    let (keyring, file) = (FakeKeyring::default(), KeyFile::in_dir(dir.path()));
    let (first, location) = master_key(&keyring, &file).unwrap();
    assert_eq!(location, KeyLocation::Keyring);
    assert!(!file.path().exists(), "no copy in a file");
    let (again, _) = master_key(&keyring, &file).unwrap();
    assert_eq!(first.as_bytes(), again.as_bytes());
}

#[test]
fn without_a_keyring_the_key_goes_to_a_file_only_its_owner_reads() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring {
        absent: true,
        ..FakeKeyring::default()
    };
    let file = KeyFile::in_dir(&dir.path().join("data"));
    let (first, location) = master_key(&keyring, &file).unwrap();
    assert_eq!(location, KeyLocation::File);
    assert_eq!(std::fs::read(file.path()).unwrap(), first.as_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let (again, _) = master_key(&keyring, &file).unwrap();
    assert_eq!(first.as_bytes(), again.as_bytes());
}

#[test]
fn a_locked_keyring_is_an_error_not_a_second_key() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring {
        locked: true,
        ..FakeKeyring::default()
    };
    let file = KeyFile::in_dir(dir.path());
    assert!(matches!(
        master_key(&keyring, &file),
        Err(KeyStoreError::Keyring(_))
    ));
    assert!(!file.path().exists());
}

#[test]
fn a_key_made_without_a_keyring_stays_in_its_file_once_there_is_one() {
    let dir = tempfile::tempdir().unwrap();
    let file = KeyFile::in_dir(dir.path());
    let absent = FakeKeyring {
        absent: true,
        ..FakeKeyring::default()
    };
    let (first, _) = master_key(&absent, &file).unwrap();
    let keyring = FakeKeyring::default();
    let (again, location) = master_key(&keyring, &file).unwrap();
    assert_eq!(location, KeyLocation::File);
    assert_eq!(first.as_bytes(), again.as_bytes());
}

#[test]
fn a_damaged_key_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let file = KeyFile::in_dir(dir.path());
    std::fs::write(file.path(), b"short").unwrap();
    let absent = FakeKeyring {
        absent: true,
        ..FakeKeyring::default()
    };
    assert_eq!(
        master_key(&absent, &file).unwrap_err(),
        KeyStoreError::Damaged
    );
}

#[cfg(unix)]
#[test]
fn a_key_file_others_can_read_is_closed_again() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let file = KeyFile::in_dir(dir.path());
    std::fs::write(file.path(), [1; 32]).unwrap();
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let absent = FakeKeyring {
        absent: true,
        ..FakeKeyring::default()
    };
    master_key(&absent, &file).unwrap();
    let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
