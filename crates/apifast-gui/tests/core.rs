//! The core without the interface: commands in, snapshots out, against a
//! collection on disk and a local HTTP server. No external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use apifast_app::{ItemPath, Placement, RequestPath, TreeItem};
use apifast_gui::core::{Command, Core, ResponseState, Snapshot};

/// Answers every request after `delay`, with the request line as the body.
fn server(delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream, delay));
        }
    });
    port
}

fn answer(stream: TcpStream, delay: Duration) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).ok();
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }
    std::thread::sleep(delay);
    let body = first.trim().to_owned();
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn collection(port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: API\n".to_owned()),
        ("users/folder.yml", "info:\n  name: Users\n  type: folder\n  seq: 1\n".to_owned()),
        ("users/get.yml", "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users/1\"\n".to_owned()),
        ("health.yml", "info:\n  name: Health\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/health\"\n".to_owned()),
        ("environments/local.yml", format!("name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:{port}\n")),
        ("environments/staging.yml", "name: staging\nvariables:\n  - name: host\n    value: http://127.0.0.1:1\n".to_owned()),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

/// A core whose responses go to a temporary data directory (kept alive by
/// the returned guard), and a counter of the repaints it asked for.
fn start() -> (Core, Arc<AtomicUsize>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let repaints = Arc::new(AtomicUsize::new(0));
    let counter = repaints.clone();
    let core = Core::start(data.path().to_path_buf(), move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    (core, repaints, data)
}

/// The response of the active tab.
fn response(snapshot: &Snapshot) -> ResponseState {
    snapshot
        .active_tab()
        .map_or(ResponseState::Idle, |tab| tab.response.clone())
}

/// Waits until the snapshot satisfies `done`, failing after 10 seconds.
fn wait(core: &Core, what: &str, done: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let start = Instant::now();
    loop {
        let snapshot = core.snapshot();
        if done(&snapshot) {
            return snapshot;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}: {snapshot:#?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn open(core: &Core, root: &Path) -> Arc<Snapshot> {
    core.send(Command::OpenCollection(root.to_path_buf()));
    wait(core, "the collection", |s| {
        s.collection.is_some() || s.error.is_some()
    })
}

#[test]
fn opens_a_collection_with_its_tree_and_environments() {
    let dir = collection(server(Duration::ZERO));
    let (core, repaints, _data) = start();
    let snapshot = open(&core, dir.path());

    assert_eq!(snapshot.error, None);
    let tree = &snapshot.collection.as_ref().unwrap().tree;
    assert_eq!(tree.collection.name, "API");
    let names: Vec<_> = tree
        .items
        .iter()
        .map(|i| match i {
            TreeItem::Folder { folder, .. } => folder.name.clone(),
            TreeItem::Request { name, .. } => name.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(names, ["Users", "Health"]);
    assert_eq!(snapshot.environments, ["local", "staging"]);
    assert!(
        repaints.load(Ordering::Relaxed) >= 1,
        "the interface was not told to repaint"
    );
}

#[test]
fn a_folder_without_a_collection_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    assert!(snapshot.collection.is_none());
    assert!(
        snapshot
            .error
            .as_ref()
            .unwrap()
            .contains("opencollection.yml"),
        "{:?}",
        snapshot.error
    );
}

#[test]
fn sends_the_open_request_with_the_active_environment() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    assert_eq!(snapshot.active_tab().unwrap().request().name, "Get");

    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(
            response(s),
            ResponseState::Received(_) | ResponseState::Failed(_)
        )
    });
    let ResponseState::Received(received) = &response(&snapshot) else {
        panic!("{:?}", response(&snapshot));
    };
    assert_eq!(received.head.status, 200);
    assert_eq!(received.size, 21);
    assert_eq!(received.body.len(), 21);
    // The body is read from the response store, by windows.
    assert_eq!(
        received.body.read(0, 100).unwrap(),
        b"GET /users/1 HTTP/1.1"
    );
    assert_eq!(received.body.read(4, 6).unwrap(), b"/users");
}

#[test]
fn a_failed_send_is_reported_without_breaking_the_core() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());

    // No environment: `host` is missing.
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "the failure", |s| {
        matches!(response(s), ResponseState::Failed(_))
    });
    assert_eq!(
        response(&snapshot),
        ResponseState::Failed("variable `host` is not defined".into())
    );

    // Nothing listens on port 1.
    core.send(Command::SelectEnvironment(Some("staging".into())));
    core.send(Command::Send);
    let snapshot = wait(
        &core,
        "the connection error",
        |s| matches!(&response(s), ResponseState::Failed(m) if m.contains("cannot connect")),
    );
    assert!(matches!(response(&snapshot), ResponseState::Failed(_)));
}

