//! Renaming and deleting environment files.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::fs;

use apifast_app::{EnvironmentRepository, RepositoryError};
use apifast_storage_fs::FsCollection;

fn setup() -> (tempfile::TempDir, FsCollection) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: c\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("environments")).unwrap();
    fs::write(
        dir.path().join("environments/local.yml"),
        "# Local machine\nname: local\ncolor: \"#22aa22\"\nvariables:\n  - name: host\n    value: http://localhost\nx-custom: kept\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("environments/prod.yml"),
        "name: prod\nvariables: []\n",
    )
    .unwrap();
    let repo = FsCollection::open(dir.path()).unwrap();
    (dir, repo)
}

#[test]
fn renaming_moves_the_file_and_keeps_the_rest_of_it() {
    let (dir, repo) = setup();
    repo.rename_environment("local", "dev box").unwrap();
    assert!(!dir.path().join("environments/local.yml").exists());
    let text = fs::read_to_string(dir.path().join("environments/dev box.yml")).unwrap();
    assert!(text.contains("name: dev box"), "{text}");
    for kept in [
        "# Local machine",
        "x-custom: kept",
        "http://localhost",
        "#22aa22",
    ] {
        assert!(text.contains(kept), "{kept} in {text}");
    }
    let names: Vec<_> = repo
        .environments()
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["dev box", "prod"]);
}

#[test]
fn renaming_to_a_taken_name_or_from_a_missing_one_fails() {
    let (_dir, repo) = setup();
    assert!(matches!(
        repo.rename_environment("local", "prod"),
        Err(RepositoryError::Invalid { .. })
    ));
    assert!(matches!(
        repo.rename_environment("nope", "x"),
        Err(RepositoryError::NotFound(_))
    ));
}

#[test]
fn deleting_removes_the_file() {
    let (dir, repo) = setup();
    repo.delete_environment("prod").unwrap();
    assert!(!dir.path().join("environments/prod.yml").exists());
    assert!(matches!(
        repo.delete_environment("prod"),
        Err(RepositoryError::NotFound(_))
    ));
}
