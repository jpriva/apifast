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
use apifast_gui::core::{Command, Core, ResponseState, Snapshot, TabKey};
use apifast_vault::SystemKeyring;

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
    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), move || {
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

/// The tab of `path` in the focused collection.
fn at(core: &Core, path: impl std::fmt::Display) -> TabKey {
    TabKey::new(root(core), RequestPath::new(path.to_string()))
}

/// The folder of the focused collection.
fn root(core: &Core) -> std::path::PathBuf {
    core.snapshot()
        .collection
        .as_ref()
        .expect("a collection is open")
        .root
        .clone()
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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

/// Answers every request with an event stream: two events, a third later,
/// and then nothing, without closing.
fn event_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                     data: one\n\nevent: two\ndata: 2\n\n"
                );
                std::thread::sleep(Duration::from_millis(200));
                let _ = write!(stream, "data: three\n\n");
                std::thread::sleep(Duration::from_secs(30));
            });
        }
    });
    port
}

#[test]
fn an_event_stream_shows_live_and_stopping_keeps_it() {
    let dir = collection(event_server());
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());

    core.send(Command::Send);
    let snapshot = wait(&core, "three events", |s| match response(s) {
        ResponseState::Streaming(stream) => stream.read(|live| live.events.len() == 3),
        _ => false,
    });
    let ResponseState::Streaming(stream) = response(&snapshot) else {
        unreachable!()
    };
    let names: Vec<String> =
        stream.read(|live| live.events.iter().map(|e| e.event.clone()).collect());
    assert_eq!(names, ["message", "two", "message"]);
    assert_eq!(
        stream.read(|live| live.head.as_ref().map(|h| h.status)),
        Some(200)
    );

    core.send(Command::Cancel);
    let snapshot = wait(&core, "the stream kept", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert_eq!(received.head.status, 200);
    let body = received.body.read(0, 1024).unwrap();
    assert_eq!(apifast_domain::parse_events(&body).len(), 3);
}

/// A WebSocket server that answers each text message in capitals.
fn websocket_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        while let Ok(message) = socket.read() {
            if let tungstenite::Message::Text(text) = message {
                let _ = socket.send(tungstenite::Message::text(text.to_uppercase()));
            }
        }
    });
    port
}

