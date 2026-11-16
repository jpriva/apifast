//! The whole interface over a real core: open a collection, pick an
//! environment, select a request, send it and see the status. Against a
//! local HTTP server; no external network and no GPU.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use apifast_gui::app::App;
use apifast_gui::core::Core;
use apifast_vault::SystemKeyring;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Answers every request with `200 OK` and a small JSON body.
fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let body = r#"{"ok":true}"#;
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

fn collection(port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: Shop API\n".to_owned()),
        ("users/folder.yml", "info:\n  name: Users\n  type: folder\n".to_owned()),
        ("users/list.yml", "info:\n  name: List users\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n".to_owned()),
        ("health.yml", "info:\n  name: Health\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/health\"\n".to_owned()),
        ("environments/local.yml", format!("name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:{port}\n")),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

/// Steps the interface until a node labelled `label` appears, failing after
/// 10 seconds. The core works on its own thread, so frames alone are not
/// enough: the test also waits.
fn wait_for(harness: &mut Harness<'_, App>, label: &str) {
    let start = Instant::now();
    loop {
        harness.step();
        if harness.query_by_label(label).is_some() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for `{label}`"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Focuses the folder field of the welcome screen and types `folder`.
fn type_folder(harness: &mut Harness<'_, App>, folder: &std::path::Path) {
    harness.get_by_role(Role::TextInput).focus();
    harness.step();
    harness
        .get_by_role(Role::TextInput)
        .type_text(folder.to_str().unwrap());
    harness.step();
}

/// The sidebar tab with the history.
const EN_HISTORY: &str = apifast_gui::strings::EN.sidebar_history;

#[test]
fn open_select_send_and_see_the_status() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);

    // Welcome screen: type the folder and open it.
    type_folder(&mut harness, dir.path());
    harness.get_by_label("Open").click();
    wait_for(&mut harness, "Shop API");

    // Folders start closed; their requests appear when opened.
    assert!(harness.query_by_label("List users").is_none());
    harness.get_by_label("Users").click();
    wait_for(&mut harness, "List users");

    // `host` comes from the `local` environment.
    harness.get_by_label("Environment").click();
    wait_for(&mut harness, "local");
    harness.get_by_label("local").click();
    harness.step();

    harness.get_by_label("Health").click();
    wait_for(&mut harness, "Send");
    harness.get_by_label("Send").click();
    wait_for(&mut harness, "200 OK");
    // JSON is shown pretty; raw shows it as received.
    wait_for(&mut harness, r#"  "ok": true"#);
    harness.get_by_label("Raw").click();
    wait_for(&mut harness, r#"{"ok":true}"#);
    harness.get_by_label("Headers (3)");

    // The send is in the history, in the sidebar.
    harness.get_by_label(EN_HISTORY).click();
    wait_for(&mut harness, "health.yml");
}

#[test]
fn a_folder_that_is_not_a_collection_shows_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);

    type_folder(&mut harness, dir.path());
    harness.key_press(egui::Key::Enter);
    let start = Instant::now();
    loop {
        harness.step();
        if harness
            .query_by_label_contains("opencollection.yml")
            .is_some()
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no error shown");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Still on the welcome screen, ready to try another folder.
    harness.get_by_label("Open");
}

/// Types into the field that has the focus, as the keyboard would.
fn type_into_focus(harness: &mut Harness<'_, App>, text: &str) {
    harness
        .input_mut()
        .events
        .push(egui::Event::Text(text.to_owned()));
}

/// Steps the interface until the tree shows a row labelled `label`.
fn wait_for_row(harness: &mut Harness<'_, App>, label: &str) {
    let start = Instant::now();
    loop {
        harness.step();
        if harness
            .query_by_role_and_label(Role::TreeItem, label)
            .is_some()
        {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for the row `{label}`"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Steps the interface until `done` holds, failing after 10 seconds.
fn wait_until(harness: &mut Harness<'_, App>, what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    loop {
        harness.step();
        if done() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens the context menu of `row` and picks `entry` (the last match: the
/// toolbar has some of the same labels).
fn menu(harness: &mut Harness<'_, App>, row: &str, entry: &str) {
    harness
        .get_by_role_and_label(Role::TreeItem, row)
        .click_secondary();
    harness.run_steps(2);
    harness.get_all_by_label(entry).last().unwrap().click();
    harness.run_steps(2);
}

#[test]
fn the_sidebar_creates_renames_and_deletes_files() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    type_folder(&mut harness, dir.path());
    harness.get_by_label("Open").click();
    wait_for(&mut harness, "Shop API");

    harness.get_by_label("New request").click();
    harness.run_steps(2);
    type_into_focus(&mut harness, "Ping");
    harness.step();
    harness.get_by_label("Create").click();
    wait_for_row(&mut harness, "Ping");
    let file = dir.path().join("ping.yml");
    assert!(file.exists());
    // The new request is open.
    harness.get_by_label("Send");

    menu(&mut harness, "Ping", "Rename");
    type_into_focus(&mut harness, " again");
    harness.step();
    harness.key_press(egui::Key::Enter);
    wait_for_row(&mut harness, "Ping again");
    assert!(!file.exists());
    assert!(dir.path().join("ping-again.yml").exists());

    menu(&mut harness, "Ping again", "Delete");
    harness.get_by_label("Delete").click();
    let gone = dir.path().join("ping-again.yml");
    wait_until(&mut harness, "the file to go", || !gone.exists());
    harness.run_steps(2);
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Ping again")
            .is_none()
    );
    // The deleted request was open: now nothing is.
    harness.get_by_label(apifast_gui::strings::EN.no_request);
}

/// A harness over a fresh app on `data`, with the collection in `dir` open.
fn open_app(dir: &std::path::Path, data: &std::path::Path) -> Harness<'static, App> {
    let app = App::new(Core::start_with(
        data.to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    type_folder(&mut harness, dir);
    harness.get_by_label("Open").click();
    wait_for(&mut harness, "Shop API");
    harness
}

/// Puts the cursor at the end of the URL field and types `text`.
fn type_in_url(harness: &mut Harness<'_, App>, text: &str) {
    harness
        .get_by_role_and_label(Role::TextInput, "URL")
        .focus();
    harness.step();
    harness.key_press(egui::Key::End);
    type_into_focus(harness, text);
    harness.step();
}

#[test]
fn edits_are_saved_with_ctrl_s_and_drafts_survive_a_restart() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let file = dir.path().join("health.yml");
    let mut harness = open_app(dir.path(), data.path());
    harness
        .get_by_role_and_label(Role::TreeItem, "Health")
        .click();
    wait_for(&mut harness, "Send");

    type_in_url(&mut harness, "/v2");
    wait_for(&mut harness, apifast_gui::strings::EN.unsaved);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("/v2"));

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    wait_until(&mut harness, "the save", || {
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("{{host}}/health/v2")
    });
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label(apifast_gui::strings::EN.unsaved)
            .is_none()
    );

    // An unsaved change, then the app closes.
    harness
        .get_by_role_and_label(Role::TextInput, "URL")
        .focus();
    harness.step();
    harness.key_press(egui::Key::End);
    type_into_focus(&mut harness, "?draft=1");
    wait_for(&mut harness, apifast_gui::strings::EN.unsaved);
    drop(harness);

    let mut harness = open_app(dir.path(), data.path());
    wait_for(&mut harness, apifast_gui::strings::EN.unsaved);
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "URL")
            .value()
            .as_deref(),
        Some("{{host}}/health/v2?draft=1")
    );
    assert!(!std::fs::read_to_string(&file).unwrap().contains("draft"));
}

#[test]
fn the_query_table_and_the_url_follow_each_other() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let mut harness = open_app(dir.path(), data.path());
    harness
        .get_by_role_and_label(Role::TreeItem, "Health")
        .click();
    wait_for(&mut harness, "Send");

    // Typing in the URL fills the table.
    type_in_url(&mut harness, "?page=2");
    harness.run_steps(2);
    let names: Vec<String> = harness
        .get_all_by_role_and_label(Role::TextInput, apifast_gui::strings::EN.name)
        .filter_map(|node| node.value())
        .collect();
    assert!(names.contains(&"page".to_owned()), "{names:?}");

    // Typing in the table's empty row extends the URL's query.
    harness
        .get_all_by_role_and_label(Role::TextInput, apifast_gui::strings::EN.name)
        .last()
        .unwrap()
        .focus();
    harness.step();
    type_into_focus(&mut harness, "q");
    harness.run_steps(2);
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "URL")
            .value()
            .as_deref(),
        Some("{{host}}/health?page=2&q")
    );
}
