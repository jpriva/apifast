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

#[test]
fn open_select_send_and_see_the_status() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start(data.path().to_path_buf(), || {}));
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
    harness.get_by_label(r#"{"ok":true}"#);
    harness.get_by_label("Headers (3)");
}

#[test]
fn a_folder_that_is_not_a_collection_shows_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start(data.path().to_path_buf(), || {}));
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