#[test]
fn a_send_in_progress_can_be_cancelled() {
    let dir = collection(server(Duration::from_secs(5)));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());

    core.send(Command::Send);
    wait(&core, "sending", |s| response(s) == ResponseState::Sending);
    let started = Instant::now();
    core.send(Command::Cancel);
    wait(&core, "the cancellation", |s| {
        response(s) == ResponseState::Cancelled
    });
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_new_tab_starts_without_a_response() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    core.send(Command::Send);
    wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });

    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "the new request", |s| {
        s.active_tab().is_some_and(|t| t.request().name == "Get")
    });
    assert_eq!(response(&snapshot), ResponseState::Idle);
}

#[test]
fn dropping_the_core_stops_its_thread() {
    let (core, _, _data) = start();
    core.send(Command::Cancel);
    let started = Instant::now();
    drop(core);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn responses_are_stored_in_the_given_data_directory() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    core.send(Command::Send);
    wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let folders: Vec<_> = std::fs::read_dir(data.path().join("responses"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(folders.len(), 1);
    assert_eq!(std::fs::read_dir(folders[0].path()).unwrap().count(), 1);
}

/// Waits until a change made after `version` is published.
fn next(core: &Core, version: u64) -> Arc<Snapshot> {
    wait(core, "the change", |s| s.version > version)
}

fn names(items: &[TreeItem]) -> Vec<String> {
    items
        .iter()
        .map(|i| match i {
            TreeItem::Folder { folder, .. } => folder.name.clone(),
            TreeItem::Request { name, .. } => name.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_created_request_appears_in_the_tree_and_opens() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    core.send(Command::CreateRequest {
        folder: String::new(),
        name: "New one".into(),
    });
    let snapshot = next(&core, snapshot.version);
    assert_eq!(snapshot.error, None);
    let tree = &snapshot.collection.as_ref().unwrap().tree;
    assert_eq!(names(&tree.items), ["Users", "Health", "New one"]);
    let open = snapshot.active_tab().unwrap();
    assert_eq!(open.path.as_str(), "new-one.yml");
    assert!(dir.path().join("new-one.yml").exists());
}

#[test]
fn renaming_the_folder_of_the_open_request_keeps_it_open() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Rename {
        item: ItemPath::Folder("users".into()),
        name: "People".into(),
    });
    let snapshot = next(&core, snapshot.version);
    assert_eq!(snapshot.error, None);
    let open = snapshot.active_tab().unwrap();
    assert_eq!(open.path.as_str(), "people/get.yml");
    assert_eq!(open.request().name, "Get");
}

#[test]
fn deleting_the_open_request_closes_it() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Delete(ItemPath::Request(RequestPath::new(
        "health.yml",
    ))));
    let snapshot = next(&core, snapshot.version);
    assert!(snapshot.active_tab().is_none());
    let tree = &snapshot.collection.as_ref().unwrap().tree;
    assert_eq!(names(&tree.items), ["Users"]);
}

#[test]
fn moving_reorders_the_tree_and_errors_are_reported() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    core.send(Command::Move {
        item: ItemPath::Request(RequestPath::new("health.yml")),
        placement: Placement::Before(ItemPath::Folder("users".into())),
    });
    let snapshot = next(&core, snapshot.version);
    let tree = &snapshot.collection.as_ref().unwrap().tree;
    assert_eq!(names(&tree.items), ["Health", "Users"]);

    core.send(Command::CreateFolder {
        parent: "missing".into(),
        name: "X".into(),
    });
    let snapshot = next(&core, snapshot.version);
    assert!(
        snapshot.error.as_ref().unwrap().contains("missing"),
        "{:?}",
        snapshot.error
    );
}

fn tab_names(snapshot: &Snapshot) -> Vec<&str> {
    snapshot
        .tabs
        .iter()
        .map(|tab| tab.request().name.as_str())
        .collect()
}

fn edited(snapshot: &Snapshot, url: &str) -> Box<apifast_domain::HttpRequest> {
    let mut request = snapshot.active_tab().unwrap().request().clone();
    url.clone_into(&mut request.url);
    Box::new(request)
}

