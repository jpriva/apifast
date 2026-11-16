//! The file system repository against real folders.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;
use std::path::Path;

use apifast_app::memory::CountingRandom;
use apifast_app::{
    CollectionRepository, EnvironmentRepository, RepositoryError, RequestPath, TreeItem,
};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, BasicAuth, BearerAuth, Body, Collection, Environment,
    Folder, FormField, Header, HttpRequest, Method, Param, ParamKind, Setting, ValueKind, Variable,
    VariableValue,
};
use apifast_storage_fs::FsCollection;
use tempfile::TempDir;

const COLLECTION: &str = "opencollection: 1.0.0\n\ninfo:\n  name: Test\n";

fn setup(files: &[(&str, &str)]) -> (TempDir, FsCollection) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("opencollection.yml"), COLLECTION).unwrap();
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

fn path(p: &str) -> RequestPath {
    RequestPath::new(p)
}

fn load(repo: &FsCollection, p: &str) -> HttpRequest {
    repo.request(&path(p)).unwrap().request
}

/// Lines that differ between two texts, as `-old` / `+new`, from a line
/// diff (removals before additions within each change).
fn changed_lines(before: &str, after: &str) -> Vec<String> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut out, mut removed, mut added) = (Vec::new(), Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    let flush = |out: &mut Vec<String>, removed: &mut Vec<String>, added: &mut Vec<String>| {
        out.append(removed);
        out.append(added);
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            flush(&mut out, &mut removed, &mut added);
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            added.push(format!("+{}", b[j]));
            j += 1;
        } else {
            removed.push(format!("-{}", a[i]));
            i += 1;
        }
    }
    flush(&mut out, &mut removed, &mut added);
    out
}

/// Edits the request at `p`, saves it, checks it reads back as edited, and
/// returns the lines that changed in the file.
fn edit(
    dir: &TempDir,
    repo: &FsCollection,
    p: &str,
    change: impl FnOnce(&mut HttpRequest),
) -> Vec<String> {
    let before = read(dir, p);
    let mut request = load(repo, p);
    change(&mut request);
    repo.save_request(&path(p), &request).unwrap();
    assert_eq!(
        load(repo, p),
        request,
        "the saved request must read back as saved"
    );
    changed_lines(&before, &read(dir, p))
}

const LOGIN: &str = "\
info:
  name: Login
  type: http
  seq: 1

# Credentials come from the vault.
http:
  method: POST
  url: \"{{host}}/login\"
  headers:
    - name: accept # json only
      value: application/json
      description: The API speaks JSON
    - name: x-trace
      value: \"1\"
  body:
    type: json
    data: |-
      {
        \"user\": \"{{user}}\"
      }
  auth:
    type: bearer
    token: \"{{token}}\"

runtime:
  scripts:
    - type: after-response
      code: bru.setVar(\"t\", res.body.token)
  assertions:
    - expression: res.status
      operator: eq
      value: \"200\"

settings:
  encodeUrl: true
  timeout: 0
  followRedirects: true
  maxRedirects: 5
  forwardAuthorizationHeader: true

docs: Logs in.
";

#[test]
fn saving_an_unchanged_request_leaves_the_file_identical() {
    let (dir, repo) = setup(&[("auth/login.yml", LOGIN)]);
    let request = load(&repo, "auth/login.yml");
    repo.save_request(&path("auth/login.yml"), &request)
        .unwrap();
    assert_eq!(read(&dir, "auth/login.yml"), LOGIN);
}

#[test]
fn changing_the_url_changes_one_line_and_keeps_comments() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let changes = edit(&dir, &repo, "login.yml", |r| {
        r.url = "{{host}}/v2/login".into();
    });
    assert_eq!(
        changes,
        [
            "-  url: \"{{host}}/login\"",
            "+  url: \"{{host}}/v2/login\""
        ]
    );
    let text = read(&dir, "login.yml");
    assert!(text.contains("# Credentials come from the vault."));
    assert!(text.contains("- name: accept # json only"));
}

#[test]
fn removing_the_first_header_keeps_the_rest_untouched() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let changes = edit(&dir, &repo, "login.yml", |r| {
        r.headers.remove(0);
    });
    assert_eq!(
        changes,
        [
            "-    - name: accept # json only",
            "-      value: application/json",
            "-      description: The API speaks JSON",
        ]
    );
}

