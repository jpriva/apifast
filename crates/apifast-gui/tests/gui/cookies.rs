//! The cookie jar of a collection in the interface: filled by sends, sealed
//! on disk, edited from the editor, and turned off from the settings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use apifast_app::RequestPath;
use apifast_domain::{Cookie, SameSite};
use apifast_gui::core::{
    Command, CookieCommand, CookieSlot, Core, HISTORY_LIMITS, RESPONSE_LIMITS, ResponseState,
    Snapshot, TabKey,
};
use apifast_gui::organisms::{CookieDraft, DraftError};
use apifast_vault::SystemKeyring;

/// The value the server sets, to look for it on disk.
const SESSION: &str = "s3ss10n-value";

/// `/login` sets a session cookie; every answer's body is the `Cookie`
/// header the request had, or `-`.
fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream));
        }
    });
    port
}

fn answer(stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).ok();
    let mut cookie = "-".to_owned();
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        if let Some(value) = line.strip_prefix("cookie: ") {
            value.trim().clone_into(&mut cookie);
        }
        line.clear();
    }
    let set = if first.contains(" /login ") {
        format!("Set-Cookie: sid={SESSION}; Path=/; HttpOnly; SameSite=Lax\r\n")
    } else {
        String::new()
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\n{set}Content-Length: {}\r\nConnection: close\r\n\r\n{cookie}",
        cookie.len()
    );
}

fn collection(port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let request = |name: &str, path: &str| {
        format!(
            "info:\n  name: {name}\n  type: http\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}{path}\"\n"
        )
    };
    let files = [
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: Shop\n".to_owned(),
        ),
        ("login.yml", request("Login", "/login")),
        ("me.yml", request("Me", "/me")),
    ];
    for (path, text) in files {
        std::fs::write(dir.path().join(path), text).unwrap();
    }
    dir
}