#[test]
fn a_websocket_request_connects_sends_the_message_picked_and_disconnects() {
    use apifast_domain::{WsData, WsEvent, WsSide};

    let dir = collection(server(Duration::ZERO));
    let port = websocket_server();
    std::fs::write(
        dir.path().join("live.yml"),
        format!(
            "info:\n  name: Live\n  type: websocket\nwebsocket:\n  url: ws://127.0.0.1:{port}/live\n  message:\n    - title: greet\n      message:\n        type: text\n        data: hello {{{{name}}}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("environments/local.yml"),
        "name: local\nvariables:\n  - name: name\n    value: Ana\n",
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "live.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());

    core.send(Command::Send);
    wait(&core, "the connection", |s| match response(s) {
        ResponseState::Streaming(stream) => stream.read(|live| live.socket),
        _ => false,
    });
    core.send(Command::SendMessage(0));
    // A message that is not there is not sent.
    core.send(Command::SendMessage(7));
    let snapshot = wait(&core, "the answer", |s| match response(s) {
        ResponseState::Streaming(stream) => stream.read(|live| live.messages.len() == 2),
        _ => false,
    });
    let ResponseState::Streaming(stream) = response(&snapshot) else {
        unreachable!()
    };
    let events: Vec<WsEvent> =
        stream.read(|live| live.messages.iter().map(|m| m.event.clone()).collect());
    assert_eq!(
        events,
        [
            WsEvent::Sent(WsData::Text("hello Ana".into())),
            WsEvent::Received(WsData::Text("HELLO ANA".into())),
        ]
    );

    core.send(Command::Cancel);
    let snapshot = wait(&core, "the connection kept", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert_eq!(received.head.status, 101);
    let log = received.body.read(0, 4096).unwrap();
    let last = apifast_domain::parse_ws_log(&log).pop().unwrap();
    assert_eq!(
        last.event,
        WsEvent::Closed {
            by: WsSide::Client,
            code: Some(1000),
            reason: String::new(),
        }
    );
    let entries = snapshot.history.as_ref();
    assert!(
        entries.iter().any(|e| e.request.as_str() == "live.yml"),
        "{entries:?}"
    );
}

#[test]
fn a_new_tab_starts_without_a_response() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    core.send(Command::Send);
    wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });

    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
        root: root(&core),
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Rename {
        root: root(&core),
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Delete {
        root: root(&core),
        item: ItemPath::Request(RequestPath::new("health.yml")),
    });
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
        root: root(&core),
        item: ItemPath::Request(RequestPath::new("health.yml")),
        placement: Placement::Before(ItemPath::Folder("users".into())),
    });
    let snapshot = next(&core, snapshot.version);
    let tree = &snapshot.collection.as_ref().unwrap().tree;
    assert_eq!(names(&tree.items), ["Health", "Users"]);

    core.send(Command::CreateFolder {
        root: root(&core),
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    assert_eq!(tab_names(&snapshot), ["Health", "Get"]);
    assert_eq!(
        snapshot.active.as_ref().map(|k| k.path.clone()),
        Some(RequestPath::new("users/get.yml"))
    );

    // Opening an open request shows its tab instead of a second one.
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = next(&core, snapshot.version);
    assert_eq!(snapshot.tabs.len(), 2);
    assert_eq!(
        snapshot.active.as_ref().map(|k| k.path.clone()),
        Some(RequestPath::new("health.yml"))
    );

    core.send(Command::CloseTab(at(&core, "health.yml")));
    let snapshot = next(&core, snapshot.version);
    assert_eq!(tab_names(&snapshot), ["Get"]);
    assert_eq!(
        snapshot.active.as_ref().map(|k| k.path.clone()),
        Some(RequestPath::new("users/get.yml"))
    );
}

#[test]
fn edits_are_a_draft_that_is_sent_and_saved() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let path = RequestPath::new("health.yml");
    let revision = snapshot.active_tab().unwrap().revision;

    core.send(Command::Edit {
        tab: at(&core, &path),
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

    core.send(Command::Save(at(&core, &path)));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let path = RequestPath::new("health.yml");
    core.send(Command::Edit {
        tab: at(&core, &path),
        request: edited(&snapshot, "x"),
    });
    core.send(Command::Edit {
        tab: at(&core, &path),
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    core.send(Command::Edit {
        tab: at(&core, "health.yml"),
        request: edited(&snapshot, "{{host}}/unsaved"),
    });
    core.send(Command::ActivateTab(at(&core, "users/get.yml")));
    wait(&core, "the draft", |s| {
        s.tabs.iter().any(apifast_gui::core::Tab::is_dirty)
    });
    // Dropping writes the session at once, without waiting for the delay.
    drop(core);

    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    let snapshot = open(&core, dir.path());
    assert_eq!(tab_names(&snapshot), ["Get", "Health"]);
    assert_eq!(
        snapshot.active.as_ref().map(|k| k.path.clone()),
        Some(RequestPath::new("users/get.yml"))
    );
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        tab: at(&core, "users/get.yml"),
        request: edited(&snapshot, "{{host}}/edited"),
    });
    core.send(Command::Rename {
        root: root(&core),
        item: ItemPath::Request(RequestPath::new("users/get.yml")),
        name: "Fetch".into(),
    });
    let snapshot = wait(&core, "the rename", |s| {
        s.active.as_ref().map(|k| &k.path) == Some(&RequestPath::new("users/fetch.yml"))
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
        tab: at(&core, "health.yml"),
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    drop(core);

    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
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
        core.send(Command::OpenRequest(at(&core, path)));
        core.send(Command::Send);
    }
    wait(&core, "two entries", |s| s.history.len() == 2);
    core.send(Command::CloseTab(at(&core, "users/get.yml")));
    wait(&core, "one tab", |s| s.tabs.len() == 1);

    let index = core
        .snapshot()
        .history
        .iter()
        .position(|e| e.request == RequestPath::new("users/get.yml"))
        .unwrap();
    core.send(Command::OpenHistory(index));
    let snapshot = wait(&core, "the old response", |s| {
        s.active.as_ref().map(|k| &k.path) == Some(&RequestPath::new("users/get.yml"))
            && matches!(response(s), ResponseState::Received(_))
    });
    assert_eq!(
        received(&snapshot).body.read(0, 100).unwrap(),
        b"GET /users/1 HTTP/1.1"
    );
}

#[test]
fn history_entries_keep_what_was_sent_and_are_renamed_removed_and_cleared() {
    use apifast_app::HistoryOutcome;
    use apifast_gui::core::HistoryCommand;

    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    core.send(Command::Send);
    wait(&core, "one entry", |s| s.history.len() == 1);
    core.send(Command::Send);
    let snapshot = wait(&core, "two entries", |s| s.history.len() == 2);

    // What was sent: resolved, in the entry and in the response shown.
    let sent = snapshot.history[0].sent.clone().unwrap();
    assert_eq!(sent.method, "GET");
    assert!(sent.url.ends_with("/users/1"), "{}", sent.url);
    assert!(!sent.url.contains("{{"), "resolved: {}", sent.url);
    let snapshot = wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    assert_eq!(received(&snapshot).sent.as_deref(), Some(&sent));

    let (first, second) = (
        snapshot.history[0].id.clone(),
        snapshot.history[1].id.clone(),
    );
    core.send(Command::History(HistoryCommand::Rename {
        id: first.clone(),
        name: "Before the fix".into(),
    }));
    let snapshot = wait(&core, "the name", |s| s.history[0].name.is_some());
    assert_eq!(snapshot.history[0].name.as_deref(), Some("Before the fix"));

    let HistoryOutcome::Received { body, .. } = &snapshot.history[1].outcome else {
        unreachable!()
    };
    let body_file = std::fs::read_dir(data.path().join("responses"))
        .unwrap()
        .flatten()
        .flat_map(|dir| std::fs::read_dir(dir.path()).unwrap().flatten())
        .find(|file| file.file_name().to_string_lossy().starts_with(&body.0))
        .unwrap()
        .path();
    core.send(Command::History(HistoryCommand::Remove(second)));
    let snapshot = wait(&core, "one entry", |s| s.history.len() == 1);
    assert_eq!(snapshot.history[0].id, first);
    assert!(!body_file.exists(), "its body goes with it");

    // The file has it too: a restart reads the name back.
    drop(core);
    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    open(&core, dir.path());
    let snapshot = wait(&core, "the history", |s| !s.history.is_empty());
    assert_eq!(snapshot.history[0].name.as_deref(), Some("Before the fix"));
    assert!(snapshot.history[0].sent.is_some());

    core.send(Command::History(HistoryCommand::Clear));
    wait(&core, "no entries", |s| s.history.is_empty());
}

#[test]
fn old_history_is_pruned_when_the_collection_opens() {
    use apifast_app::{HistoryEntry, HistoryStore};

    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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

    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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

    // In the interface's language, once the core is told it.
    core.send(Command::Language("es"));
    core.send(Command::CreateEnvironment("dev".into()));
    let snapshot = wait(&core, "the error in Spanish", |s| {
        s.error.as_deref() == Some("ya existe un entorno llamado `dev`")
    });
    assert_eq!(snapshot.environment.as_deref(), Some("dev"));
    core.send(Command::Language("en"));

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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        tab: at(&core, "health.yml"),
        request: edited(&snapshot, "{{host}}/saved"),
    });
    core.send(Command::Save(at(&core, "health.yml")));
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
    core.send(Command::OpenRequest(at(&core, &path)));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        tab: at(&core, &path),
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
        tab: at(&core, &path),
        keep_draft: true,
    });
    let snapshot = wait(&core, "kept", |s| !active_tab(s).conflict);
    assert_eq!(active_tab(&snapshot).request().url, "{{host}}/mine");

    theirs("{{host}}/again");
    wait(&core, "another conflict", |s| active_tab(s).conflict);
    core.send(Command::ResolveConflict {
        tab: at(&core, &path),
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
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "two tabs", |s| s.tabs.len() == 2);
    core.send(Command::Edit {
        tab: at(&core, "health.yml"),
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
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
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    for count in 1..=3 {
        core.send(Command::Send);
        wait(&core, "the entry", |s| s.history.len() == count);
    }
    drop(core);

    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    core.send(Command::Configure {
        history: HistoryLimits {
            max_entries: Some(1),
            max_age: None,
        },
        responses: Limits::default(),
        proxy: apifast_domain::ProxyChoice::System,
        trust: apifast_domain::TlsTrust::default(),
        cookies: (true, true),
        timeout_ms: 30_000,
        reopen_last: true,
    });
    open(&core, dir.path());
    let snapshot = wait(&core, "the history", |s| !s.history.is_empty());
    assert_eq!(snapshot.history.len(), 1);
}

/// The `local` environment with `token` as a secret.
fn with_secret_token(snapshot: &Snapshot) -> apifast_domain::Environment {
    let mut local = snapshot
        .environment_files
        .iter()
        .find(|e| e.name == "local")
        .unwrap()
        .clone();
    local
        .variables
        .push(apifast_domain::Variable::secret("token"));
    local
}

#[test]
fn a_first_secret_gives_the_collection_an_id_and_its_value_reaches_the_send() {
    use apifast_app::HistoryOutcome;
    use apifast_gui::core::{SecretStatus, VaultCommand};

    let dir = collection(server(Duration::ZERO));
    std::fs::write(
        dir.path().join("secret.yml"),
        "info:\n  name: Secret\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/s/{{token}}\"\n",
    )
    .unwrap();
    let (core, _, data) = start();
    let snapshot = open(&core, dir.path());
    assert!(
        snapshot
            .collection
            .as_ref()
            .unwrap()
            .tree
            .collection
            .id
            .is_none()
    );
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "secret.yml")));
    core.send(Command::SaveEnvironment(Box::new(with_secret_token(
        &snapshot,
    ))));
    let snapshot = wait(&core, "the secret variable", |s| {
        !s.vault.secrets.is_empty()
    });
    assert_eq!(
        snapshot.vault.status("local", "token"),
        SecretStatus::Missing
    );

    core.send(Command::Vault(VaultCommand::Set {
        environment: "local".into(),
        name: "token".into(),
        value: apifast_domain::Secret::new("t0k3n"),
        shared: false,
    }));
    let snapshot = wait(&core, "the stored value", |s| {
        s.vault.status("local", "token") == SecretStatus::Local
    });
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
    assert!(
        dir.path().join(".apifast/collection.yml").exists(),
        "it got an id"
    );
    assert!(snapshot.vault.master_in_file, "no keyring in tests");
    // The open tabs and the environment came along.
    assert_eq!(snapshot.environment.as_deref(), Some("local"));
    assert!(snapshot.active_tab().is_some());

    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(
            response(s),
            ResponseState::Received(_) | ResponseState::Failed(_)
        )
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        panic!("{:?}", response(&snapshot));
    };
    assert_eq!(
        received.body.read(0, 100).unwrap(),
        b"GET /s/t0k3n HTTP/1.1"
    );
    let snapshot = wait(&core, "the history", |s| !s.history.is_empty());
    let HistoryOutcome::Received { head, .. } = &snapshot.history[0].outcome else {
        unreachable!()
    };
    assert!(!head.url.contains("t0k3n"), "{}", head.url);
    let file = std::fs::read_dir(data.path().join("history"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let text = std::fs::read_to_string(file.path()).unwrap();
    assert!(!text.contains("t0k3n"), "never stored in the clear");
}

#[test]
fn values_are_revealed_on_request_and_keys_shown_and_rotated() {
    use apifast_gui::core::{SecretStatus, VaultCommand};

    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    core.send(Command::SaveEnvironment(Box::new(with_secret_token(
        &snapshot,
    ))));
    wait(&core, "the secret variable", |s| {
        !s.vault.secrets.is_empty()
    });
    core.send(Command::Vault(VaultCommand::Set {
        environment: "local".into(),
        name: "token".into(),
        value: apifast_domain::Secret::new("t0k3n"),
        shared: true,
    }));
    let snapshot = wait(&core, "shared", |s| {
        s.vault.status("local", "token") == SecretStatus::Shared
    });
    assert!(snapshot.vault.shares && snapshot.vault.has_key);
    assert!(dir.path().join(".apifast/secrets/local.yml").exists());

    core.send(Command::Vault(VaultCommand::Reveal {
        environment: "local".into(),
        name: "token".into(),
    }));
    let snapshot = wait(&core, "the value", |s| s.vault.revealed.is_some());
    assert_eq!(
        snapshot.vault.revealed.as_ref().unwrap().value.expose(),
        "t0k3n"
    );
    // Never printed, even in a debug dump of the snapshot.
    assert!(!format!("{snapshot:?}").contains("t0k3n"));
    core.send(Command::Vault(VaultCommand::Hide));
    wait(&core, "hidden", |s| s.vault.revealed.is_none());

    core.send(Command::Vault(VaultCommand::ShowKey));
    let snapshot = wait(&core, "the key", |s| s.vault.shareable_key.is_some());
    let key = snapshot.vault.shareable_key.clone().unwrap();
    assert!(key.expose().starts_with("AFK-"));
    core.send(Command::Vault(VaultCommand::RotateKey));
    let snapshot = wait(&core, "a new key", |s| {
        s.vault.shareable_key.as_ref().is_some_and(|k| k != &key)
    });
    assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
}

#[test]
fn a_value_no_secret_uses_is_an_orphan_and_can_be_forgotten() {
    use apifast_gui::core::VaultCommand;

    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    core.send(Command::SaveEnvironment(Box::new(with_secret_token(
        &snapshot,
    ))));
    wait(&core, "the secret variable", |s| {
        !s.vault.secrets.is_empty()
    });
    core.send(Command::Vault(VaultCommand::Set {
        environment: "local".into(),
        name: "token".into(),
        value: apifast_domain::Secret::new("t0k3n"),
        shared: false,
    }));
    let snapshot = wait(&core, "stored", |s| {
        s.vault.orphans.is_empty() && !s.vault.secrets.is_empty()
    });
    // The variable stops being secret, as Bruno might leave it.
    let mut local = with_secret_token(&snapshot);
    local.variables.retain(|v| v.name != "token");
    core.send(Command::SaveEnvironment(Box::new(local)));
    let snapshot = wait(&core, "the orphan", |s| !s.vault.orphans.is_empty());
    let orphan = snapshot.vault.orphans[0].clone();
    assert_eq!(
        (orphan.environment.as_str(), orphan.name.as_str()),
        ("local", "token")
    );
    core.send(Command::Vault(VaultCommand::Forget(orphan)));
    wait(&core, "forgotten", |s| s.vault.orphans.is_empty());
}

#[test]
fn a_capture_feeds_the_next_send_and_checks_reach_the_tab() {
    let dir = collection(server(Duration::ZERO));
    std::fs::write(
        dir.path().join("first.yml"),
        "info:\n  name: First\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/first\"\nruntime:\n  actions:\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.status\n        method: jsonq\n      variable:\n        name: code\n        scope: runtime\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("second.yml"),
        "info:\n  name: Second\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/code/{{code}}\"\nruntime:\n  assertions:\n    - expression: res.body\n      operator: contains\n      value: /code/200\n    - expression: res.status\n      operator: eq\n      value: \"201\"\n",
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "first.yml")));
    wait(&core, "the first tab", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "the first response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let checks = received(&snapshot).checks;
    assert_eq!(checks.captures[0].variable, "code");

    core.send(Command::OpenRequest(at(&core, "second.yml")));
    wait(&core, "the second tab", |s| {
        s.active_tab()
            .is_some_and(|t| t.path == RequestPath::new("second.yml"))
    });
    core.send(Command::Send);
    let snapshot = wait(&core, "the second response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let checks = received(&snapshot).checks;
    let passed: Vec<bool> = checks.assertions.iter().map(|a| a.verdict.passed).collect();
    assert_eq!(passed, [true, false], "{checks:?}");
}

#[test]
fn a_request_uses_another_ones_response_which_shows_in_the_history() {
    let dir = collection(server(Duration::ZERO));
    std::fs::write(
        dir.path().join("chained.yml"),
        "info:\n  name: Chained\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/len/{{ header(request='health', name='content-length') }}\"\n",
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "chained.yml")));
    wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "both entries", |s| s.history.len() == 2);
    let requests: Vec<&str> = snapshot
        .history
        .iter()
        .map(|e| e.request.as_str())
        .collect();
    assert_eq!(requests, ["health.yml", "chained.yml"]);
    // The server answers with the request line: `GET /health HTTP/1.1`.
    let snapshot = wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let body = received(&snapshot).body.read(0, 100).unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), "GET /len/20 HTTP/1.1");
}