#[test]
fn editing_a_header_keeps_its_unknown_keys() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let changes = edit(&dir, &repo, "login.yml", |r| {
        r.headers[0].value = "text/plain".into();
    });
    assert_eq!(
        changes,
        ["-      value: application/json", "+      value: text/plain"]
    );
    assert!(read(&dir, "login.yml").contains("description: The API speaks JSON"));
}

#[test]
fn inserting_and_disabling_headers() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    edit(&dir, &repo, "login.yml", |r| {
        r.headers.insert(0, Header::new("x-first", "1"));
        r.headers.push(Header::new("x-last", "2"));
        r.headers[1].enabled = false;
    });
    let text = read(&dir, "login.yml");
    assert!(text.contains("description: The API speaks JSON"));
    let loaded = load(&repo, "login.yml");
    let names: Vec<_> = loaded
        .headers
        .iter()
        .map(|h| (h.name.as_str(), h.enabled))
        .collect();
    assert_eq!(
        names,
        [
            ("x-first", true),
            ("accept", false),
            ("x-trace", true),
            ("x-last", true)
        ]
    );
}

#[test]
fn scripts_assertions_and_unknown_settings_survive_edits() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    edit(&dir, &repo, "login.yml", |r| {
        r.name = "Sign in".into();
        r.method = Method::Put;
        r.docs = None;
        r.settings.timeout_ms = Setting::Set(5000);
    });
    let text = read(&dir, "login.yml");
    for kept in [
        "bru.setVar",
        "operator: eq",
        "forwardAuthorizationHeader: true",
        "# Credentials",
    ] {
        assert!(text.contains(kept), "lost `{kept}`:\n{text}");
    }
    assert!(!text.contains("docs:"));
    assert!(text.contains("timeout: 5000"));
}

#[test]
fn changing_the_bearer_token_rewrites_only_the_token() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let changes = edit(&dir, &repo, "login.yml", |r| {
        r.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "{{other}}".into(),
        }));
    });
    assert_eq!(
        changes,
        ["-    token: \"{{token}}\"", "+    token: \"{{other}}\""]
    );
}

#[test]
fn switching_auth_schemes() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    edit(&dir, &repo, "login.yml", |r| {
        r.auth = Setting::Set(AuthScheme::Basic(BasicAuth {
            username: "u".into(),
            password: "{{pass}}".into(),
        }));
    });
    assert!(!read(&dir, "login.yml").contains("bearer"));

    edit(&dir, &repo, "login.yml", |r| {
        r.auth = Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
            key: "X-Key".into(),
            value: "{{key}}".into(),
            placement: ApiKeyPlacement::Query,
        }));
    });
    // `edit` already checked that it reads back as an API key in the query.
    let text = read(&dir, "login.yml");
    assert!(
        text.contains("type: apikey") && !text.contains("username"),
        "{text}"
    );

    edit(&dir, &repo, "login.yml", |r| r.auth = Setting::Inherit);
    assert!(read(&dir, "login.yml").contains("  auth: inherit\n"));

    // No auth is written as no key, as Bruno does.
    edit(&dir, &repo, "login.yml", |r| {
        r.auth = Setting::Set(AuthScheme::None);
    });
    assert!(!read(&dir, "login.yml").contains("auth:"));
}

#[test]
fn an_unsupported_auth_survives_other_edits() {
    let oauth = "info:\n  name: O\n  type: http\nhttp:\n  method: GET\n  url: x\n  auth:\n    type: oauth2\n    flow: client_credentials\n    clientId: abc\n";
    let (dir, repo) = setup(&[("o.yml", oauth)]);
    assert_eq!(
        load(&repo, "o.yml").auth,
        Setting::Set(AuthScheme::Unsupported("oauth2".into()))
    );
    edit(&dir, &repo, "o.yml", |r| r.url = "y".into());
    assert!(read(&dir, "o.yml").contains("clientId: abc"));
}

