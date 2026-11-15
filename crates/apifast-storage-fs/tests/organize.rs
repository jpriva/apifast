//! The sidebar use cases on a collection on disk: files and folders move,
//! and only the `seq` lines that change are written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;

use apifast_app::{
    CollectionRepository, ItemPath, Placement, RepositoryError, RequestPath, create_folder,
    create_request, delete_item, move_item, rename_item,
};
use apifast_storage_fs::FsCollection;
use tempfile::TempDir;

fn setup() -> (TempDir, FsCollection) {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: API\n",
        ),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\n  seq: 1\n",
        ),
        (
            "users/list.yml",
            "# keep me\ninfo:\n  name: List\n  type: http\n  seq: 1\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n",
        ),
        (
            "users/create.yml",
            "info:\n  name: Create\n  type: http\n  seq: 2\nhttp:\n  method: POST\n  url: \"{{host}}/users\"\n",
        ),
        ("environments/local.yml", "name: local\nvariables: []\n"),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }
    let repo = FsCollection::open(dir.path()).unwrap();
    (dir, repo)
}

fn read(dir: &TempDir, path: &str) -> String {
    fs::read_to_string(dir.path().join(path)).unwrap()
}

fn req(path: &str) -> ItemPath {
    ItemPath::Request(RequestPath::new(path))
}

#[test]
fn creates_requests_and_folders_as_files() {
    let (dir, repo) = setup();
    let path = create_request(&repo, "users", "Delete user").unwrap();
    assert_eq!(path.as_str(), "users/delete-user.yml");
    let text = read(&dir, "users/delete-user.yml");
    assert!(text.contains("name: Delete user"), "{text}");
    assert!(text.contains("seq: 3"), "{text}");

    assert_eq!(create_folder(&repo, "", "Admin").unwrap(), "admin");
    let text = read(&dir, "admin/folder.yml");
    assert!(
        text.contains("name: Admin") && text.contains("seq: 2"),
        "{text}"
    );
}

#[test]
fn renaming_keeps_the_rest_of_the_file() {
    let (dir, repo) = setup();
    let renamed = rename_item(&repo, &req("users/list.yml"), "List all").unwrap();
    assert_eq!(renamed, req("users/list-all.yml"));
    assert!(!dir.path().join("users/list.yml").exists());
    assert_eq!(
        read(&dir, "users/list-all.yml"),
        "# keep me\ninfo:\n  name: List all\n  type: http\n  seq: 1\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n"
    );

    let renamed = rename_item(&repo, &ItemPath::Folder("users".into()), "People").unwrap();
    assert_eq!(renamed, ItemPath::Folder("people".into()));
    assert!(dir.path().join("people/list-all.yml").exists());
    assert!(read(&dir, "people/folder.yml").contains("name: People"));
}

#[test]
fn reordering_writes_only_the_seq_lines() {
    let (dir, repo) = setup();
    move_item(
        &repo,
        &req("users/create.yml"),
        &Placement::Before(req("users/list.yml")),
    )
    .unwrap();
    assert_eq!(
        read(&dir, "users/list.yml"),
        "# keep me\ninfo:\n  name: List\n  type: http\n  seq: 2\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n"
    );
    assert!(read(&dir, "users/create.yml").contains("seq: 1"));
}

#[test]
fn moving_a_request_out_of_its_folder_moves_the_file() {
    let (dir, repo) = setup();
    let moved = move_item(
        &repo,
        &req("users/create.yml"),
        &Placement::Inside(String::new()),
    )
    .unwrap();
    assert_eq!(moved, req("create.yml"));
    assert!(!dir.path().join("users/create.yml").exists());
    assert!(read(&dir, "create.yml").contains("seq: 2"));
}

#[test]
fn deletes_files_and_folders() {
    let (dir, repo) = setup();
    delete_item(&repo, &req("users/list.yml")).unwrap();
    assert!(!dir.path().join("users/list.yml").exists());
    delete_item(&repo, &ItemPath::Folder("users".into())).unwrap();
    assert!(!dir.path().join("users").exists());
}

#[test]
fn the_collection_files_and_reserved_folders_cannot_be_moved_or_deleted() {
    let (dir, repo) = setup();
    for path in ["opencollection.yml", "environments", ".git", "../outside"] {
        assert!(
            matches!(repo.delete_path(path), Err(RepositoryError::Invalid { .. })),
            "{path}"
        );
    }
    assert!(matches!(
        repo.rename_path("users", "environments"),
        Err(RepositoryError::Invalid { .. })
    ));
    assert!(dir.path().join("environments/local.yml").exists());
}

#[test]
fn moving_never_replaces_an_existing_file() {
    let (dir, repo) = setup();
    assert!(matches!(
        repo.rename_path("users/list.yml", "users/create.yml"),
        Err(RepositoryError::Invalid { .. })
    ));
    assert!(read(&dir, "users/create.yml").contains("name: Create"));
    assert!(matches!(
        repo.rename_path("users/nope.yml", "users/x.yml"),
        Err(RepositoryError::NotFound(_))
    ));
}
