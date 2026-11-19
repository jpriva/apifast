//! Molecules and organisms on their own: what they show and the events they
//! return, through the accessibility tree.

use apifast_app::{
    CollectionTree, RequestPath, ResponseHead, TimelineEvent, TimelineKind, TreeItem,
};
use apifast_domain::{Collection, Folder, Method};
use apifast_gui::atoms::VariableHints;
use apifast_gui::body::BodyHandle;
use apifast_gui::core::{Received, ResponseState};
use apifast_gui::molecules::EnvironmentSelector;
use apifast_gui::organisms::{ResponseView, TreeEvent, TreeView, UrlBar, UrlBarEvent};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn tree() -> CollectionTree {
    CollectionTree {
        collection: Collection::new("API"),
        items: vec![
            TreeItem::Folder {
                path: "users".into(),
                folder: Folder::new("Users"),
                children: vec![TreeItem::Request {
                    path: RequestPath::new("users/list.yml"),
                    name: "List users".into(),
                    method: Method::Get,
                    seq: None,
                }],
            },
            TreeItem::Request {
                path: RequestPath::new("login.yml"),
                name: "Login".into(),
                method: Method::Post,
                seq: None,
            },
            TreeItem::Other {
                path: RequestPath::new("query.yml"),
                name: "Query".into(),
                kind: "graphql".into(),
                seq: None,
            },
            TreeItem::Invalid {
                path: RequestPath::new("broken.yml"),
                message: "bad YAML".into(),
            },
        ],
    }
}

#[test]
fn the_tree_lists_every_item_and_opens_folders_on_click() {
    let collections = [apifast_gui::core::CollectionEntry::loaded("/c", tree())];
    let mut events = Vec::new();
    let mut harness = Harness::new_ui_state(
        |ui, view: &mut TreeView| {
            if let Some((_, event)) = view.show(ui, &collections, 1, (None, None), &EN) {
                events.push(event);
            }
        },
        TreeView::default(),
    );
    harness.run();
    for label in ["Users", "Login", "POST", "Query (graphql)", "broken.yml"] {
        harness.get_by_label(label);
    }
    assert!(harness.query_by_label("List users").is_none());

    harness.get_by_label("Users").click();
    harness.run();
    harness.get_by_label("List users").click();
    harness.run();
    drop(harness);
    assert_eq!(
        events,
        [TreeEvent::RequestSelected(RequestPath::new(
            "users/list.yml"
        ))]
    );
}

#[test]
fn the_url_bar_sends_or_cancels() {
    for (sending, button, expected) in [
        (false, "Send", UrlBarEvent::Send),
        (true, "Cancel", UrlBarEvent::Cancel),
    ] {
        let mut events = Vec::new();
        let (mut method, mut url) = (Method::Get, "{{host}}/users".to_owned());
        let mut harness = Harness::new_ui(|ui| {
            if let Some(event) =
                UrlBar::new(&mut method, &mut url, &VariableHints::new(), sending, &EN).show(ui)
            {
                events.push(event);
            }
        });
        harness.get_by_label(button).click();
        harness.run();
        drop(harness);
        assert_eq!(events, [expected]);
    }
}

#[test]
fn the_url_bar_edits_the_method_and_the_url() {
    let mut events = Vec::new();
    let (mut method, mut url) = (Method::Get, "{{host}}/users".to_owned());
    let mut harness = Harness::new_ui(|ui| {
        if let Some(event) =
            UrlBar::new(&mut method, &mut url, &VariableHints::new(), false, &EN).show(ui)
        {
            events.push(event);
        }
    });
    let field = harness.get_by_role(egui::accesskit::Role::TextInput);
    assert_eq!(field.value().as_deref(), Some("{{host}}/users"));
    field.focus();
    harness.run();
    harness.key_press(egui::Key::End);
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .type_text("/1");
    harness.run();

    // The selector is named for screen readers; its value is the method.
    let selector = harness.get_by_label("Method");
    assert_eq!(selector.value().as_deref(), Some("GET"));
    selector.click();
    harness.run();
    harness.get_by_label("DELETE").click();
    harness.run();
    drop(harness);
    assert_eq!((method, url.as_str()), (Method::Delete, "{{host}}/users/1"));
    assert!(
        events.iter().all(|e| *e == UrlBarEvent::Edited),
        "{events:?}"
    );
    assert!(events.len() >= 2, "{events:?}");
}