#[test]
fn requests_open_in_tabs_and_closing_shows_a_neighbour() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    assert_eq!(tab_names(&snapshot), ["Health", "Get"]);
    assert_eq!(snapshot.active, Some(RequestPath::new("users/get.yml")));

    // Opening an open request shows its tab instead of a second one.
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = next(&core, snapshot.version);
    assert_eq!(snapshot.tabs.len(), 2);
    assert_eq!(snapshot.active, Some(RequestPath::new("health.yml")));

    core.send(Command::CloseTab(RequestPath::new("health.yml")));
    let snapshot = next(&core, snapshot.version);
    assert_eq!(tab_names(&snapshot), ["Get"]);
    assert_eq!(snapshot.active, Some(RequestPath::new("users/get.yml")));
}

#[test]
fn edits_are_a_draft_that_is_sent_and_saved() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let path = RequestPath::new("health.yml");
    let revision = snapshot.active_tab().unwrap().revision;

    core.send(Command::Edit {
        path: path.clone(),
        request: edited(&snapshot, "{{host}}/draft"),
    });
    let snapshot = next(&core, snapshot.version);
    let tab = snapshot.active_tab().unwrap();
    assert!(tab.is_dirty());
    // An edit is not a new revision: the interface keeps its own copy.
    assert_eq!(tab.revision, revision);

    // What is sent is what is on screen, not what is saved.
    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert_eq!(received.body.read(0, 100).unwrap(), b"GET /draft HTTP/1.1");
    let file = dir.path().join("health.yml");
    assert!(!std::fs::read_to_string(&file).unwrap().contains("draft"));

    core.send(Command::Save(path));
    let snapshot = next(&core, snapshot.version);
    let tab = snapshot.active_tab().unwrap();
    assert!(!tab.is_dirty());
    assert_ne!(tab.revision, revision);
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("{{host}}/draft")
    );
}

#[test]
fn editing_back_to_the_saved_contents_is_not_a_draft() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let path = RequestPath::new("health.yml");
    core.send(Command::Edit {
        path: path.clone(),
        request: edited(&snapshot, "x"),
    });
    core.send(Command::Edit {
        path,
        request: edited(&snapshot, "{{host}}/health"),
    });
    let snapshot = wait(&core, "both edits", |s| s.version >= snapshot.version + 2);
    assert!(!snapshot.active_tab().unwrap().is_dirty());
}

#[test]
fn tabs_drafts_and_environment_come_back_after_a_restart() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    core.send(Command::Edit {
        path: RequestPath::new("health.yml"),
        request: edited(&snapshot, "{{host}}/unsaved"),
    });
    core.send(Command::ActivateTab(RequestPath::new("users/get.yml")));
    wait(&core, "the draft", |s| {
        s.tabs.iter().any(apifast_gui::core::Tab::is_dirty)
    });
    // Dropping writes the session at once, without waiting for the delay.
    drop(core);

    let core = Core::start(data.path().to_path_buf(), || {});
    let snapshot = open(&core, dir.path());
    assert_eq!(tab_names(&snapshot), ["Get", "Health"]);
    assert_eq!(snapshot.active, Some(RequestPath::new("users/get.yml")));
    assert_eq!(snapshot.environment.as_deref(), Some("local"));
    let health = &snapshot.tabs[1];
    assert_eq!(health.draft.as_ref().unwrap().url, "{{host}}/unsaved");
    assert_eq!(health.saved.url, "{{host}}/health");
}

#[test]
fn a_renamed_request_keeps_its_tab_and_draft() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        path: RequestPath::new("users/get.yml"),
        request: edited(&snapshot, "{{host}}/edited"),
    });
    core.send(Command::Rename {
        item: ItemPath::Request(RequestPath::new("users/get.yml")),
        name: "Fetch".into(),
    });
    let snapshot = wait(&core, "the rename", |s| {
        s.active == Some(RequestPath::new("users/fetch.yml"))
    });
    let tab = snapshot.active_tab().unwrap();
    assert_eq!(tab.saved.name, "Fetch");
    let draft = tab.draft.as_ref().unwrap();
    // Saving the draft must not undo the rename.
    assert_eq!(
        (draft.name.as_str(), draft.url.as_str()),
        ("Fetch", "{{host}}/edited")
    );
}