#[test]
fn a_collection_is_created_and_opened_but_never_over_another() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("new/billing");
    let (core, _, _data) = start();
    core.send(Command::CreateCollection {
        folder: folder.clone(),
        name: "Billing".into(),
    });
    let snapshot = wait(&core, "the new collection", |s| s.collection.is_some());
    let collection = snapshot.collection.as_ref().unwrap();
    assert_eq!(collection.tree.collection.name, "Billing");
    assert!(folder.join("opencollection.yml").is_file());

    // A folder that already holds one is left alone.
    let before = std::fs::read_to_string(folder.join("opencollection.yml")).unwrap();
    core.send(Command::CreateCollection {
        folder: folder.clone(),
        name: "Other".into(),
    });
    let snapshot = wait(&core, "the refusal", |s| s.error.is_some());
    assert!(
        snapshot
            .error
            .as_deref()
            .unwrap()
            .contains("opencollection.yml"),
        "{:?}",
        snapshot.error
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("opencollection.yml")).unwrap(),
        before
    );
}

#[test]
fn a_folder_without_a_collection_is_remembered_to_offer_one() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _, data) = start();
    assert!(core.snapshot().default_parent.starts_with(data.path()));
    core.send(Command::OpenCollection(dir.path().to_path_buf()));
    let snapshot = wait(&core, "the error", |s| s.error.is_some());
    // The folder as the core keeps it: resolved (`/private/var` on macOS,
    // `\\?\C:\…` on Windows).
    let resolved = dir.path().canonicalize().unwrap();
    assert_eq!(
        snapshot.missing_collection.as_deref(),
        Some(resolved.as_path())
    );
    // A folder that does not exist is not offered.
    core.send(Command::OpenCollection(dir.path().join("nope")));
    let snapshot = wait(&core, "the second error", |s| {
        s.missing_collection.is_none()
    });
    assert!(snapshot.error.is_some());
}

// --- Workspaces and several collections --------------------------------------

/// A core on the data directory `data`, as after a restart.
fn core_in(data: &Path) -> Core {
    Core::start_with(data.to_path_buf(), SystemKeyring::off(), || {})
}

/// Opens the collection in `dir` and waits until it is the focused one.
fn open_root(core: &Core, dir: &Path) -> std::path::PathBuf {
    let root = dir.canonicalize().unwrap();
    core.send(Command::OpenCollection(dir.to_path_buf()));
    wait(core, "the collection focused", |s| {
        s.collection.as_ref().is_some_and(|c| c.root == root)
    });
    root
}

/// A second collection, named `name`, on the same server.
fn named(port: u16, name: &str) -> tempfile::TempDir {
    let dir = collection(port);
    std::fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\ninfo:\n  name: {name}\n"),
    )
    .unwrap();
    dir
}

#[test]
fn two_collections_are_open_with_their_own_tabs_and_environments() {
    let port = server(Duration::ZERO);
    let (shop, billing) = (named(port, "Shop"), named(port, "Billing"));
    let (core, _, data) = start();
    let shop_root = open_root(&core, shop.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(TabKey::new(
        &shop_root,
        RequestPath::new("health.yml"),
    )));
    let billing_root = open_root(&core, billing.path());
    core.send(Command::SelectEnvironment(Some("staging".into())));
    let billing_tab = TabKey::new(&billing_root, RequestPath::new("health.yml"));
    core.send(Command::OpenRequest(billing_tab.clone()));
    let snapshot = wait(&core, "both tabs", |s| s.tabs.len() == 2);

    // Same request path, two tabs: one per collection.
    let names: Vec<&str> = snapshot
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Shop", "Billing"]);
    assert!(
        snapshot
            .collections
            .iter()
            .all(|c| c.tree.is_some() && c.expanded)
    );
    assert_eq!(snapshot.active.as_ref(), Some(&billing_tab));
    assert_eq!(snapshot.environment.as_deref(), Some("staging"));

    // Each tab sends with its own collection's environment.
    let shop_tab = TabKey::new(&shop_root, RequestPath::new("health.yml"));
    core.send(Command::ActivateTab(shop_tab.clone()));
    let snapshot = wait(&core, "the shop tab", |s| {
        s.active.as_ref() == Some(&shop_tab)
    });
    assert_eq!(snapshot.environment.as_deref(), Some("local"));
    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    assert_eq!(snapshot.history.len(), 1, "the shop's history only");

    // Both are in the default workspace's file.
    let workspace = data.path().join("default-workspace/workspace.yml");
    let text = std::fs::read_to_string(workspace).unwrap();
    assert!(
        text.contains("name: Shop") || text.contains("Shop"),
        "{text}"
    );
    assert!(text.contains("Billing"), "{text}");
}