#[test]
fn the_environment_selector_returns_only_real_changes() {
    let environments = vec!["local".to_owned(), "staging".to_owned()];
    let mut choices = Vec::new();
    let mut harness = Harness::new_ui(|ui| {
        if let Some(choice) =
            EnvironmentSelector::new(&environments, Some("local"), "None", "Environment").show(ui)
        {
            choices.push(choice);
        }
    });
    // Open the list, pick the active entry (no change), then another.
    // The selector is named for screen readers; its value is the choice.
    assert_eq!(
        harness.get_by_label("Environment").value().as_deref(),
        Some("local")
    );
    harness.get_by_label("Environment").click();
    harness.run();
    harness.get_by_label("local").click();
    harness.run();
    harness.get_by_label("Environment").click();
    harness.run();
    harness.get_by_label("staging").click();
    harness.run();
    drop(harness);
    assert_eq!(choices, [Some("staging".to_owned())]);
}

#[test]
fn the_response_view_shows_each_state() {
    let received = ResponseState::Received(Received {
        head: ResponseHead {
            status: 404,
            version: "HTTP/1.1".into(),
            url: "http://localhost/x".into(),
            redirects: 0,
            headers: vec![("content-type".into(), "text/plain".into())],
            elapsed_ms: 12,
            timeline: vec![
                TimelineEvent {
                    at_us: 0,
                    kind: TimelineKind::Request {
                        method: "GET".into(),
                        url: "http://localhost/x".into(),
                    },
                },
                TimelineEvent {
                    at_us: 11_250,
                    kind: TimelineKind::Headers { status: 404 },
                },
            ],
        },
        size: 8,
        body: BodyHandle::memory("r1", "not\nhere"),
        checks: apifast_gui::core::Checks::default(),
    });
    let cases = [
        (ResponseState::Idle, EN.response_idle),
        (ResponseState::Sending, EN.sending),
        (ResponseState::Cancelled, EN.cancelled),
        (
            ResponseState::Failed("cannot connect".into()),
            "cannot connect",
        ),
        (received, "404 Not Found"),
    ];
    for (state, label) in cases {
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ResponseView| view.show(ui, &state, &EN),
            ResponseView::default(),
        );
        harness.run_steps(2);
        harness.get_by_label(label);
        if matches!(state, ResponseState::Received(_)) {
            for label in ["12 ms", "8 B", "not", "here"] {
                harness.get_by_label(label);
            }
            harness.get_by_label("Headers (1)").click();
            harness.run();
            harness.get_by_label("content-type");
            harness.get_by_label("text/plain");
        }
    }
}

#[test]
fn collection_settings_mark_an_override_that_is_not_an_ip_and_save_them() {
    use apifast_domain::DnsOverride;
    use apifast_gui::organisms::{CollectionSettings, CollectionSettingsWindow};
    struct State {
        window: CollectionSettingsWindow,
        saved: Option<(std::path::PathBuf, CollectionSettings)>,
    }
    let mut window = CollectionSettingsWindow::default();
    let settings = CollectionSettings {
        dns_overrides: vec![DnsOverride {
            host: "api.test".into(),
            address: "localhost".into(),
            enabled: true,
        }],
        ..CollectionSettings::default()
    };
    window.open_for("/shop".into(), "Shop".into(), settings.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let ctx = ui.ctx().clone();
                if let Some(saved) = state.window.show(&ctx, &EN) {
                    state.saved = Some(saved);
                }
            },
            State {
                window,
                saved: None,
            },
        );
    harness.run();
    assert!(harness.query_by_label(EN.dns_overrides).is_some());
    assert!(harness.query_by_label(EN.dns_not_ip).is_some());
    harness.get_by_label(EN.save).click();
    harness.run();
    let (root, saved) = harness.state().saved.clone().unwrap();
    assert_eq!(root, std::path::Path::new("/shop"));
    assert_eq!(saved.dns_overrides, settings.dns_overrides);
}

#[test]
fn pasting_a_curl_command_into_the_url_is_an_event_and_not_text() {
    let mut events = Vec::new();
    let (mut method, mut url) = (Method::Get, String::new());
    let command = "curl 'https://api.test/x' \\\n  -H 'a: b'";
    let mut harness = Harness::new_ui(|ui| {
        if let Some(event) =
            UrlBar::new(&mut method, &mut url, &VariableHints::new(), false, &EN).show(ui)
        {
            events.push(event);
        }
    });
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .focus();
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::Paste(command.into()));
    harness.run();
    // Other text is pasted as always.
    harness
        .input_mut()
        .events
        .push(egui::Event::Paste("http://h".into()));
    harness.run();
    drop(harness);
    assert_eq!(url, "http://h");
    assert_eq!(events[0], UrlBarEvent::PastedCurl(command.into()));
    assert_eq!(events[1..], [UrlBarEvent::Edited]);
}

