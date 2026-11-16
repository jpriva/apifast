//! The vault's challenge in `.apifast/collection.yml`.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::RepositoryError;
use apifast_app::SharedSecretFiles as _;
use apifast_app::memory::CountingRandom;
use apifast_storage_fs::FsCollection;

fn setup() -> (tempfile::TempDir, FsCollection) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: c\n",
    )
    .unwrap();
    let repo = FsCollection::open(dir.path()).unwrap();
    (dir, repo)
}

#[test]
fn a_challenge_needs_the_collection_id_first() {
    let (_dir, repo) = setup();
    assert_eq!(repo.vault_challenge().unwrap(), None);
    assert!(matches!(
        repo.set_vault_challenge("enc:v1:abc"),
        Err(RepositoryError::NotFound(_))
    ));
}

#[test]
fn the_challenge_is_written_next_to_the_id_and_replaced() {
    let (dir, repo) = setup();
    let id = repo.ensure_id(&CountingRandom::new(1)).unwrap();
    let file = dir.path().join(".apifast/collection.yml");
    // A comment someone added stays.
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("# shared with the team\n{text}")).unwrap();

    repo.set_vault_challenge("enc:v1:first").unwrap();
    assert_eq!(
        repo.vault_challenge().unwrap().as_deref(),
        Some("enc:v1:first")
    );
    repo.set_vault_challenge("enc:v1:second").unwrap();
    assert_eq!(
        repo.vault_challenge().unwrap().as_deref(),
        Some("enc:v1:second")
    );

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("# shared with the team"), "{text}");
    assert!(text.contains(id.as_str()), "{text}");
    assert!(
        text.contains("vault:\n  challenge: enc:v1:second"),
        "{text}"
    );
}

#[test]
fn shared_secrets_live_in_one_file_per_environment() {
    let (dir, repo) = setup();
    assert_eq!(repo.shared_secret("prod", "token").unwrap(), None);
    repo.set_shared_secret("prod", "token", "enc:v1:a").unwrap();
    repo.set_shared_secret("prod", "key", "enc:v1:b").unwrap();
    repo.set_shared_secret("prod/eu", "token", "enc:v1:c")
        .unwrap();
    repo.set_shared_secret("prod", "token", "enc:v1:a2")
        .unwrap();

    assert_eq!(
        repo.shared_secret("prod", "token").unwrap().as_deref(),
        Some("enc:v1:a2")
    );
    assert_eq!(
        repo.shared_secret("prod/eu", "token").unwrap().as_deref(),
        Some("enc:v1:c")
    );
    let text = std::fs::read_to_string(dir.path().join(".apifast/secrets/prod.yml")).unwrap();
    assert!(text.starts_with("environment: prod\nsecrets:"), "{text}");
    // A name that is not a file name keeps its real name inside the file.
    let other = std::fs::read_to_string(dir.path().join(".apifast/secrets/prod-eu.yml")).unwrap();
    assert!(other.contains("environment: prod/eu"), "{other}");

    let mut entries = repo.shared_secret_entries().unwrap();
    entries.sort();
    assert_eq!(
        entries,
        [
            ("prod".to_owned(), "key".to_owned()),
            ("prod".to_owned(), "token".to_owned()),
            ("prod/eu".to_owned(), "token".to_owned()),
        ]
    );

    assert!(repo.remove_shared_secret("prod/eu", "token").unwrap());
    assert!(!repo.remove_shared_secret("prod/eu", "token").unwrap());
    assert!(
        !dir.path().join(".apifast/secrets/prod-eu.yml").exists(),
        "empty files go"
    );
}
