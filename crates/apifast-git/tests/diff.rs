//! What changed in each file of a real collection since its last commit,
//! read with the storage's own reader and gix.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::{
    CollectionRepository, EnvironmentRepository, FileChange, GitRepository, RequestPath, git_diff,
};
use apifast_domain::{
    Collection, Environment, FieldChange, Folder, Header, HttpRequest, Method, Variable,
};
use apifast_git::GixRepository;
use apifast_git_contract::{git, repo, write};
use apifast_storage_fs::FsCollection;

fn change(field: &str, before: Option<&str>, after: Option<&str>) -> FieldChange {
    FieldChange {
        field: field.into(),
        before: before.map(str::to_owned),
        after: after.map(str::to_owned),
    }
}

#[test]
fn each_changed_file_says_what_changed_in_it() {
    let dir = repo();
    let root = dir.path();
    let collection = FsCollection::create(root, &Collection::new("Shop")).unwrap();
    let mut request = HttpRequest::new("List", Method::Get, "{{host}}/users");
    request.headers = vec![Header::new("Accept", "application/json")];
    collection
        .save_request(&RequestPath::new("users/list.yml"), &request)
        .unwrap();
    collection
        .save_folder("users", &Folder::new("Users"))
        .unwrap();
    collection
        .save_environment(&Environment::new("dev", vec![Variable::text("host", "a")]))
        .unwrap();
    write(root, "notes.txt", "one\n");
    write(
        root,
        "gone.yml",
        "info:\n  name: Gone\n  type: http\nhttp:\n  method: GET\n  url: x\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);

    request.method = Method::Post;
    request.headers.push(Header::new("X-Team", "core"));
    collection
        .save_request(&RequestPath::new("users/list.yml"), &request)
        .unwrap();
    let mut folder = Folder::new("Users");
    folder.tags = vec!["users".into()];
    collection.save_folder("users", &folder).unwrap();
    collection
        .save_environment(&Environment::new("dev", vec![Variable::text("host", "b")]))
        .unwrap();
    write(root, "notes.txt", "two\n");
    std::fs::remove_file(root.join("gone.yml")).unwrap();
    collection
        .save_request(
            &RequestPath::new("new.yml"),
            &HttpRequest::new("New", Method::Get, "x"),
        )
        .unwrap();

    let git_repo = GixRepository::discover(root).unwrap().unwrap();
    let status = git_repo.status().unwrap();
    let diffs: Vec<_> = (status.changes.iter())
        .map(|file| git_diff(&collection, &git_repo, file).unwrap())
        .map(|diff| (diff.path, diff.change, diff.fields))
        .collect();
    assert_eq!(
        diffs,
        [
            (
                "environments/dev.yml".to_owned(),
                FileChange::Modified,
                vec![change("variable host", Some("a"), Some("b"))]
            ),
            ("gone.yml".to_owned(), FileChange::Deleted, vec![]),
            ("new.yml".to_owned(), FileChange::Added, vec![]),
            (
                "notes.txt".to_owned(),
                FileChange::Modified,
                vec![change("content", Some("one\n"), Some("two\n"))]
            ),
            (
                "users/folder.yml".to_owned(),
                FileChange::Modified,
                vec![change("tags", None, Some("users"))]
            ),
            (
                "users/list.yml".to_owned(),
                FileChange::Modified,
                vec![
                    change("method", Some("GET"), Some("POST")),
                    change("header x-team", None, Some("X-Team: core")),
                ]
            ),
        ]
    );
    // Only what changed: the other environment reader is not confused.
    assert_eq!(collection.environments().unwrap().len(), 1);
}