#[test]
fn the_import_window_reviews_then_imports_only_what_was_reviewed() {
    use apifast_app::{ImportPlan, ImportWarning, PlannedEntry};
    use apifast_gui::core::ImportView;
    use apifast_gui::organisms::{ImportEvent, ImportWindow};
    use egui::accesskit::Role;
    use std::sync::Arc;

    struct State {
        window: ImportWindow,
        view: Option<ImportView>,
        events: Vec<ImportEvent>,
    }
    let mut window = ImportWindow::default();
    window.open_in(std::path::Path::new("/work"));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 800.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let ctx = ui.ctx().clone();
                if let Some(event) = state.window.show(&ctx, state.view.as_ref(), &EN) {
                    state.events.push(event);
                }
            },
            State {
                window,
                view: None,
                events: Vec::new(),
            },
        );
    harness.run();
    assert!(harness.query_by_label(EN.import_hint).is_some());
    harness
        .get_by_role_and_label(Role::TextInput, EN.import_collection_file)
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, EN.import_collection_file)
        .type_text("/tmp/shop.json");
    harness.run();
    harness.get_by_label(EN.import_review).click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [ImportEvent::Review {
            collection: "/tmp/shop.json".into(),
            environments: Vec::new(),
        }]
    );

    harness.state_mut().view = Some(ImportView {
        files: vec!["/tmp/shop.json".into()],
        plan: Ok(Arc::new(ImportPlan {
            collection: "My Shop".into(),
            entries: vec![PlannedEntry {
                path: "users".into(),
                name: "Users".into(),
                is_folder: true,
            }],
            environments: Vec::new(),
            warnings: vec![ImportWarning::new(
                "Users",
                "a `test` script is not imported",
            )],
        })),
    });
    harness.run();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("My Shop: 1 folders")
            .is_some()
    );
    assert!(harness.query_by_label_contains("a `test` script").is_some());
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, EN.import_folder)
            .value()
            .as_deref(),
        Some("my-shop")
    );
    harness.get_by_label(EN.import).click();
    harness.run();
    assert_eq!(
        harness.state().events.last(),
        Some(&ImportEvent::Import("/work/my-shop".into()))
    );
}

#[test]
fn the_reimport_window_proposes_the_source_and_applies_what_is_chosen() {
    use apifast_app::{ReimportAction, ReimportEntry, ReimportPlan};
    use apifast_gui::core::ReimportView;
    use apifast_gui::organisms::{ReimportEvent, ReimportWindow};
    use egui::accesskit::Role;
    use std::sync::Arc;

    struct State {
        window: ReimportWindow,
        view: Option<ReimportView>,
        events: Vec<ReimportEvent>,
    }
    let entry = |key: &str, action, selected| ReimportEntry {
        key: key.into(),
        name: key.into(),
        path: format!("{key}.yml"),
        action,
        selected,
        previous_hash: None,
    };
    let mut window = ReimportWindow::default();
    window.open();
    let view = ReimportView {
        root: "/work/pets".into(),
        previous_source: Some("/specs/pets.yaml".into()),
        file: None,
        plan: None,
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 800.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let ctx = ui.ctx().clone();
                if let Some(event) = state.window.show(&ctx, state.view.as_ref(), &EN) {
                    state.events.push(event);
                }
            },
            State {
                window,
                view: Some(view),
                events: Vec::new(),
            },
        );
    harness.run();
    assert!(harness.query_by_label(EN.reimport_hint).is_some());
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, EN.reimport_file)
            .value()
            .as_deref(),
        Some("/specs/pets.yaml"),
        "the file it came from"
    );
    harness.get_by_label(EN.import_review).click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [ReimportEvent::Review("/specs/pets.yaml".into())]
    );

    let plan = ReimportPlan {
        previous_source: "../specs/pets.yaml".into(),
        entries: vec![
            entry("same", ReimportAction::Unchanged, false),
            entry("changed", ReimportAction::Update, true),
            entry("mine", ReimportAction::Conflict, false),
        ],
        warnings: Vec::new(),
    };
    let view = harness.state().view.clone().unwrap();
    harness.state_mut().view = Some(ReimportView {
        file: Some("/specs/pets.yaml".into()),
        plan: Some(Ok(Arc::new(plan))),
        ..view
    });
    harness.run();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("0 new, 2 changed")
            .is_some()
    );
    assert!(
        harness.query_by_label_contains("same.yml").is_none(),
        "unchanged is not listed"
    );
    let changed = format!("changed.yml: {}", EN.reimport_changed);
    let mine = format!("mine.yml: {}", EN.reimport_conflict);
    assert!(harness.query_by_label(&changed).is_some());
    harness.get_by_label(&mine).click();
    harness.run();
    harness.get_by_label(EN.reimport_apply).click();
    harness.run();
    assert_eq!(
        harness.state().events.last(),
        Some(&ReimportEvent::Apply(vec!["changed".into(), "mine".into()])),
        "the default, and the one clicked"
    );
    assert!(!harness.state().window.open);
}
