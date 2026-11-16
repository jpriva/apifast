//! Collection keys wrapped with the master key, and the challenge that
//! tells whether a key is the collection's.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_vault::{
    CollectionKeyStore, Key, KeyStoreError, VaultError, check_challenge, new_challenge,
};

#[test]
fn a_collection_key_comes_back_only_with_its_master_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = CollectionKeyStore::in_dir(dir.path());
    let (master, key) = (Key::generate().unwrap(), Key::generate().unwrap());
    assert!(store.load(&master, "col_1").unwrap().is_none());
    store.save(&master, "col_1", &key).unwrap();
    assert_eq!(
        store.load(&master, "col_1").unwrap().unwrap().as_bytes(),
        key.as_bytes()
    );
    let file = dir.path().join("keys/col_1.key");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("enc:v1:"), "wrapped, never in the clear");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let other_master = Key::generate().unwrap();
    assert_eq!(
        store.load(&other_master, "col_1").unwrap_err(),
        KeyStoreError::Vault(VaultError::Rejected)
    );
}

#[test]
fn a_key_file_copied_to_another_collection_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let store = CollectionKeyStore::in_dir(dir.path());
    let master = Key::generate().unwrap();
    store
        .save(&master, "col_1", &Key::generate().unwrap())
        .unwrap();
    std::fs::copy(
        dir.path().join("keys/col_1.key"),
        dir.path().join("keys/col_2.key"),
    )
    .unwrap();
    assert!(store.load(&master, "col_2").is_err());
}

#[test]
fn ids_that_could_leave_the_folder_are_refused_and_keys_can_be_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let store = CollectionKeyStore::in_dir(dir.path());
    let master = Key::generate().unwrap();
    for bad in ["", "../x", "a/b", "col 1"] {
        assert!(store.load(&master, bad).is_err(), "{bad}");
    }
    store
        .save(&master, "path-0123abcd", &Key::generate().unwrap())
        .unwrap();
    store.remove("path-0123abcd").unwrap();
    store.remove("path-0123abcd").unwrap();
    assert!(store.load(&master, "path-0123abcd").unwrap().is_none());
}

#[test]
fn the_challenge_tells_the_right_key_and_its_collection() {
    let key = Key::generate().unwrap();
    let challenge = new_challenge(&key, "col_1").unwrap();
    assert_eq!(check_challenge(&key, "col_1", &challenge), Ok(()));
    let wrong = Key::generate().unwrap();
    assert_eq!(
        check_challenge(&wrong, "col_1", &challenge),
        Err(VaultError::Rejected)
    );
    assert_eq!(
        check_challenge(&key, "col_2", &challenge),
        Err(VaultError::Rejected)
    );
    assert_ne!(challenge, new_challenge(&key, "col_1").unwrap());
}