#[test]
fn everything_comes_back_after_a_restart_but_collapsed_collections_stay_unloaded() {
    let port = server(Duration::ZERO);
    let (shop, billing) = (named(port, "Shop"), named(port, "Billing"));
    let data = tempfile::tempdir().unwrap();
    let draft_key;
    {
        let core = core_in(data.path());
        let shop_root = open_root(&core, shop.path());
        core.send(Command::SelectEnvironment(Some("local".into())));
        draft_key = TabKey::new(&shop_root, RequestPath::new("health.yml"));
        core.send(Command::OpenRequest(draft_key.clone()));
        let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
        core.send(Command::Edit {
            tab: draft_key.clone(),
            request: edited(&snapshot, "{{host}}/unsaved"),
        });
        let billing_root = open_root(&core, billing.path());
        core.send(Command::ExpandCollection {
            root: billing_root,
            expanded: false,
        });
        core.send(Command::ActivateTab(draft_key.clone()));
        wait(&core, "the draft", |s| {
            s.active_tab().is_some_and(apifast_gui::core::Tab::is_dirty)
        });
    }
    let core = core_in(data.path());
    core.send(Command::Restore);
    let snapshot = wait(&core, "the restored tab", |s| s.active.is_some());
    assert_eq!(snapshot.active.as_ref(), Some(&draft_key));
    assert_eq!(
        snapshot.active_tab().unwrap().request().url,
        "{{host}}/unsaved"
    );
    assert_eq!(snapshot.environment.as_deref(), Some("local"));
    let billing = &snapshot.collections[1];
    assert_eq!(billing.name, "Billing", "named from the workspace file");
    assert!(!billing.expanded && billing.tree.is_none(), "not loaded");
}

#[test]
fn closing_a_collection_keeps_its_drafts_and_removing_it_keeps_its_files() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, data) = start();
    let root = open_root(&core, dir.path());
    let key = TabKey::new(&root, RequestPath::new("health.yml"));
    core.send(Command::OpenRequest(key.clone()));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::Edit {
        tab: key.clone(),
        request: edited(&snapshot, "{{host}}/kept"),
    });
    wait(&core, "the draft", |s| {
        s.active_tab().is_some_and(apifast_gui::core::Tab::is_dirty)
    });

    core.send(Command::CloseCollection(root.clone()));
    let snapshot = wait(&core, "closed", |s| s.tabs.is_empty());
    assert!(snapshot.collection.is_none());
    assert!(!snapshot.collections[0].expanded && snapshot.collections[0].tree.is_none());

    core.send(Command::OpenRequest(key.clone()));
    let snapshot = wait(&core, "reopened", |s| s.active_tab().is_some());
    assert_eq!(
        snapshot.active_tab().unwrap().request().url,
        "{{host}}/kept"
    );

    core.send(Command::RemoveCollection(root));
    let snapshot = wait(&core, "removed", |s| s.collections.is_empty());
    assert_eq!(snapshot.tabs.len(), 0);
    assert!(dir.path().join("health.yml").exists());
    let text =
        std::fs::read_to_string(data.path().join("default-workspace/workspace.yml")).unwrap();
    assert!(!text.contains("path:"), "{text}");
}

#[test]
fn workspaces_switch_with_their_own_collections_and_tabs() {
    let port = server(Duration::ZERO);
    let (shop, billing) = (named(port, "Shop"), named(port, "Billing"));
    let team = tempfile::tempdir().unwrap();
    let (core, _, _data) = start();
    let shop_root = open_root(&core, shop.path());
    let shop_tab = TabKey::new(&shop_root, RequestPath::new("health.yml"));
    core.send(Command::OpenRequest(shop_tab.clone()));
    wait(&core, "the shop tab", |s| s.active.is_some());
    let default = core.snapshot().workspace.clone().unwrap();
    assert_eq!(default.name, "My workspace");

    core.send(Command::CreateWorkspace {
        folder: team.path().join("team"),
        name: "Team".into(),
    });
    let snapshot = wait(&core, "the team workspace", |s| {
        s.workspace.as_ref().is_some_and(|w| w.name == "Team")
    });
    assert!(snapshot.collections.is_empty() && snapshot.tabs.is_empty());
    assert_eq!(snapshot.workspaces.len(), 2);
    // The same collection can be in both.
    open_root(&core, shop.path());
    open_root(&core, billing.path());
    let snapshot = core.snapshot();
    assert_eq!(snapshot.collections.len(), 2);
    assert_eq!(
        snapshot.default_parent,
        team.path().join("team").canonicalize().unwrap()
    );

    core.send(Command::OpenWorkspace(default.folder.clone()));
    let snapshot = wait(&core, "back", |s| {
        s.workspace
            .as_ref()
            .is_some_and(|w| w.name == "My workspace")
    });
    assert_eq!(snapshot.collections.len(), 1);
    assert_eq!(
        snapshot.active.as_ref(),
        Some(&shop_tab),
        "its tabs came back"
    );

    core.send(Command::RenameWorkspace("Mine".into()));
    wait(&core, "renamed", |s| {
        s.workspace.as_ref().is_some_and(|w| w.name == "Mine")
    });
    core.send(Command::ForgetWorkspace(default.folder.clone()));
    let snapshot = wait(&core, "the refusal", |s| s.error.is_some());
    assert_eq!(
        snapshot.workspaces.len(),
        2,
        "the active one is not forgotten"
    );
}

#[test]
fn a_workspace_file_changed_outside_updates_the_list() {
    let port = server(Duration::ZERO);
    let (shop, billing) = (named(port, "Shop"), named(port, "Billing"));
    let (core, _, data) = start();
    let shop_root = open_root(&core, shop.path());
    open_root(&core, billing.path());
    let file = data.path().join("default-workspace/workspace.yml");
    // As after a `git pull` that removed Billing.
    std::fs::write(
        &file,
        format!(
            "opencollection: 1.0.0\ninfo:\n  name: Pulled\n  type: workspace\ncollections:\n  - name: Shop\n    path: {}\n",
            shop_root.display()
        ),
    )
    .unwrap();
    let snapshot = wait(&core, "the pulled list", |s| s.collections.len() == 1);
    assert_eq!(snapshot.collections[0].root, shop_root);
    assert_eq!(snapshot.workspace.as_ref().unwrap().name, "Pulled");
}

#[test]
fn a_collection_whose_folder_is_gone_shows_why() {
    let dir = collection(server(Duration::ZERO));
    let data = tempfile::tempdir().unwrap();
    let root = {
        let core = core_in(data.path());
        let root = open_root(&core, dir.path());
        core.send(Command::CloseCollection(root.clone()));
        wait(&core, "closed", |s| s.collection.is_none());
        // As the workspace lists it: resolved, as the interface would send it.
        root
    };
    let gone = root;
    drop(dir);
    assert!(!gone.exists());
    let core = core_in(data.path());
    core.send(Command::ExpandCollection {
        root: gone,
        expanded: true,
    });
    let snapshot = wait(&core, "the error", |s| {
        s.collections.first().is_some_and(|c| c.error.is_some())
    });
    assert!(snapshot.collections[0].tree.is_none());
}

#[test]
fn the_collection_key_is_never_asked_of_its_creator_nor_twice_on_a_machine() {
    use apifast_gui::core::VaultCommand;

    let dir = collection(server(Duration::ZERO));
    let (alice, bob) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let needs_key = |s: &Snapshot| s.collections.first().is_some_and(|c| c.needs_key);

    // Alice shares the first secret: the key is made, nothing is asked.
    let key = {
        let core = core_in(alice.path());
        open_root(&core, dir.path());
        let snapshot = core.snapshot();
        core.send(Command::SaveEnvironment(Box::new(with_secret_token(
            &snapshot,
        ))));
        wait(&core, "the secret variable", |s| {
            !s.vault.secrets.is_empty()
        });
        core.send(Command::Vault(VaultCommand::Set {
            environment: "local".into(),
            name: "token".into(),
            value: apifast_domain::Secret::new("t0k3n"),
            shared: true,
        }));
        let snapshot = wait(&core, "shared", |s| s.vault.shares);
        assert!(snapshot.vault.has_key && !needs_key(&snapshot));
        // She does not remember it: it can be shown, to send it.
        core.send(Command::Vault(VaultCommand::ShowKey));
        let snapshot = wait(&core, "the key", |s| s.vault.shareable_key.is_some());
        snapshot.vault.shareable_key.clone().unwrap()
    };
    // After a restart she is still not asked.
    {
        let core = core_in(alice.path());
        core.send(Command::Restore);
        let snapshot = wait(&core, "restored", |s| s.vault.shares);
        assert!(snapshot.vault.has_key && !needs_key(&snapshot));
    }

    // Bob's machine has never seen it: asked once, by its root's mark.
    {
        let core = core_in(bob.path());
        open_root(&core, dir.path());
        let snapshot = wait(&core, "the vault", |s| s.vault.shares);
        assert!(!snapshot.vault.has_key && needs_key(&snapshot));
        core.send(Command::Vault(VaultCommand::ImportKey(key)));
        let snapshot = wait(&core, "the key taken", |s| s.vault.has_key);
        assert!(!needs_key(&snapshot));
    }
    // And never again on his machine.
    let core = core_in(bob.path());
    core.send(Command::Restore);
    let snapshot = wait(&core, "restored", |s| s.vault.shares);
    assert!(snapshot.vault.has_key && !needs_key(&snapshot));
}