#[test]
fn each_tab_knows_its_variables_and_where_they_come_from() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let context = snapshot.active_tab().unwrap().context.clone();
    assert!(context.variables.get("host").is_none());

    core.send(Command::SelectEnvironment(Some("local".into())));
    let snapshot = next(&core, snapshot.version);
    let tab = snapshot.active_tab().unwrap();
    let host = tab.context.variables.get("host").unwrap();
    assert_eq!(
        host.origin,
        apifast_domain::Origin::Environment("local".into())
    );
    assert!(
        host.value
            .as_ref()
            .unwrap()
            .data
            .starts_with("http://127.0.0.1:")
    );
    assert_ne!(tab.context.revision, context.revision);

    // Typing in the URL does not work them out again.
    let revision = tab.context.revision;
    core.send(Command::Edit {
        path: RequestPath::new("health.yml"),
        request: edited(&snapshot, "{{host}}/other"),
    });
    let snapshot = next(&core, snapshot.version);
    assert_eq!(snapshot.active_tab().unwrap().context.revision, revision);
}

#[test]
fn each_tab_knows_what_it_inherits_from_its_folders() {
    let dir = collection(server(Duration::ZERO));
    std::fs::write(
        dir.path().join("users/folder.yml"),
        "info:\n  name: Users\n  type: folder\n  seq: 1\nrequest:\n  headers:\n    - name: x-team\n      value: core\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let context = &snapshot.active_tab().unwrap().context;
    assert_eq!(context.folders, ["Users"]);
    let header = &context.inherited_headers[0];
    assert_eq!(
        (header.value.name.as_str(), &header.origin),
        ("x-team", &apifast_domain::Origin::Folder(0))
    );
    assert!(matches!(
        context.inherited_auth.value,
        apifast_domain::AuthScheme::Bearer(_)
    ));
    assert_eq!(
        context.inherited_settings.timeout_ms.origin,
        apifast_domain::Origin::Default
    );
}

fn received(snapshot: &Snapshot) -> apifast_gui::core::Received {
    match response(snapshot) {
        ResponseState::Received(received) => received,
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_send_goes_to_the_history_and_its_file() {
    use apifast_app::HistoryOutcome;

    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    // Fails: no environment, so `host` is missing.
    core.send(Command::Send);
    wait(&core, "one entry", |s| s.history.len() == 1);
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::Send);
    let snapshot = wait(&core, "two entries", |s| s.history.len() == 2);

    let [failed, sent] = snapshot.history.as_slice() else {
        unreachable!()
    };
    assert_eq!(failed.request, RequestPath::new("health.yml"));
    assert_eq!(
        failed.outcome,
        HistoryOutcome::Failed("variable `host` is not defined".into())
    );
    // The URL as written, never resolved.
    assert_eq!(sent.url, "{{host}}/health");
    assert!(matches!(&sent.outcome, HistoryOutcome::Received { head, .. } if head.status == 200));

    let files: Vec<_> = std::fs::read_dir(data.path().join("history"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn the_last_response_of_each_tab_comes_back_after_a_restart() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    drop(core);

    let core = Core::start(data.path().to_path_buf(), || {});
    open(&core, dir.path());
    let snapshot = wait(&core, "the restored response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let received = received(&snapshot);
    assert_eq!(received.head.status, 200);
    assert_eq!(
        received.body.read(0, 100).unwrap(),
        b"GET /users/1 HTTP/1.1"
    );
    assert_eq!(snapshot.history.len(), 1);
}

#[test]
fn a_history_entry_opens_its_request_with_that_response() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    for path in ["users/get.yml", "health.yml"] {
        core.send(Command::OpenRequest(RequestPath::new(path)));
        core.send(Command::Send);
    }
    wait(&core, "two entries", |s| s.history.len() == 2);
    core.send(Command::CloseTab(RequestPath::new("users/get.yml")));
    wait(&core, "one tab", |s| s.tabs.len() == 1);

    let index = core
        .snapshot()
        .history
        .iter()
        .position(|e| e.request == RequestPath::new("users/get.yml"))
        .unwrap();
    core.send(Command::OpenHistory(index));
    let snapshot = wait(&core, "the old response", |s| {
        s.active == Some(RequestPath::new("users/get.yml"))
            && matches!(response(s), ResponseState::Received(_))
    });
    assert_eq!(
        received(&snapshot).body.read(0, 100).unwrap(),
        b"GET /users/1 HTTP/1.1"
    );
}

#[test]
fn old_history_is_pruned_when_the_collection_opens() {
    use apifast_app::{HistoryEntry, HistoryStore};

    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    core.send(Command::Send);
    wait(&core, "one entry", |s| s.history.len() == 1);
    drop(core);

    // An entry from a year ago, before the one just sent.
    let file = std::fs::read_dir(data.path().join("history"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let store = apifast_history::FsHistoryStore::new(&file);
    let mut entries = store.entries().unwrap();
    let year_ago = entries[0].at_ms - 365 * 24 * 3600 * 1000;
    entries.insert(
        0,
        HistoryEntry::failed(
            year_ago,
            RequestPath::new("health.yml"),
            ("GET".into(), "old".into()),
            "old".into(),
        ),
    );
    std::fs::remove_file(&file).unwrap();
    for entry in &entries {
        store.append(entry).unwrap();
    }

    let core = Core::start(data.path().to_path_buf(), || {});
    open(&core, dir.path());
    let snapshot = wait(&core, "the history", |s| !s.history.is_empty());
    assert_eq!(snapshot.history.len(), 1);
    assert_ne!(snapshot.history[0].url, "old");
    assert_eq!(store.entries().unwrap().len(), 1, "the file was rewritten");
}

fn host(snapshot: &Snapshot) -> Option<String> {
    let tab = snapshot.active_tab()?;
    let variable = tab.context.variables.get("host")?;
    Some(variable.value.as_ref()?.data.clone())
}

#[test]
fn environments_are_created_renamed_saved_and_deleted() {
    use apifast_domain::Variable;

    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());

    core.send(Command::CreateEnvironment("qa".into()));
    let snapshot = wait(&core, "qa", |s| s.environments.contains(&"qa".to_owned()));
    assert!(dir.path().join("environments/qa.yml").exists());
    assert_eq!(snapshot.environment_files.len(), 3);

    // Renaming the active environment keeps it active, by its new name.
    core.send(Command::RenameEnvironment {
        from: "local".into(),
        to: "dev".into(),
    });
    let snapshot = wait(&core, "dev", |s| s.environments.contains(&"dev".to_owned()));
    assert_eq!(snapshot.environment.as_deref(), Some("dev"));
    assert!(dir.path().join("environments/dev.yml").exists());
    assert!(!dir.path().join("environments/local.yml").exists());

    // Saving a new value reaches what the open request sees.
    let mut dev = snapshot
        .environment_files
        .iter()
        .find(|e| e.name == "dev")
        .unwrap()
        .clone();
    dev.variables = vec![Variable::text("host", "http://changed")];
    core.send(Command::SaveEnvironment(Box::new(dev)));
    let snapshot = wait(&core, "the new host", |s| {
        host(s).as_deref() == Some("http://changed")
    });
    assert!(snapshot.error.is_none());

    // A duplicate name is reported, and nothing changes.
    core.send(Command::CreateEnvironment("dev".into()));
    let snapshot = wait(&core, "the error", |s| s.error.is_some());
    assert!(snapshot.error.as_ref().unwrap().contains("already exists"));
    assert_eq!(snapshot.environment.as_deref(), Some("dev"));

    // Deleting the active environment leaves none active.
    core.send(Command::DeleteEnvironment("dev".into()));
    let snapshot = wait(&core, "dev gone", |s| {
        !s.environments.contains(&"dev".to_owned())
    });
    assert_eq!(snapshot.environment, None);
    assert_eq!(host(&snapshot), None);
}

fn request_file(name: &str, url: &str) -> String {
    format!("info:\n  name: {name}\n  type: http\nhttp:\n  method: GET\n  url: \"{url}\"\n")
}

fn active_tab(snapshot: &Snapshot) -> &apifast_gui::core::Tab {
    snapshot.active_tab().unwrap()
}

#[test]
fn a_file_changed_outside_reaches_its_clean_tab_and_the_tree() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let before = wait(&core, "the tab", |s| s.active_tab().is_some());
    let revision = active_tab(&before).revision;

    std::fs::write(
        dir.path().join("health.yml"),
        request_file("Health", "{{host}}/ping"),
    )
    .unwrap();
    let snapshot = wait(&core, "the new URL", |s| {
        s.active_tab()
            .is_some_and(|t| t.saved.url == "{{host}}/ping")
    });
    let tab = active_tab(&snapshot);
    assert!(tab.revision > revision, "the editor drops its copy");
    assert!(!tab.conflict);

    std::fs::write(dir.path().join("new.yml"), request_file("New", "x")).unwrap();
    wait(&core, "the new request in the tree", |s| {
        names(&s.collection.as_ref().unwrap().tree.items).contains(&"New".to_owned())
    });
}

#[test]
fn saving_is_not_taken_for_a_change_from_outside() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        path: RequestPath::new("health.yml"),
        request: edited(&snapshot, "{{host}}/saved"),
    });
    core.send(Command::Save(RequestPath::new("health.yml")));
    wait(&core, "saved", |s| !active_tab(s).is_dirty());
    // Longer than the watcher's quiet time.
    std::thread::sleep(Duration::from_millis(600));
    let snapshot = core.snapshot();
    let tab = active_tab(&snapshot);
    assert!(!tab.conflict);
    assert_eq!(tab.saved.url, "{{host}}/saved");
}

#[test]
fn a_draft_and_a_change_from_outside_are_a_conflict_to_settle() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    let path = RequestPath::new("health.yml");
    core.send(Command::OpenRequest(path.clone()));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        path: path.clone(),
        request: edited(&snapshot, "{{host}}/mine"),
    });
    wait(&core, "the draft", |s| active_tab(s).is_dirty());

    let theirs = |url: &str| {
        std::fs::write(dir.path().join("health.yml"), request_file("Health", url)).unwrap();
    };
    theirs("{{host}}/theirs");
    let snapshot = wait(&core, "the conflict", |s| active_tab(s).conflict);
    let tab = active_tab(&snapshot);
    assert_eq!(
        tab.request().url,
        "{{host}}/mine",
        "the draft stays on screen"
    );
    assert_eq!(tab.saved.url, "{{host}}/theirs");

    core.send(Command::ResolveConflict {
        path: path.clone(),
        keep_draft: true,
    });
    let snapshot = wait(&core, "kept", |s| !active_tab(s).conflict);
    assert_eq!(active_tab(&snapshot).request().url, "{{host}}/mine");

    theirs("{{host}}/again");
    wait(&core, "another conflict", |s| active_tab(s).conflict);
    core.send(Command::ResolveConflict {
        path,
        keep_draft: false,
    });
    let snapshot = wait(&core, "taken from disk", |s| !active_tab(s).conflict);
    let tab = active_tab(&snapshot);
    assert!(!tab.is_dirty());
    assert_eq!(tab.request().url, "{{host}}/again");
}