#[test]
fn legacy_runtime_auth_moves_to_http_only_when_changed() {
    let legacy = "info:\n  name: L\n  type: http\nhttp:\n  method: GET\n  url: x\nruntime:\n  auth: inherit\n";
    let (dir, repo) = setup(&[("l.yml", legacy)]);
    edit(&dir, &repo, "l.yml", |r| r.url = "y".into());
    assert!(read(&dir, "l.yml").contains("runtime:\n  auth: inherit"));

    edit(&dir, &repo, "l.yml", |r| {
        r.auth = Setting::Set(AuthScheme::Bearer(BearerAuth { token: "t".into() }));
    });
    let text = read(&dir, "l.yml");
    assert!(!text.contains("runtime:"), "{text}");
    assert!(text.contains("type: bearer"));
}

#[test]
fn settings_blocks_keep_bruno_semantics() {
    let bare = "info:\n  name: B\n  type: http\nhttp:\n  method: GET\n  url: x\n";
    let (dir, repo) = setup(&[("b.yml", bare)]);

    // Setting one value creates the block; the timeout must then say `inherit`,
    // because a block without timeout means 0 (no limit) to Bruno.
    edit(&dir, &repo, "b.yml", |r| {
        r.settings.follow_redirects = Setting::Set(false);
    });
    let text = read(&dir, "b.yml");
    assert!(text.contains("followRedirects: false"), "{text}");
    assert!(text.contains("timeout: inherit"), "{text}");

    // Inheriting everything again removes the block.
    edit(&dir, &repo, "b.yml", |r| {
        r.settings = apifast_domain::Settings::default();
    });
    assert!(!read(&dir, "b.yml").contains("settings"));
}

#[test]
fn body_edits() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let changes = edit(&dir, &repo, "login.yml", |r| {
        r.body = Body::Json("{}".into());
    });
    assert!(changes.len() <= 5, "{changes:?}");

    edit(&dir, &repo, "login.yml", |r| {
        r.body = Body::FormUrlEncoded(vec![FormField {
            name: "a".into(),
            value: "1".into(),
            enabled: true,
        }]);
    });
    assert!(read(&dir, "login.yml").contains("type: form-urlencoded"));

    edit(&dir, &repo, "login.yml", |r| r.body = Body::None);
    assert!(!read(&dir, "login.yml").contains("body:"));
}

#[test]
fn file_bodies_update_the_selected_variant_and_keep_the_others() {
    let file = "info:\n  name: F\n  type: http\nhttp:\n  method: POST\n  url: x\n  body:\n    type: file\n    data:\n      - filePath: a.bin\n        contentType: ''\n        selected: false\n      - filePath: b.pdf\n        contentType: application/pdf\n        selected: true\n";
    let (dir, repo) = setup(&[("f.yml", file)]);
    edit(&dir, &repo, "f.yml", |r| {
        r.body = Body::File {
            path: "c.pdf".into(),
            content_type: Some("application/pdf".into()),
        };
    });
    let text = read(&dir, "f.yml");
    assert!(text.contains("filePath: a.bin"));
    assert!(text.contains("filePath: c.pdf"));
    assert!(!text.contains("b.pdf"));
}

#[test]
fn typed_variables_and_params() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    edit(&dir, &repo, "login.yml", |r| {
        r.variables = vec![
            Variable::text("page", "1"),
            Variable {
                value: Some(VariableValue {
                    kind: ValueKind::Number,
                    data: "50".into(),
                }),
                ..Variable::text("size", "")
            },
        ];
        r.params = vec![Param {
            name: "id".into(),
            value: "{{id}}".into(),
            kind: ParamKind::Path,
            enabled: true,
        }];
    });
    let text = read(&dir, "login.yml");
    assert!(text.contains("type: number"), "{text}");
    assert!(text.contains("type: path"), "{text}");
}