#[test]
fn environment_captures_are_saved_when_the_user_agrees_and_secrets_go_to_the_vault() {
    let dir = collection(server(Duration::ZERO));
    let capture = |expression: &str, name: &str| {
        format!(
            "    - type: set-variable\n      phase: after-response\n      selector:\n        expression: {expression}\n        method: jsonq\n      variable:\n        name: {name}\n        scope: environment\n"
        )
    };
    std::fs::write(
        dir.path().join("users/get.yml"),
        format!(
            "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: \"{{{{host}}}}/users/1\"\nruntime:\n  actions:\n{}{}",
            capture("res.status", "last_status"),
            capture("res.body", "token"),
        ),
    )
    .unwrap();
    let local = dir.path().join("environments/local.yml");
    let text = std::fs::read_to_string(&local).unwrap();
    std::fs::write(&local, format!("{text}  - name: token\n    secret: true\n")).unwrap();

    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "the captures", |s| s.captured.is_some());
    let captured = snapshot.captured.as_ref().unwrap();
    assert_eq!(captured.environment, "local");
    assert_eq!(captured.names, ["last_status", "token"]);

    // Discarding writes nothing.
    core.send(Command::SaveCaptures {
        root: root(&core),
        save: false,
    });
    wait(&core, "discarded", |s| s.captured.is_none());
    assert!(
        !std::fs::read_to_string(&local)
            .unwrap()
            .contains("last_status")
    );

    core.send(Command::Send);
    wait(&core, "the captures again", |s| s.captured.is_some());
    core.send(Command::SaveCaptures {
        root: root(&core),
        save: true,
    });
    wait(&core, "saved", |s| s.captured.is_none());
    let text = std::fs::read_to_string(&local).unwrap();
    // A number keeps its type, as OpenCollection writes typed values.
    assert!(
        text.contains("- name: last_status\n    value:\n      type: number\n      data: \"200\""),
        "{text}"
    );
    assert!(
        !text.contains("GET /users"),
        "a secret never reaches the file: {text}"
    );

    core.send(Command::Vault(apifast_gui::core::VaultCommand::Reveal {
        environment: "local".into(),
        name: "token".into(),
    }));
    let snapshot = wait(&core, "the secret", |s| {
        s.vault.revealed.is_some() || s.error.is_some()
    });
    assert_eq!(snapshot.error, None);
    assert_eq!(
        snapshot.vault.revealed.as_ref().unwrap().value.expose(),
        "GET /users/1 HTTP/1.1"
    );
}

#[test]
fn the_runner_runs_a_folder_in_the_background_and_opens_a_step() {
    use apifast_gui::core::RunSettings;
    let dir = collection(server(Duration::ZERO));
    std::fs::write(
        dir.path().join("users/get.yml"),
        "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users/1\"\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"201\"\n",
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::Run {
        root: root(&core),
        settings: RunSettings {
            iterations: 2,
            ..RunSettings::default()
        },
    });
    let snapshot = wait(&core, "the run", |s| {
        s.run.as_ref().is_some_and(|run| !run.running)
    });
    let run = snapshot.run.as_ref().unwrap();
    assert_eq!(run.error, None);
    assert_eq!((run.total, run.iterations, run.steps.len()), (2, 2, 4));
    let report = run.report.as_ref().unwrap();
    let summary = report.summary();
    assert_eq!((summary.passed, summary.failed), (2, 2));
    assert!((run.progress() - 1.0).abs() < f32::EPSILON);
    // Each step went to the history.
    wait(&core, "the history", |s| s.history.len() == 4);
    // In tree order: the folder has `seq: 1`, so it comes before health.
    let order: Vec<&str> = run.steps.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        order,
        ["users/get.yml", "health.yml", "users/get.yml", "health.yml"]
    );

    // A step opens its request with its response and checks.
    let get = run
        .steps
        .iter()
        .position(|s| s.path.as_str() == "users/get.yml")
        .unwrap();
    core.send(Command::OpenRunStep(get));
    let snapshot = wait(&core, "the step", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert_eq!(received.checks.assertions.len(), 1);
    assert!(!received.checks.assertions[0].verdict.passed);

    core.send(Command::CloseRun);
    wait(&core, "closed", |s| s.run.is_none());
}

/// A token endpoint at `/token` that numbers its tokens, and an API that
/// answers with the `Authorization` it got.
fn token_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let issued = Arc::new(AtomicUsize::new(0));
    let count = issued.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).ok();
            let (mut line, mut length, mut authorization) = (String::new(), 0, String::new());
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                let lower = line.to_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if lower.starts_with("authorization:") {
                    line[14..].trim().clone_into(&mut authorization);
                }
                line.clear();
            }
            let mut body = vec![0; length];
            std::io::Read::read_exact(&mut reader, &mut body).ok();
            let reply = if first.contains("/token") {
                let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                format!(r#"{{"access_token":"secret-token-{n}","expires_in":3600}}"#)
            } else {
                authorization
            };
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (port, issued)
}

#[test]
fn oauth2_tokens_are_kept_sealed_got_on_demand_and_forgotten() {
    use apifast_gui::core::TokenState;
    let (port, issued) = token_server();
    let dir = collection(port);
    std::fs::write(
        dir.path().join("users/get.yml"),
        format!("info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: http://127.0.0.1:{port}/users\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: http://127.0.0.1:{port}/token\n    credentials:\n      clientId: cli\n      clientSecret: shh\n"),
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let start_core = || Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    let token_of = |s: &Snapshot| s.active_tab().map(|t| t.context.token.clone());

    let core = start_core();
    open(&core, dir.path());
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "no token", |s| {
        token_of(s) == Some(TokenState::Missing)
    });

    let key = at(&core, "users/get.yml");
    core.send(Command::GetToken(key.clone()));
    let snapshot = wait(&core, "a token", |s| {
        matches!(token_of(s), Some(TokenState::Stored(_))) || s.error.is_some()
    });
    assert_eq!(snapshot.error, None);
    let Some(TokenState::Stored(info)) = token_of(&snapshot) else {
        unreachable!()
    };
    assert!(info.expires_at_ms.is_some());

    // Sealed on disk.
    let tokens = std::fs::read_dir(data.path().join("oauth2")).unwrap();
    for file in tokens.flatten() {
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(!text.contains("secret-token"), "{text}");
    }

    // A send uses it, without asking for another.
    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(
            response(s),
            ResponseState::Received(_) | ResponseState::Failed(_)
        )
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        panic!("{:?}", response(&snapshot));
    };
    assert_eq!(
        received.body.read(0, 100).unwrap(),
        b"Bearer secret-token-1"
    );
    assert_eq!(issued.load(Ordering::Relaxed), 1);
    drop(core);

    // Still there after a restart.
    let core = start_core();
    open(&core, dir.path());
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "the kept token", |s| {
        matches!(token_of(s), Some(TokenState::Stored(_)))
    });
    core.send(Command::ForgetToken(at(&core, "users/get.yml")));
    wait(&core, "forgotten", |s| {
        token_of(s) == Some(TokenState::Missing)
    });
}

/// An HTTP proxy that answers every request itself with `via proxy`.
fn http_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nvia proxy"
            );
        }
    });
    port
}

