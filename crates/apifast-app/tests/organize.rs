//! The sidebar use cases on the in-memory repository: names, files, order.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::memory::MemoryCollections;
use apifast_app::{
    AppError, CollectionRepository, ItemPath, Placement, RequestPath, TreeItem, create_folder,
    create_request, delete_item, move_item, open_collection, rename_item, slug,
};
use apifast_domain::{Collection, Folder, HttpRequest, Method};

fn request(name: &str, seq: u32) -> HttpRequest {
    let mut request = HttpRequest::new(name, Method::Get, "{{host}}");
    request.seq = Some(seq);
    request
}

fn folder(name: &str, seq: u32) -> Folder {
    let mut folder = Folder::new(name);
    folder.seq = Some(seq);
    folder
}

/// `users/` with two requests, then `login.yml` at the root.
fn repo() -> MemoryCollections {
    MemoryCollections::new(Collection::new("API"))
        .with_folder("users", folder("Users", 1))
        .with_request("users/list.yml", request("List", 1))
        .with_request("users/create.yml", request("Create", 2))
        .with_request("login.yml", request("Login", 2))
}

fn req(path: &str) -> ItemPath {
    ItemPath::Request(RequestPath::new(path))
}

/// Names in tree order, with folders' children inside brackets.
fn outline(repo: &MemoryCollections) -> String {
    fn walk(items: &[TreeItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                TreeItem::Folder {
                    folder, children, ..
                } => {
                    let mut inner = Vec::new();
                    walk(children, &mut inner);
                    out.push(format!("{}[{}]", folder.name, inner.join(" ")));
                }
                TreeItem::Request { name, .. } | TreeItem::Other { name, .. } => {
                    out.push(name.clone());
                }
                TreeItem::Invalid { path, .. } => out.push(path.to_string()),
            }
        }
    }
    let mut out = Vec::new();
    walk(&open_collection(repo).unwrap().items, &mut out);
    out.join(" ")
}

#[test]
fn slugs_are_lowercase_words_joined_by_dashes() {
    assert_eq!(slug("List users", "x"), "list-users");
    assert_eq!(slug("  Crear Usuario (v2)! ", "x"), "crear-usuario-v2");
    assert_eq!(slug("Ñandú café", "x"), "ñandú-café");
    assert_eq!(slug("¿?", "request"), "request");
}

#[test]
fn new_requests_go_last_with_a_free_file_name() {
    let repo = repo();
    let path = create_request(&repo, "users", "List").unwrap();
    assert_eq!(path.as_str(), "users/list-2.yml");
    let created = repo.get("users/list-2.yml").unwrap();
    assert_eq!(
        (created.name.as_str(), created.seq, created.method),
        ("List", Some(3), Method::Get)
    );
    assert_eq!(outline(&repo), "Users[List Create List] Login");
}

#[test]
fn new_folders_go_last_in_their_parent() {
    let repo = repo();
    assert_eq!(
        create_folder(&repo, "", "Admin Tools").unwrap(),
        "admin-tools"
    );
    assert_eq!(
        create_folder(&repo, "admin-tools", "Keys").unwrap(),
        "admin-tools/keys"
    );
    assert_eq!(repo.folder("admin-tools").unwrap().seq, Some(3));
    assert_eq!(
        outline(&repo),
        "Users[List Create] Login Admin Tools[Keys[]]"
    );
}

#[test]
fn blank_names_and_missing_folders_are_refused() {
    let repo = repo();
    assert!(matches!(
        create_request(&repo, "", "   "),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        create_request(&repo, "nope", "X"),
        Err(AppError::Repository(_))
    ));
}

#[test]
fn renaming_changes_the_name_and_the_file() {
    let repo = repo();
    let renamed = rename_item(&repo, &req("users/list.yml"), "List all").unwrap();
    assert_eq!(renamed, req("users/list-all.yml"));
    assert!(repo.get("users/list.yml").is_none());
    assert_eq!(repo.get("users/list-all.yml").unwrap().name, "List all");

    let renamed = rename_item(&repo, &ItemPath::Folder("users".into()), "People").unwrap();
    assert_eq!(renamed, ItemPath::Folder("people".into()));
    assert_eq!(repo.folder("people").unwrap().name, "People");
    assert_eq!(repo.get("people/list-all.yml").unwrap().name, "List all");
}

#[test]
fn renaming_to_a_name_with_the_same_file_keeps_the_path() {
    let repo = repo();
    let renamed = rename_item(&repo, &req("login.yml"), "LOGIN").unwrap();
    assert_eq!(renamed, req("login.yml"));
    assert_eq!(repo.get("login.yml").unwrap().name, "LOGIN");
}

#[test]
fn deleting_a_folder_deletes_what_it_holds() {
    let repo = repo();
    delete_item(&repo, &ItemPath::Folder("users".into())).unwrap();
    assert_eq!(outline(&repo), "Login");
    assert!(!repo.exists("users/list.yml").unwrap());
}

#[test]
fn moving_inside_a_folder_renumbers_it() {
    let repo = repo();
    let moved = move_item(
        &repo,
        &req("users/create.yml"),
        &Placement::Before(req("users/list.yml")),
    )
    .unwrap();
    assert_eq!(moved, req("users/create.yml"));
    assert_eq!(outline(&repo), "Users[Create List] Login");
    assert_eq!(repo.get("users/create.yml").unwrap().seq, Some(1));
    assert_eq!(repo.get("users/list.yml").unwrap().seq, Some(2));
}

#[test]
fn moving_to_another_folder_moves_the_file() {
    let repo = repo();
    let moved = move_item(
        &repo,
        &req("login.yml"),
        &Placement::After(req("users/list.yml")),
    )
    .unwrap();
    assert_eq!(moved, req("users/login.yml"));
    assert_eq!(outline(&repo), "Users[List Login Create]");

    let moved = move_item(
        &repo,
        &req("users/list.yml"),
        &Placement::Inside(String::new()),
    )
    .unwrap();
    assert_eq!(moved, req("list.yml"));
    assert_eq!(outline(&repo), "Users[Login Create] List");
}

#[test]
fn a_folder_cannot_go_inside_itself_and_names_do_not_collide() {
    let repo = repo().with_request("list.yml", request("Root list", 3));
    assert!(matches!(
        move_item(
            &repo,
            &ItemPath::Folder("users".into()),
            &Placement::Inside("users".into())
        ),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        move_item(&repo, &req("list.yml"), &Placement::Inside("users".into())),
        Err(AppError::Validation(_))
    ));
    assert_eq!(outline(&repo), "Users[List Create] Login Root list");
}

#[test]
fn a_new_item_goes_last_even_after_items_without_seq() {
    let repo = repo().with_request("health.yml", HttpRequest::new("Health", Method::Get, ""));
    create_request(&repo, "", "New").unwrap();
    assert_eq!(outline(&repo), "Users[List Create] Login Health New");
    assert_eq!(repo.get("health.yml").unwrap().seq, Some(3));
}