#[test]
fn new_requests_are_written_like_bruno() {
    let (dir, repo) = setup(&[]);
    let mut request = HttpRequest::new("List users", Method::Get, "{{host}}/users?page=1");
    request.seq = Some(2);
    request.headers = vec![Header::new("accept", "application/json")];
    request.params = vec![Param {
        name: "page".into(),
        value: "1".into(),
        kind: ParamKind::Query,
        enabled: true,
    }];
    request.settings.timeout_ms = Setting::Set(0);
    request.settings.encode_url = Setting::Set(true);
    repo.save_request(&path("users/list.yml"), &request)
        .unwrap();

    assert_eq!(
        read(&dir, "users/list.yml"),
        "info:\n  name: List users\n  type: http\n  seq: 2\n\
         \nhttp:\n  method: GET\n  url: \"{{host}}/users?page=1\"\n  headers:\n    - name: accept\n      value: application/json\n  params:\n    - name: page\n      value: \"1\"\n      type: query\n  auth: inherit\n\
         \nsettings:\n  encodeUrl: true\n  timeout: 0\n"
    );
    assert_eq!(load(&repo, "users/list.yml"), request);
}

#[test]
fn saving_unchanged_writes_nothing_and_leaves_no_temporary_files() {
    let (dir, repo) = setup(&[("login.yml", LOGIN)]);
    let file = dir.path().join("login.yml");
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms.clone()).unwrap();
    // A read-only file can still be "saved" unchanged, because nothing is written.
    repo.save_request(&path("login.yml"), &load(&repo, "login.yml"))
        .unwrap();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "restoring the test file"
    )]
    perms.set_readonly(false);
    fs::set_permissions(&file, perms).unwrap();

    edit(&dir, &repo, "login.yml", |r| r.url = "z".into());
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        names.iter().all(|n| !n.to_string_lossy().contains("tmp")),
        "{names:?}"
    );
}

#[test]
fn crlf_files_keep_their_line_endings() {
    let crlf = LOGIN.replace('\n', "\r\n");
    let (dir, repo) = setup(&[("login.yml", &crlf)]);
    edit(&dir, &repo, "login.yml", |r| r.url = "x".into());
    let text = read(&dir, "login.yml");
    assert!(
        text.contains("# Credentials come from the vault.\r\n"),
        "{text:?}"
    );
}

#[test]
fn unicode_survives() {
    let (dir, repo) = setup(&[]);
    let request = HttpRequest::new("Créer un usuario ñ 日本", Method::Post, "{{host}}/ñ");
    repo.save_request(&path("ñ/crear.yml"), &request).unwrap();
    assert_eq!(load(&repo, "ñ/crear.yml"), request);
    assert!(read(&dir, "ñ/crear.yml").contains("日本"));
}

#[test]
fn paths_cannot_escape_the_collection() {
    let (_dir, repo) = setup(&[]);
    let request = HttpRequest::new("x", Method::Get, "/");
    for bad in [
        "../x.yml",
        "/etc/x.yml",
        "a/../../x.yml",
        ".apifast/collection.yml",
        ".git/x.yml",
    ] {
        assert!(
            matches!(
                repo.save_request(&path(bad), &request),
                Err(RepositoryError::Invalid { .. })
            ),
            "saving {bad} must fail"
        );
        assert!(repo.request(&path(bad)).is_err(), "loading {bad} must fail");
    }
    assert!(matches!(
        repo.save_request(&path("x.json"), &request),
        Err(RepositoryError::Invalid { .. })
    ));
}

#[test]
fn missing_and_unsupported_requests() {
    let (_dir, repo) = setup(&[("q.yml", "info:\n  name: Q\n  type: graphql\n")]);
    assert_eq!(
        repo.request(&path("nope.yml")),
        Err(RepositoryError::NotFound("nope.yml".into()))
    );
    assert!(matches!(
        repo.request(&path("q.yml")),
        Err(RepositoryError::Unsupported { .. })
    ));
}

#[test]
fn requests_load_with_their_folder_chain() {
    let (_dir, repo) = setup(&[
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\nrequest:\n  auth: inherit\n  headers:\n    - name: x-team\n      value: core\n",
        ),
        (
            "users/admin/list.yml",
            "info:\n  name: List\n  type: http\nhttp:\n  method: GET\n  url: x\n  auth: inherit\n",
        ),
    ]);
    let loaded = repo.request(&path("users/admin/list.yml")).unwrap();
    let names: Vec<_> = loaded.folders.iter().map(|f| f.name.as_str()).collect();
    // `admin` has no folder.yml: it is named after its directory and inherits.
    assert_eq!(names, ["Users", "admin"]);
    assert_eq!(
        loaded.folders[0].defaults.headers,
        [Header::new("x-team", "core")]
    );
    assert_eq!(loaded.folders[1].defaults.auth, Setting::Inherit);
}