#[test]
fn a_collection_proxy_is_saved_and_used_by_the_next_send() {
    use apifast_domain::{CollectionProxy, ProxyConfig, ProxyMode};
    let dir = collection(server(Duration::ZERO));
    let proxy_port = http_proxy();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    let body_of = |core: &Core| {
        let sent = core.snapshot().history.len();
        core.send(Command::Send);
        // A new entry in the history: the tab may still show the last one.
        let snapshot = wait(core, "the response", |s| {
            s.history.len() > sent
                && matches!(
                    response(s),
                    ResponseState::Received(_) | ResponseState::Failed(_)
                )
        });
        let ResponseState::Received(received) = response(&snapshot) else {
            panic!("{:?}", response(&snapshot));
        };
        String::from_utf8(received.body.read(0, 100).unwrap()).unwrap()
    };
    assert_eq!(body_of(&core), "GET /health HTTP/1.1");

    let proxy = CollectionProxy {
        mode: ProxyMode::Custom,
        config: ProxyConfig {
            hostname: "127.0.0.1".into(),
            port: proxy_port.to_string(),
            ..ProxyConfig::default()
        },
    };
    core.send(Command::SaveCollectionSettings {
        root: root(&core),
        client_certificates: Vec::new(),
        dns_overrides: Vec::new(),
        proxy: proxy.clone(),
    });
    wait(&core, "the proxy saved", |s| {
        s.collection
            .as_ref()
            .is_some_and(|c| c.tree.collection.proxy == proxy)
    });
    assert!(
        std::fs::read_to_string(dir.path().join("opencollection.yml"))
            .unwrap()
            .contains("inherit: false")
    );
    assert_eq!(body_of(&core), "via proxy");

    core.send(Command::SaveCollectionSettings {
        root: root(&core),
        client_certificates: Vec::new(),
        dns_overrides: Vec::new(),
        proxy: CollectionProxy {
            mode: ProxyMode::Off,
            ..proxy
        },
    });
    wait(&core, "no proxy", |s| {
        s.collection
            .as_ref()
            .is_some_and(|c| c.tree.collection.proxy.mode == ProxyMode::Off)
    });
    assert_eq!(body_of(&core), "GET /health HTTP/1.1");
}

#[test]
fn dns_overrides_are_saved_apart_and_used_by_the_next_send() {
    let port = server(Duration::ZERO);
    let dir = collection(port);
    std::fs::write(
        dir.path().join("environments/named.yml"),
        format!("name: named\nvariables:\n  - name: host\n    value: http://shop.internal.invalid:{port}\n"),
    )
    .unwrap();
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("named".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "the DNS error", |s| {
        matches!(response(s), ResponseState::Failed(_))
    });
    let ResponseState::Failed(message) = response(&snapshot) else {
        unreachable!()
    };
    assert!(message.contains("shop.internal.invalid"), "{message}");

    let dns = vec![apifast_domain::DnsOverride {
        host: "*.internal.invalid".into(),
        address: "127.0.0.1".into(),
        enabled: true,
    }];
    core.send(Command::SaveCollectionSettings {
        root: root(&core),
        proxy: apifast_domain::CollectionProxy::default(),
        client_certificates: Vec::new(),
        dns_overrides: dns.clone(),
    });
    wait(&core, "the overrides saved", |s| {
        s.collection
            .as_ref()
            .is_some_and(|c| c.tree.collection.dns_overrides == dns)
    });
    let apifast = std::fs::read_to_string(dir.path().join(".apifast/collection.yml")).unwrap();
    assert!(apifast.contains("*.internal.invalid"), "{apifast}");
    let sent = core.snapshot().history.len();
    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        s.history.len() > sent && matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert_eq!(received.body.read(0, 100).unwrap(), b"GET /health HTTP/1.1");
}

#[test]
fn a_request_is_copied_as_curl_with_its_environment_and_draft() {
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    let key = at(&core, "users/get.yml");
    core.send(Command::CopyAs(
        key.clone(),
        apifast_import::CodeLanguage::Curl,
    ));
    let snapshot = wait(&core, "the copy", |s| s.copied.is_some());
    let copied = snapshot.copied.clone().unwrap();
    assert!(
        copied.text.starts_with("curl --request GET"),
        "{}",
        copied.text
    );
    assert!(
        copied.text.contains("--url 'http://127.0.0.1:"),
        "{}",
        copied.text
    );
    assert_eq!(copied.note, None);

    // With the tab's draft, and a variable missing.
    core.send(Command::OpenRequest(key.clone()));
    wait(&core, "the tab", |s| s.active.as_ref() == Some(&key));
    let mut draft = active_tab(&core.snapshot()).request().clone();
    draft.url = "{{host}}/users/{{missing}}".into();
    core.send(Command::Edit {
        tab: key.clone(),
        request: Box::new(draft),
    });
    core.send(Command::CopyAs(key, apifast_import::CodeLanguage::Curl));
    let snapshot = wait(&core, "the second copy", |s| {
        s.copied.as_ref().is_some_and(|c| c.id != copied.id)
    });
    let second = snapshot.copied.clone().unwrap();
    assert!(second.text.contains("/users/'"), "{}", second.text);
    assert!(second.note.clone().unwrap().contains("missing"));

    // The same request as Python.
    let key = at(&core, "users/get.yml");
    core.send(Command::CopyAs(key, apifast_import::CodeLanguage::Python));
    let snapshot = wait(&core, "the Python copy", |s| {
        s.copied.as_ref().is_some_and(|c| c.id != second.id)
    });
    let python = snapshot.copied.clone().unwrap();
    assert_eq!(python.language, apifast_import::CodeLanguage::Python);
    assert!(
        python.text.starts_with("import requests\n"),
        "{}",
        python.text
    );
    assert!(python.text.contains("\"GET\""), "{}", python.text);
}

const POSTMAN: &str = r#"{
  "info": {"name": "Imported Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
  "item": [{"name": "Users", "item": [{"name": "List", "event": [{"listen": "test", "script": {"exec": ["pm.sendRequest('x')"]}}], "request": {"method": "GET", "url": "{{host}}/users"}}]}]
}"#;

