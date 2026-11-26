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
use apifast_gui::core::{Command, Core};
use apifast_gui::strings::EN;
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
        if harness.query_all_by_label(label).next().is_some() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for `{label}`"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens the collection in `folder`, as the system's folder picker would
/// (which a test cannot drive).
fn open_folder(harness: &mut Harness<'_, App>, folder: &std::path::Path) {
    harness
        .state()
        .core()
        .send(Command::OpenCollection(folder.to_path_buf()));
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

    // The app starts in its main view, which says how to get a collection;
    // then one is opened.
    harness.get_by_label(EN.empty_collection_title);
    open_folder(&mut harness, dir.path());
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
fn a_folder_that_is_not_a_collection_offers_to_create_one() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("my-api");
    std::fs::create_dir(&folder).unwrap();
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);

    open_folder(&mut harness, &folder);
    wait_for(&mut harness, EN.create_here);
    assert!(
        harness
            .query_by_label_contains("opencollection.yml")
            .is_some()
    );
    harness.get_by_label(EN.create_here).click();
    wait_for(&mut harness, "my-api");
    assert!(folder.join("opencollection.yml").is_file());
}

#[test]
fn a_new_collection_is_created_from_the_empty_view() {
    let data = tempfile::tempdir().unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);

    // The empty view's button; the sidebar has one too.
    harness.get_by_label(EN.empty_collection_title);
    harness
        .get_all_by_label(EN.new_collection)
        .next()
        .unwrap()
        .click();
    wait_for(&mut harness, EN.new_collection_title);
    // The name takes the focus.
    type_into_focus(&mut harness, "Billing API");
    harness.step();
    // The default folder is the default workspace's, in the data directory.
    harness.get_by_label_contains("default-workspace");
    harness.get_by_label(EN.create).click();
    wait_for(&mut harness, "Billing API");
    let created = data
        .path()
        .join("default-workspace/billing-api/opencollection.yml");
    let text = std::fs::read_to_string(created).unwrap();
    assert!(text.contains("Billing API"), "{text}");
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
    wait_for_row_to_be(harness, label, true);
}

/// Steps until the tree row `label` is there (`shown`) or gone.
fn wait_for_row_to_be(harness: &mut Harness<'_, App>, label: &str, shown: bool) {
    let start = Instant::now();
    loop {
        harness.step();
        if harness
            .query_by_role_and_label(Role::TreeItem, label)
            .is_some()
            == shown
        {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for the row `{label}` to be {}",
            if shown { "shown" } else { "gone" }
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Steps the interface until no node is labelled `label`, failing after
/// 10 seconds.
fn wait_for_gone(harness: &mut Harness<'_, App>, label: &str) {
    let start = Instant::now();
    loop {
        harness.step();
        if harness.query_all_by_label(label).next().is_none() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for `{label}` to go"
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
    open_folder(&mut harness, dir.path());
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
    // The core sees the deletion a moment later.
    wait_for_row_to_be(&mut harness, "Ping again", false);
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
    open_folder(&mut harness, dir);
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
    // The core writes the file, then says so: wait for that too, not a
    // fixed number of frames, which a slow machine may not fill.
    wait_for_gone(&mut harness, apifast_gui::strings::EN.unsaved);

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

#[test]
fn two_collections_and_a_second_workspace() {
    let port = server();
    let (shop, billing) = (collection(port), collection(port));
    std::fs::write(
        billing.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Billing\n",
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut harness = open_app(shop.path(), data.path());
    open_folder(&mut harness, billing.path());
    wait_for(&mut harness, "Billing");

    // Both collections are roots; each request opens in its own tab.
    harness.get_by_role_and_label(Role::TreeItem, "Shop API");
    harness.get_by_role_and_label(Role::TreeItem, "Billing");
    // No tab yet: every "Health" is a row, one per collection.
    let healths: Vec<_> = harness
        .get_all_by_label("Health")
        .map(|n| n.rect().center())
        .collect();
    assert_eq!(healths.len(), 2);
    for center in healths {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
        std::thread::sleep(Duration::from_millis(50));
        harness.step();
    }
    // Two tabs named alike: each shows its collection.
    wait_for(&mut harness, "Shop API / Health");
    wait_for(&mut harness, "Billing / Health");

    // A second workspace, from the selector.
    harness.get_by_label(EN.workspace).click();
    harness.run();
    harness.get_by_label(EN.new_workspace).click();
    wait_for(&mut harness, EN.new_workspace.trim_end_matches('…'));
    type_into_focus(&mut harness, "Team");
    harness.step();
    harness.get_by_label(EN.create).click();
    let start = Instant::now();
    while harness.query_all_by_label("Shop API").next().is_some() {
        harness.step();
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "still on the first workspace"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let snapshot = harness.state().core().snapshot();
    let workspace = snapshot.workspace.as_ref().unwrap();
    assert_eq!(workspace.name, "Team");
    // Next to the first one, in the data directory here.
    assert_eq!(
        workspace.folder,
        data.path().canonicalize().unwrap().join("team")
    );
    assert_eq!(snapshot.workspaces.len(), 2);
    // Empty: how to start, in the sidebar and in the middle.
    assert!(
        harness
            .query_all_by_label(EN.no_collection)
            .next()
            .is_some()
    );
}

#[test]
fn a_response_is_saved_as_an_example_of_its_request() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let file = dir.path().join("health.yml");
    let mut harness = open_app(dir.path(), data.path());
    harness.get_by_label("Environment").click();
    wait_for(&mut harness, "local");
    harness.get_by_label("local").click();
    harness.step();
    harness
        .get_by_role_and_label(Role::TreeItem, "Health")
        .click();
    wait_for(&mut harness, "Send");
    harness.get_by_label("Send").click();
    wait_for(&mut harness, "200 OK");

    harness.get_by_label(EN.save_as_example).click();
    wait_for(&mut harness, EN.unsaved);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("examples"));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    wait_until(&mut harness, "the save", || {
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("examples:")
    });
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("name: 200 OK"), "{text}");
    assert!(text.contains("url: \"{{host}}/health\""), "{text}");
    assert!(text.contains("type: json"), "{text}");
    assert!(
        text.contains(r#"{"ok":true}"#) || text.contains(r#"{\"ok\":true}"#),
        "{text}"
    );

    harness.get_by_label("Examples (1)").click();
    harness.run_steps(2);
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, EN.example_name)
            .value()
            .as_deref(),
        Some("200 OK")
    );
}

#[test]
fn a_pasted_curl_command_becomes_the_request() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let mut harness = open_app(dir.path(), data.path());
    harness
        .get_by_role_and_label(Role::TreeItem, "Health")
        .click();
    wait_for(&mut harness, "Send");
    harness
        .get_by_role_and_label(Role::TextInput, "URL")
        .focus();
    harness.step();
    harness.input_mut().events.push(egui::Event::Paste(
        "curl -k 'https://api.test/v1/users' \\\n  -H 'X-Trace: 1' \\\n  --data-raw '{\"a\":1}' \\\n  -H 'Content-Type: application/json'"
            .into(),
    ));
    wait_for(&mut harness, EN.unsaved);
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "URL")
            .value()
            .as_deref(),
        Some("https://api.test/v1/users")
    );
    assert_eq!(
        harness.get_by_label("Method").value().as_deref(),
        Some("POST")
    );
    harness.get_by_label("Headers (2)");
    // The name stays; what was not kept is said.
    harness.get_by_role_and_label(Role::TreeItem, "Health");
    wait_for(&mut harness, &format!("{} -k", EN.curl_ignored));
}