fn core_in(data: &Path) -> Core {
    Core::start_with(data.to_path_buf(), SystemKeyring::off(), || {})
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

fn open(core: &Core, dir: &Path) -> std::path::PathBuf {
    let root = dunce::canonicalize(dir).unwrap();
    core.send(Command::OpenCollection(dir.to_path_buf()));
    wait(core, "the collection", |s| {
        s.collection.as_ref().is_some_and(|c| c.root == root)
    });
    root
}

/// The body of the response the active tab shows, by its key.
fn shown(snapshot: &Snapshot) -> Option<&apifast_gui::body::BodyHandle> {
    match &snapshot.active_tab()?.response {
        ResponseState::Received(received) => Some(&received.body),
        _ => None,
    }
}

/// Sends `path` and returns the body: the cookies the server got.
fn send(core: &Core, root: &Path, path: &str) -> String {
    let key = TabKey::new(root, RequestPath::new(path));
    core.send(Command::OpenRequest(key.clone()));
    let snapshot = wait(core, "the tab", |s| s.active.as_ref() == Some(&key));
    // The tab may show an earlier response: wait for another one.
    let before = shown(&snapshot).map(|body| body.key().to_owned());
    core.send(Command::Send);
    let snapshot = wait(core, "the response", |s| {
        shown(s).is_some_and(|body| Some(body.key()) != before.as_deref())
    });
    let body = shown(&snapshot).unwrap();
    String::from_utf8(body.read(0, 1000).unwrap()).unwrap()
}

fn names(cookies: &[Cookie]) -> Vec<&str> {
    cookies.iter().map(|c| c.name.as_str()).collect()
}

fn configure(core: &Core, send: bool, store: bool) {
    core.send(Command::Configure {
        history: HISTORY_LIMITS,
        responses: RESPONSE_LIMITS,
        proxy: apifast_domain::ProxyChoice::System,
        trust: apifast_domain::TlsTrust::default(),
        cookies: (send, store),
        timeout_ms: 30_000,
        reopen_last: true,
    });
}

#[test]
fn a_cookie_a_response_sets_is_sent_sealed_on_disk_and_comes_back() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let core = core_in(data.path());
    let root = open(&core, dir.path());

    assert_eq!(send(&core, &root, "login.yml"), "-");
    let snapshot = wait(&core, "the cookie", |s| !s.cookies.is_empty());
    let sid = &snapshot.cookies[0];
    assert_eq!(
        (sid.name.as_str(), sid.domain.as_str()),
        ("sid", "127.0.0.1")
    );
    assert!(sid.http_only && sid.same_site == Some(SameSite::Lax));
    assert_eq!(send(&core, &root, "me.yml"), format!("sid={SESSION}"));

    let jars: Vec<_> = std::fs::read_dir(data.path().join("cookies"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(jars.len(), 1);
    let text = std::fs::read_to_string(jars[0].path()).unwrap();
    assert!(!text.contains(SESSION), "{text}");

    drop(core);
    let core = core_in(data.path());
    open(&core, dir.path());
    let snapshot = wait(&core, "the jar from disk", |s| !s.cookies.is_empty());
    assert_eq!(names(&snapshot.cookies), ["sid"]);
    assert_eq!(send(&core, &root, "me.yml"), format!("sid={SESSION}"));
}

#[test]
fn the_editor_adds_changes_and_deletes_cookies() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let core = core_in(data.path());
    let root = open(&core, dir.path());
    send(&core, &root, "login.yml");
    let snapshot = wait(&core, "the cookie", |s| s.cookies.len() == 1);

    let mut draft = CookieDraft::new("127.0.0.1");
    draft.name = "theme".into();
    draft.value = "dark".into();
    core.send(Command::Cookies(CookieCommand::Set {
        replaces: None,
        cookie: Box::new(draft.to_cookie(1).unwrap()),
    }));
    // Renaming a cookie replaces it.
    let mut renamed = CookieDraft::of(&snapshot.cookies[0]);
    renamed.name = "session".into();
    core.send(Command::Cookies(CookieCommand::Set {
        replaces: renamed.replaces.clone(),
        cookie: Box::new(renamed.to_cookie(1).unwrap()),
    }));
    let snapshot = wait(&core, "the edits", |s| {
        names(&s.cookies) == ["session", "theme"]
    });
    // Same path: the older first, and `theme` was made at 1 ms.
    assert_eq!(
        send(&core, &root, "me.yml"),
        format!("theme=dark; session={SESSION}")
    );

    let theme = snapshot.cookies.iter().find(|c| c.name == "theme").unwrap();
    core.send(Command::Cookies(CookieCommand::Remove(CookieSlot::of(
        theme,
    ))));
    wait(&core, "one removed", |s| names(&s.cookies) == ["session"]);
    core.send(Command::Cookies(CookieCommand::Clear));
    wait(&core, "all removed", |s| s.cookies.is_empty());
    assert_eq!(send(&core, &root, "me.yml"), "-");
}

#[test]
fn the_settings_stop_sending_and_storing_cookies() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let core = core_in(data.path());
    let root = open(&core, dir.path());
    configure(&core, true, false);
    send(&core, &root, "login.yml");
    assert_eq!(send(&core, &root, "me.yml"), "-");
    assert!(core.snapshot().cookies.is_empty());

    configure(&core, false, true);
    send(&core, &root, "login.yml");
    wait(&core, "the cookie", |s| s.cookies.len() == 1);
    assert_eq!(send(&core, &root, "me.yml"), "-", "not sent");
    configure(&core, true, true);
    assert_eq!(send(&core, &root, "me.yml"), format!("sid={SESSION}"));
}