#[test]
fn a_postman_export_is_reviewed_then_imported_and_opened() {
    let (core, _, _data) = start();
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("shop.json");
    std::fs::write(&export, POSTMAN).unwrap();
    std::fs::write(dir.path().join("bad.json"), "{}").unwrap();

    core.send(Command::PlanImport {
        collection: dir.path().join("bad.json"),
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the refusal", |s| s.import.is_some());
    assert!(snapshot.import.as_ref().unwrap().plan.is_err());

    core.send(Command::PlanImport {
        collection: export.clone(),
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the plan", |s| {
        s.import.as_ref().is_some_and(|i| i.plan.is_ok())
    });
    let view = snapshot.import.clone().unwrap();
    assert_eq!(view.files, [export]);
    let plan = view.plan.unwrap();
    assert_eq!(plan.collection, "Imported Shop");
    assert_eq!(plan.counts(), (1, 1));
    assert_eq!(plan.warnings.len(), 1);

    let target = dir.path().join("imported-shop");
    core.send(Command::ApplyImport(target.clone()));
    // Resolved by its parent, which exists: the target does not yet.
    let root = dir.path().canonicalize().unwrap().join("imported-shop");
    let snapshot = wait(&core, "the collection open", |s| {
        s.import.is_none() && s.collection.as_ref().is_some_and(|c| c.root == root)
    });
    assert_eq!(
        snapshot.collection.as_ref().unwrap().tree.collection.name,
        "Imported Shop"
    );
    assert!(target.join("users/list.yml").exists());

    // Never over a collection: the plan stays for another folder.
    core.send(Command::PlanImport {
        collection: dir.path().join("shop.json"),
        environments: Vec::new(),
    });
    wait(&core, "the second plan", |s| s.import.is_some());
    core.send(Command::ApplyImport(target));
    let snapshot = wait(&core, "the error", |s| s.error.is_some());
    assert!(snapshot.import.is_some());
    assert!(snapshot.error.as_ref().unwrap().contains("already exists"));
}

#[test]
fn an_openapi_spec_is_recognised_and_planned() {
    let (core, _, _data) = start();
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("pets.yaml");
    std::fs::write(
        &spec,
        "openapi: 3.1.0\ninfo:\n  title: Pets\npaths:\n  /pets:\n    get:\n      summary: List pets\n      responses: {}\n",
    )
    .unwrap();
    core.send(Command::PlanImport {
        collection: spec,
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the plan", |s| s.import.is_some());
    let plan = snapshot.import.clone().unwrap().plan.unwrap();
    assert_eq!(plan.collection, "Pets");
    assert_eq!(plan.entries[0].path, "list-pets.yml");

    let other = dir.path().join("notes.txt");
    std::fs::write(&other, "just: text").unwrap();
    core.send(Command::PlanImport {
        collection: other,
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the refusal", |s| {
        s.import.as_ref().is_some_and(|i| i.plan.is_err())
    });
    let why = snapshot.import.clone().unwrap().plan.unwrap_err();
    assert!(why.contains("nor an OpenAPI 3 spec"), "{why}");

    let insomnia = dir.path().join("insomnia.yaml");
    std::fs::write(
        &insomnia,
        "type: collection.insomnia.rest/5.0\nname: From Insomnia\ncollection:\n  - name: Ping\n    method: GET\n    url: https://h/ping\n",
    )
    .unwrap();
    core.send(Command::PlanImport {
        collection: insomnia,
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the Insomnia plan", |s| {
        s.import
            .as_ref()
            .and_then(|i| i.plan.as_ref().ok())
            .is_some_and(|p| p.collection == "From Insomnia")
    });
    assert_eq!(
        snapshot.import.clone().unwrap().plan.unwrap().entries[0].path,
        "ping.yml"
    );
}

#[test]
fn a_bru_folder_is_recognised_and_planned() {
    let (core, _, _data) = start();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bruno.json"), r#"{"name": "Old"}"#).unwrap();
    std::fs::write(
        dir.path().join("ping.bru"),
        "meta {\n  name: Ping\n}\n\nget {\n  url: http://h/ping\n}\n",
    )
    .unwrap();
    core.send(Command::PlanImport {
        collection: dir.path().to_path_buf(),
        environments: Vec::new(),
    });
    let snapshot = wait(&core, "the plan", |s| s.import.is_some());
    let plan = snapshot.import.clone().unwrap().plan.unwrap();
    assert_eq!(
        (plan.collection.as_str(), plan.entries[0].path.as_str()),
        ("Old", "ping.yml")
    );
}

#[test]
fn an_imported_collection_is_imported_again_with_the_chosen_changes() {
    let (core, _, _data) = start();
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("pets.yaml");
    let version = |summary: &str, extra: &str| {
        format!(
            "openapi: 3.0.0\ninfo: {{title: Pets, version: '1'}}\npaths:\n  /pets:\n    get: {{operationId: listPets, summary: {summary}}}\n  /pets/{{id}}:\n    get: {{operationId: getPet, summary: Get pet}}\n{extra}"
        )
    };
    std::fs::write(&spec, version("List pets", "")).unwrap();
    core.send(Command::PlanImport {
        collection: spec.clone(),
        environments: Vec::new(),
    });
    wait(&core, "the plan", |s| {
        s.import.as_ref().is_some_and(|i| i.plan.is_ok())
    });
    let target = dir.path().join("pets");
    core.send(Command::ApplyImport(target.clone()));
    let snapshot = wait(&core, "the collection open", |s| {
        s.import.is_none() && s.collection.is_some()
    });
    let root = snapshot.collection.as_ref().unwrap().root.clone();
    assert_eq!(root, target.canonicalize().unwrap());

    // The spec changes a request and adds one.
    let extra = "  /owners:\n    get: {operationId: listOwners, summary: List owners}\n";
    std::fs::write(&spec, version("All pets", extra)).unwrap();
    core.send(Command::OpenReimport(root.clone()));
    let snapshot = wait(&core, "the reimport", |s| s.reimport.is_some());
    let view = snapshot.reimport.clone().unwrap();
    assert!(view.plan.is_none());
    let previous = view.previous_source.unwrap().canonicalize().unwrap();
    assert_eq!(previous, spec.canonicalize().unwrap(), "proposed again");

    core.send(Command::PlanReimport(spec.clone()));
    let snapshot = wait(&core, "the reimport plan", |s| {
        s.reimport.as_ref().is_some_and(|r| r.plan.is_some())
    });
    let plan = snapshot.reimport.clone().unwrap().plan.unwrap().unwrap();
    let action = |key: &str| plan.entries.iter().find(|e| e.key == key).unwrap().action;
    assert_eq!(action("listPets"), apifast_app::ReimportAction::Update);
    assert_eq!(action("listOwners"), apifast_app::ReimportAction::Create);
    assert_eq!(action("getPet"), apifast_app::ReimportAction::Unchanged);

    // Only the update is chosen.
    core.send(Command::ApplyReimport(vec!["listPets".into()]));
    wait(&core, "the reimport applied", |s| s.reimport.is_none());
    let list = std::fs::read_to_string(target.join("pets/list-pets.yml"))
        .or_else(|_| std::fs::read_to_string(target.join("list-pets.yml")))
        .unwrap();
    assert!(list.contains("All pets"), "{list}");
    assert!(!target.join("list-owners.yml").exists(), "not chosen");

    // A collection that was not imported says so.
    std::fs::remove_file(target.join(".apifast/import.yml")).unwrap();
    core.send(Command::OpenReimport(root));
    let snapshot = wait(&core, "the refusal", |s| {
        s.reimport.as_ref().is_some_and(|r| r.plan.is_some())
    });
    let why = snapshot
        .reimport
        .clone()
        .unwrap()
        .plan
        .unwrap()
        .unwrap_err();
    assert!(why.contains("did not come from an import"), "{why}");
}

#[test]
fn a_collection_is_exported_to_postman_with_its_environments() {
    let (core, _, _data) = start();
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("shop.json");
    std::fs::write(&export, POSTMAN).unwrap();
    core.send(Command::PlanImport {
        collection: export,
        environments: Vec::new(),
    });
    wait(&core, "the plan", |s| {
        s.import.as_ref().is_some_and(|i| i.plan.is_ok())
    });
    let target = dir.path().join("shop");
    core.send(Command::ApplyImport(target.clone()));
    let snapshot = wait(&core, "the collection open", |s| {
        s.import.is_none() && s.collection.is_some()
    });
    let root = snapshot.collection.as_ref().unwrap().root.clone();
    std::fs::create_dir_all(target.join("environments")).unwrap();
    std::fs::write(
        target.join("environments/local.yml"),
        "name: local\nvariables:\n  - name: host\n    value: http://localhost\n",
    )
    .unwrap();

    let file = dir.path().join("out.postman_collection.json");
    core.send(Command::ExportPostman {
        root: root.clone(),
        file: file.clone(),
    });
    let snapshot = wait(&core, "the export", |s| s.exported.is_some());
    let exported = snapshot.exported.clone().unwrap();
    assert_eq!(exported.environments, 1);
    assert!(exported.result.is_ok(), "{:?}", exported.result);
    let back = apifast_import::import_postman(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back.name, "Imported Shop");
    assert!(dir.path().join("local.postman_environment.json").exists());

    // A folder that cannot be written says why.
    let first = exported.id;
    core.send(Command::ExportPostman {
        root,
        file: dir.path().join("missing/out.json"),
    });
    let snapshot = wait(&core, "the failure", |s| {
        s.exported.as_ref().is_some_and(|e| e.id != first)
    });
    let why = snapshot.exported.clone().unwrap().result.unwrap_err();
    assert!(why.contains("cannot write"), "{why}");
}

/// A GraphQL server: the introspection query gets a small schema, unless
/// the path is `/closed`, whose introspection is off.
fn graphql_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).ok();
                let mut length = 0;
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let mut body = vec![0; length];
                std::io::Read::read_exact(&mut reader, &mut body).ok();
                let asked = String::from_utf8_lossy(&body).contains("IntrospectionQuery");
                let answer = if first.contains("/closed") {
                    r#"{"errors": [{"message": "introspection is off"}]}"#
                } else if asked {
                    r#"{"data": {"__schema": {"queryType": {"name": "Query"}, "types": [
                        {"kind": "OBJECT", "name": "Query", "fields": [{"name": "me", "args": [], "type": {"kind": "OBJECT", "name": "User"}}]},
                        {"kind": "OBJECT", "name": "User", "fields": [{"name": "id", "args": [], "type": {"kind": "SCALAR", "name": "ID"}}]},
                        {"kind": "SCALAR", "name": "ID"}]}}}"#
                } else {
                    "{}"
                };
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            });
        }
    });
    port
}