#[test]
fn a_file_removed_outside_closes_its_tab_unless_it_has_a_draft() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("users/get.yml")));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    core.send(Command::Edit {
        path: RequestPath::new("health.yml"),
        request: edited(&snapshot, "{{host}}/kept"),
    });
    wait(&core, "the draft", |s| {
        s.tabs.iter().any(apifast_gui::core::Tab::is_dirty)
    });

    std::fs::remove_file(dir.path().join("users/get.yml")).unwrap();
    std::fs::remove_file(dir.path().join("health.yml")).unwrap();
    let snapshot = wait(&core, "the clean tab closed", |s| s.tabs.len() == 1);
    assert_eq!(tab_names(&snapshot), ["Health"]);
}

#[test]
fn an_environment_changed_outside_reaches_the_variables() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("staging".into())));
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    wait(&core, "the tab", |s| s.active_tab().is_some());
    std::fs::write(
        dir.path().join("environments/staging.yml"),
        "name: staging\nvariables:\n  - name: host\n    value: http://elsewhere\n",
    )
    .unwrap();
    wait(&core, "the new host", |s| {
        host(s).as_deref() == Some("http://elsewhere")
    });
}

#[test]
fn the_history_limits_come_from_the_settings() {
    use apifast_app::HistoryLimits;
    use apifast_history::Limits;

    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(RequestPath::new("health.yml")));
    for count in 1..=3 {
        core.send(Command::Send);
        wait(&core, "the entry", |s| s.history.len() == count);
    }
    drop(core);

    let core = Core::start(data.path().to_path_buf(), || {});
    core.send(Command::Configure {
        history: HistoryLimits {
            max_entries: Some(1),
            max_age: None,
        },
        responses: Limits::default(),
    });
    open(&core, dir.path());
    let snapshot = wait(&core, "the history", |s| !s.history.is_empty());
    assert_eq!(snapshot.history.len(), 1);
}