#[test]
fn the_tree_lists_sorted_items_and_skips_ignored_ones() {
    let (_dir, repo) = setup(&[
        ("b.yml", "info:\n  name: B\n  type: http\n  seq: 2\n"),
        ("a.yml", "info:\n  name: A\n  type: http\n  seq: 1\n"),
        ("q.yml", "info:\n  name: Q\n  type: graphql\n"),
        ("broken.yml", "info: [unclosed\n"),
        ("notes.txt", "not yaml"),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\n  seq: 1\n",
        ),
        ("users/list.yml", "info:\n  name: List\n  type: http\n"),
        ("environments/prod.yml", "name: prod\n"),
        (".apifast/collection.yml", "format: 1\nid: col_x\n"),
        ("node_modules/x.yml", "info:\n  name: X\n"),
        ("drafts/d.yml", "info:\n  name: D\n  type: http\n"),
    ]);
    fs::write(
        repo.root().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Test\nextensions:\n  bruno:\n    ignore:\n      - drafts\n",
    )
    .unwrap();

    let tree = apifast_app::open_collection(&repo).unwrap();
    let describe = |item: &TreeItem| match item {
        TreeItem::Folder {
            folder, children, ..
        } => format!("{}/{}", folder.name, children.len()),
        TreeItem::Request { name, .. } => name.clone(),
        TreeItem::Other { name, kind, .. } => format!("{name}:{kind}"),
        TreeItem::Invalid { path, .. } => format!("!{path}"),
    };
    let names: Vec<_> = tree.items.iter().map(describe).collect();
    assert_eq!(names, ["A", "Users/1", "B", "Q:graphql", "!broken.yml"]);
}

#[test]
fn folders_are_created_and_edited() {
    let (dir, repo) = setup(&[]);
    let mut folder = Folder::new("Users");
    folder.seq = Some(3);
    folder.defaults.headers = vec![Header::new("x-team", "core")];
    folder.defaults.auth = Setting::Inherit;
    repo.save_folder("users", &folder).unwrap();
    assert_eq!(
        read(&dir, "users/folder.yml"),
        "info:\n  name: Users\n  type: folder\n  seq: 3\n\nrequest:\n  headers:\n    - name: x-team\n      value: core\n  auth: inherit\n"
    );

    folder.defaults.variables = vec![Variable::text("resource", "users")];
    repo.save_folder("users", &folder).unwrap();
    let loaded = repo.request(&path("users/x.yml"));
    assert!(loaded.is_err());
    repo.save_request(
        &path("users/x.yml"),
        &HttpRequest::new("x", Method::Get, "/"),
    )
    .unwrap();
    assert_eq!(
        repo.request(&path("users/x.yml")).unwrap().folders[0],
        folder
    );
}

#[test]
fn collections_are_created_and_get_an_id_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut collection = Collection::new("API");
    collection.defaults.headers = vec![Header::new("accept", "application/json")];
    let repo = FsCollection::create(dir.path(), &collection).unwrap();
    assert_eq!(repo.collection().unwrap(), collection);
    assert!(FsCollection::create(dir.path(), &collection).is_err());

    let random = CountingRandom::new(123_456_789);
    let id = repo.ensure_id(&random).unwrap();
    assert!(
        id.as_str().starts_with("col_") && id.as_str().len() == 14,
        "{id:?}"
    );
    assert_eq!(
        repo.ensure_id(&random).unwrap(),
        id,
        "the id is created only once"
    );
    assert_eq!(repo.collection().unwrap().id, Some(id));

    let text = read(&dir, "opencollection.yml");
    assert!(
        text.contains("- node_modules\n") && text.contains("- .apifast\n"),
        "{text}"
    );
    assert!(read(&dir, ".apifast/collection.yml").starts_with("format: 1\nid: col_"));
}