#[test]
fn a_graphql_schema_is_fetched_kept_and_read_again() {
    let port = graphql_server();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: G\n",
    )
    .unwrap();
    for (name, path) in [("me", "graphql"), ("closed", "closed")] {
        std::fs::write(
            dir.path().join(format!("{name}.yml")),
            format!("info:\n  name: {name}\n  type: graphql\ngraphql:\n  url: http://127.0.0.1:{port}/{path}\n  body:\n    query: '{{ me {{ id }} }}'\n    variables: ''\n"),
        )
        .unwrap();
    }
    let (core, _, data) = start();
    open(&core, dir.path());
    let key = at(&core, "me.yml");
    core.send(Command::OpenRequest(key.clone()));
    let snapshot = wait(&core, "the tab", |s| s.active.as_ref() == Some(&key));
    let view = active_tab(&snapshot).context.schema.clone().unwrap();
    assert!(view.schema.is_none() && !view.fetching);

    core.send(Command::FetchSchema(key.clone()));
    let snapshot = wait(&core, "the schema", |s| {
        s.active_tab()
            .and_then(|t| t.context.schema.as_ref())
            .is_some_and(|v| v.schema.is_some())
    });
    let view = active_tab(&snapshot).context.schema.clone().unwrap();
    let schema = view.schema.unwrap();
    assert_eq!(schema.query.as_deref(), Some("Query"));
    assert_eq!(schema.type_named("User").unwrap().fields[0].name, "id");
    assert_eq!(view.error, None);
    assert!(data.path().join("schemas").exists(), "kept on disk");
    drop(core);

    // Another session finds it without asking.
    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    open(&core, dir.path());
    let key = at(&core, "me.yml");
    core.send(Command::OpenRequest(key.clone()));
    let snapshot = wait(&core, "the kept schema", |s| {
        s.active_tab()
            .and_then(|t| t.context.schema.as_ref())
            .is_some_and(|v| v.schema.is_some())
    });
    assert!(
        !active_tab(&snapshot)
            .context
            .schema
            .as_ref()
            .unwrap()
            .fetching
    );

    // A server with introspection off says so.
    let closed = at(&core, "closed.yml");
    core.send(Command::OpenRequest(closed.clone()));
    wait(&core, "the closed tab", |s| {
        s.active.as_ref() == Some(&closed)
    });
    core.send(Command::FetchSchema(closed));
    let snapshot = wait(&core, "the refusal", |s| {
        s.active_tab()
            .and_then(|t| t.context.schema.as_ref())
            .is_some_and(|v| v.error.is_some())
    });
    let why = active_tab(&snapshot)
        .context
        .schema
        .clone()
        .unwrap()
        .error
        .unwrap();
    assert!(why.contains("introspection is off"), "{why}");
}

#[test]
fn folder_scripts_are_saved_and_run_when_sending() {
    use apifast_domain::{ScriptBlock, ScriptKind};
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    let snapshot = open(&core, dir.path());
    core.send(Command::SaveScripts {
        root: root(&core),
        folder: Some("users".into()),
        scripts: vec![
            ScriptBlock::new(ScriptKind::BeforeRequest, "req.setUrl(req.getUrl() + '?from=folder')"),
            ScriptBlock::new(
                ScriptKind::Tests,
                "test('the folder changed the URL', () => expect(res.body).to.contain('from=folder'))",
            ),
        ],
    });
    next(&core, snapshot.version);
    let file = std::fs::read_to_string(dir.path().join("users/folder.yml")).unwrap();
    assert!(file.contains("type: before-request"), "{file}");
    assert!(file.contains("seq: 1"), "{file}");

    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "the request", |s| s.active_tab().is_some());
    core.send(Command::Send);
    let snapshot = wait(&core, "the response", |s| {
        matches!(
            response(s),
            ResponseState::Received(_) | ResponseState::Failed(_)
        )
    });
    let received = received(&snapshot);
    assert_eq!(
        received.body.read(0, 100).unwrap(),
        b"GET /users/1?from=folder HTTP/1.1"
    );
    let tests: Vec<_> = received.checks.scripts.tests().cloned().collect();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].error, None, "{tests:?}");
}

#[test]
fn collection_scripts_are_saved_in_its_file() {
    use apifast_domain::{ScriptBlock, ScriptKind};
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::SaveScripts {
        root: root(&core),
        folder: None,
        scripts: vec![ScriptBlock::new(
            ScriptKind::AfterResponse,
            "console.log(1)",
        )],
    });
    // The next snapshot may be another message's (the history loaded):
    // wait for the scripts.
    wait(&core, "the collection's scripts", |s| {
        s.collection
            .as_ref()
            .is_some_and(|c| c.tree.collection.defaults.scripts.len() == 1)
    });
    let file = std::fs::read_to_string(dir.path().join("opencollection.yml")).unwrap();
    assert!(file.contains("console.log(1)"), "{file}");
}

#[test]
fn collection_and_folder_variables_are_saved_and_reach_the_open_tab() {
    use apifast_domain::{Origin, Variable};
    let dir = collection(server(Duration::ZERO));
    let (core, _, _data) = start();
    open(&core, dir.path());
    core.send(Command::OpenRequest(at(&core, "users/get.yml")));
    wait(&core, "the tab", |s| s.active_tab().is_some());
    core.send(Command::SaveVariables {
        root: root(&core),
        folder: None,
        variables: vec![Variable::text("greeting", "hi"), Variable::text("id", "1")],
    });
    core.send(Command::SaveVariables {
        root: root(&core),
        folder: Some("users".into()),
        variables: vec![Variable::text("id", "2")],
    });
    let snapshot = wait(&core, "the variables in the tab", |s| {
        s.active_tab().is_some_and(|t| {
            let variables = &t.context.variables;
            variables.get("greeting").is_some()
                && variables
                    .get("id")
                    .is_some_and(|v| v.value.as_ref().is_some_and(|value| value.data == "2"))
        })
    });
    let variables = &snapshot.active_tab().unwrap().context.variables;
    assert_eq!(
        variables.get("greeting").unwrap().origin,
        Origin::Collection
    );
    // The folder's overrides the collection's.
    assert!(matches!(
        variables.get("id").unwrap().origin,
        Origin::Folder(_)
    ));
    let file = std::fs::read_to_string(dir.path().join("opencollection.yml")).unwrap();
    assert!(file.contains("greeting"), "{file}");
    assert!(
        file.contains("info:\n  name: API"),
        "the rest is kept: {file}"
    );
    let folder = std::fs::read_to_string(dir.path().join("users/folder.yml")).unwrap();
    assert!(folder.contains("name: id"), "{folder}");
}

/// Tells `core` the settings a test needs: the default timeout, and
/// whether the workspace used last opens at the start.
fn configure(core: &Core, timeout_ms: u64, reopen_last: bool) {
    core.send(Command::Configure {
        history: apifast_gui::core::HISTORY_LIMITS,
        responses: apifast_gui::core::RESPONSE_LIMITS,
        proxy: apifast_domain::ProxyChoice::System,
        trust: apifast_domain::TlsTrust::default(),
        cookies: (true, true),
        timeout_ms,
        reopen_last,
    });
}

#[test]
fn the_default_timeout_of_the_settings_reaches_requests_that_set_none() {
    let dir = collection(server(Duration::from_secs(5)));
    let (core, _, _data) = start();
    configure(&core, 200, true);
    open(&core, dir.path());
    core.send(Command::SelectEnvironment(Some("local".into())));
    core.send(Command::OpenRequest(at(&core, "health.yml")));
    let snapshot = wait(&core, "the request", |s| s.active_tab().is_some());
    assert_eq!(
        snapshot
            .active_tab()
            .unwrap()
            .context
            .inherited_settings
            .timeout_ms,
        apifast_domain::Resolved::new(200, apifast_domain::Origin::Default)
    );

    let started = Instant::now();
    core.send(Command::Send);
    wait(&core, "the timeout", |s| {
        matches!(response(s), ResponseState::Failed(_))
    });
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn the_settings_choose_whether_the_last_workspace_opens_at_the_start() {
    let team = tempfile::tempdir().unwrap();
    let (core, _, data) = start();
    core.send(Command::CreateWorkspace {
        folder: team.path().join("team"),
        name: "Team".into(),
    });
    wait(&core, "the team workspace", |s| {
        s.workspace.as_ref().is_some_and(|w| w.name == "Team")
    });
    drop(core);

    let workspace_at_start = |reopen_last: bool| {
        let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
        configure(&core, 30_000, reopen_last);
        core.send(Command::Restore);
        let snapshot = wait(&core, "a workspace", |s| s.workspace.is_some());
        snapshot.workspace.clone().unwrap().name
    };
    assert_eq!(workspace_at_start(true), "Team");
    assert_eq!(workspace_at_start(false), "My workspace");
}