#[test]
fn the_cookie_form_checks_its_fields() {
    let mut draft = CookieDraft::new(".API.test");
    draft.name = "a".into();
    draft.expires = "2030-01-31 12:00:00".into();
    let cookie = draft.to_cookie(7).unwrap();
    assert_eq!(cookie.domain, "api.test");
    assert_eq!(cookie.created_ms, 7);
    assert_eq!(
        cookie.expires_ms,
        apifast_domain::parse_expiry("2030-01-31 12:00:00")
    );
    // A changed cookie keeps when it was made.
    let edited = CookieDraft::of(&cookie);
    assert_eq!(edited.to_cookie(99).unwrap().created_ms, 7);

    let error = |change: fn(&mut CookieDraft)| {
        let mut bad = draft.clone();
        change(&mut bad);
        bad.to_cookie(0).unwrap_err()
    };
    assert_eq!(error(|d| d.name = String::new()), DraftError::Name);
    assert_eq!(error(|d| d.name = "a b".into()), DraftError::Name);
    assert_eq!(error(|d| d.domain = " ".into()), DraftError::Domain);
    assert_eq!(error(|d| d.path = "x".into()), DraftError::Path);
    assert_eq!(error(|d| d.expires = "soon".into()), DraftError::Expires);
}

mod editor {
    use apifast_domain::{Cookie, CookieJar, CookieTarget};
    use apifast_gui::core::{CookieCommand, CookieSlot};
    use apifast_gui::organisms::CookiesWindow;
    use apifast_gui::strings::EN;
    use eframe::egui;
    use eframe::egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        window: CookiesWindow,
        cookies: Vec<Cookie>,
        commands: Vec<CookieCommand>,
    }

    fn harness(cookies: Vec<Cookie>) -> Harness<'static, State> {
        let mut window = CookiesWindow::default();
        window.open = true;
        Harness::builder()
            .with_size(egui::vec2(900.0, 700.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    let ctx = ui.ctx().clone();
                    let commands = state
                        .window
                        .show(&ctx, Some("Shop"), &state.cookies, 0, &EN);
                    state.commands.extend(commands);
                },
                State {
                    window,
                    cookies,
                    commands: Vec::new(),
                },
            )
    }

    fn jar() -> Vec<Cookie> {
        let mut jar = CookieJar::new();
        let target = CookieTarget::new("https", "api.test", "/");
        jar.receive("sid=abc; Secure", &target, 0);
        jar.cookies().to_vec()
    }

    #[test]
    fn a_cookie_is_listed_by_domain_and_deleted() {
        let mut harness = harness(jar());
        harness.run();
        assert!(harness.query_by_label("api.test").is_some());
        assert!(harness.query_by_label("sid").is_some());
        harness.get_by_label(EN.cookies_delete).click();
        harness.run();
        let slot = CookieSlot::of(&jar()[0]);
        assert_eq!(harness.state().commands, [CookieCommand::Remove(slot)]);
    }

    #[test]
    fn the_form_adds_a_cookie_and_says_what_is_wrong() {
        let mut harness = harness(Vec::new());
        harness.run();
        assert!(harness.query_by_label(EN.cookies_empty).is_some());
        harness.get_by_label(EN.cookies_add).click();
        harness.run();
        assert!(harness.query_by_label(EN.cookies_bad_name).is_some());
        for (field, text) in [(EN.cookies_name, "token"), (EN.cookies_domain, "api.test")] {
            harness
                .get_by_role_and_label(Role::TextInput, field)
                .focus();
            harness.run();
            harness
                .get_by_role_and_label(Role::TextInput, field)
                .type_text(text);
            harness.run();
        }
        assert!(harness.query_by_label(EN.cookies_bad_name).is_none());
        harness.get_by_label(EN.save).click();
        harness.run();
        let [CookieCommand::Set { replaces, cookie }] = harness.state().commands.as_slice() else {
            panic!("{:?}", harness.state().commands);
        };
        assert_eq!(*replaces, None);
        assert_eq!(
            (cookie.name.as_str(), cookie.domain.as_str()),
            ("token", "api.test")
        );
    }
}