#[test]
fn the_id_keeps_an_existing_ignore_list() {
    let (dir, repo) = setup(&[]);
    fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: T\nextensions:\n  bruno:\n    ignore:\n      - drafts # wip\n",
    )
    .unwrap();
    repo.ensure_id(&CountingRandom::new(1)).unwrap();
    let text = read(&dir, "opencollection.yml");
    assert!(
        text.contains("- drafts # wip\n") && text.contains("- .apifast"),
        "{text}"
    );
}

#[test]
fn environments_are_listed_created_and_edited() {
    let (dir, repo) = setup(&[(
        "environments/prod.yml",
        "name: prod\ncolor: \"#f00\"\nvariables:\n  - name: host # main\n    value: https://api.test\n  - name: key\n    secret: true\n",
    )]);
    let mut prod = repo.environments().unwrap().remove(0);
    assert_eq!(prod.variables[1], Variable::secret("key"));

    prod.variables[0].value = Some(VariableValue::text("https://api2.test"));
    prod.variables[1] = Variable::text("key", "public");
    repo.save_environment(&prod).unwrap();
    let text = read(&dir, "environments/prod.yml");
    assert!(text.contains("- name: host # main"), "{text}");
    assert!(!text.contains("secret"), "{text}");
    assert_eq!(repo.environments().unwrap()[0], prod);

    let staging = Environment::new("Staging EU", vec![Variable::secret("token")]);
    repo.save_environment(&staging).unwrap();
    assert_eq!(
        read(&dir, "environments/Staging EU.yml"),
        "name: Staging EU\n\nvariables:\n  - name: token\n    secret: true\n"
    );
    assert_eq!(repo.environments().unwrap().len(), 2);
}

#[test]
fn environment_file_names_do_not_collide() {
    let (dir, repo) = setup(&[("environments/a.yml", "name: other\n")]);
    repo.save_environment(&Environment::new("a", vec![]))
        .unwrap();
    assert!(dir.path().join("environments/a-2.yml").exists());
    assert_eq!(read(&dir, "environments/a.yml"), "name: other\n");
}

#[test]
fn dotenv_overrides_the_system_environment() {
    let (_dir, repo) = setup(&[(".env", "PATH=from-dotenv\nAPI_KEY=\"k\"\n")]);
    let vars = repo.process_env().unwrap();
    assert_eq!(vars["PATH"], "from-dotenv");
    assert_eq!(vars["API_KEY"], "k");
}

#[test]
fn opening_needs_an_opencollection_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        FsCollection::open(dir.path()),
        Err(RepositoryError::NotFound(_))
    ));
    let _ = Path::new("unused");
}

const CHECKED: &str = "info:
  name: Login
  type: http

http:
  method: POST
  url: \"{{host}}/login\"

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: \"200\"
    - expression: res.body.error
      operator: isUndefined
      disabled: true
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.token
        method: jsonq
      variable:
        name: token
        scope: runtime
    - type: future-action
      note: kept as is
";

#[test]
fn assertions_and_captures_are_read_and_edited_in_place() {
    use apifast_domain::{Assertion, Capture, CaptureScope, Operator};
    let (dir, repo) = setup(&[("login.yml", CHECKED)]);
    let mut request = load(&repo, "login.yml");
    assert_eq!(
        request.assertions,
        [
            Assertion::new("res.status", Operator::Eq, Some("200")),
            Assertion {
                enabled: false,
                ..Assertion::new("res.body.error", Operator::IsUndefined, None)
            },
        ]
    );
    assert_eq!(request.captures, [Capture::new("res.body.token", "token")]);

    request.assertions[0].value = Some("201".into());
    request.captures[0].scope = CaptureScope::Environment;
    request
        .captures
        .push(Capture::new("res.body.user.id", "user_id"));
    repo.save_request(&path("login.yml"), &request).unwrap();
    let after = read(&dir, "login.yml");
    let expected = CHECKED
        .replace("value: \"200\"", "value: \"201\"")
        .replace(
            "name: token\n        scope: runtime",
            "name: token\n        scope: environment",
        )
        .replace(
            "    - type: future-action",
            "    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.user.id
        method: jsonq
      variable:
        name: user_id
        scope: runtime
    - type: future-action",
        );
    assert_eq!(after, expected);
    assert!(after.contains("type: future-action\n      note: kept as is"));
    assert_eq!(load(&repo, "login.yml"), request);
}
