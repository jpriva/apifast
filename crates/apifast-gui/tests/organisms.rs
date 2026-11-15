//! Molecules and organisms on their own: what they show and the events they
//! return, through the accessibility tree.

use apifast_app::{CollectionTree, RequestPath, ResponseHead, TreeItem};
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
    let tree = tree();
    let mut events = Vec::new();
    let mut harness = Harness::new_ui_state(
        |ui, view: &mut TreeView| {
            if let Some(event) = view.show(ui, &tree, 1, None, &EN) {
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
        },
        size: 8,
        body: BodyHandle::memory("r1", "not\nhere"),
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