#[test]
fn the_tree_copies_a_request_as_curl() {
    let dir = collection(server());
    let data = tempfile::tempdir().unwrap();
    let mut harness = open_app(dir.path(), data.path());
    menu(&mut harness, "Health", EN.copy_as);
    harness.get_by_label("cURL").click();
    harness.run_steps(2);
    // No environment: `host` is missing, and the notice says so.
    let missing = apifast_gui::strings::fill(
        EN.copied_missing.split("{missing}").next().unwrap(),
        &[("language", &"cURL")],
    );
    let start = Instant::now();
    loop {
        harness.step();
        if harness
            .query_all_by_label_contains(&missing)
            .next()
            .is_some()
        {
            assert!(harness.query_all_by_label_contains("host").next().is_some());
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no copy notice");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_postman_export_is_imported_from_the_sidebar() {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let export = source.path().join("pets.json");
    std::fs::write(
        &export,
        r#"{"info": {"name": "Pet Store", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item": [{"name": "List pets", "request": {"method": "GET", "url": "https://pets.test/pets"}}]}"#,
    )
    .unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    harness.run_steps(2);
    // The sidebar's; the empty view has one too.
    harness
        .get_all_by_label(EN.import_collection_action)
        .next()
        .unwrap()
        .click();
    wait_for(&mut harness, EN.import_hint);
    harness
        .get_by_role_and_label(Role::TextInput, EN.import_collection_file)
        .focus();
    harness.step();
    type_into_focus(&mut harness, &export.display().to_string());
    harness.get_by_label(EN.import_review).click();
    wait_for(&mut harness, EN.import);
    harness.run_steps(2);
    harness.get_by_label(EN.import).click();
    wait_for(&mut harness, "Pet Store");
    wait_for_row(&mut harness, "List pets");
}

#[test]
fn the_system_language_shows_and_the_settings_change_it_at_once() {
    let data = tempfile::tempdir().unwrap();
    let es = apifast_gui::strings::language("es").unwrap();
    let app = App::new(Core::start_with(
        data.path().to_path_buf(),
        SystemKeyring::off(),
        || {},
    ))
    .with_system_language(Some(es));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    wait_for(&mut harness, es.strings.new_collection);

    // Ctrl+, opens the settings, in Spanish; English is picked by its name.
    let modifiers = egui::Modifiers::COMMAND;
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::ModifiersChanged(modifiers));
    events.push(egui::Event::Key {
        key: egui::Key::Comma,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    harness.run();
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, es.strings.language)
        .click();
    harness.run();
    harness.get_by_label("English").click();
    wait_for(&mut harness, EN.new_collection);
    harness.get_by_label(EN.settings_appearance);
    assert!(harness.query_by_label(es.strings.new_collection).is_none());
}
