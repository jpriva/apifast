//! Faster editing: bulk text for tables, and suggestions for header names
//! and variables. The text rules are pure functions, checked on their own;
//! the rest through real key and pointer events.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{Header, HttpRequest, Method, Param, ParamKind};
use apifast_gui::atoms::{
    TemplateField, TemplateFieldTexts, VariableHint, VariableHints, complete_variable, completions,
    variable_prefix,
};
use apifast_gui::core::TabContext;
use apifast_gui::organisms::{RequestPanel, RequestTab, from_bulk, to_bulk};
use apifast_gui::strings::EN;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn bulk_text_reads_back_as_the_rows_it_was_made_from() {
    let rows = vec![
        (true, "Accept".to_owned(), "application/json".to_owned()),
        (false, "X-Debug".to_owned(), "1".to_owned()),
        (true, "X-Empty".to_owned(), String::new()),
        (true, "X-Url".to_owned(), "http://a:1/b".to_owned()),
    ];
    let text = to_bulk(rows.clone());
    assert_eq!(
        text,
        "Accept: application/json\n//X-Debug: 1\nX-Empty\nX-Url: http://a:1/b"
    );
    assert_eq!(from_bulk(&text), rows);
}

#[test]
fn bulk_text_is_forgiving_with_spaces_and_blank_lines() {
    assert_eq!(
        from_bulk("  a :  1 \n\n// b:2\n//\nc"),
        [
            (true, "a".to_owned(), "1".to_owned()),
            (false, "b".to_owned(), "2".to_owned()),
            (true, "c".to_owned(), String::new()),
        ]
    );
}

#[test]
fn completions_start_with_the_input_ignoring_case() {
    let names = ["Accept", "Accept-Encoding", "Authorization", "accept"];
    assert_eq!(
        completions("acc", names),
        ["Accept", "Accept-Encoding", "accept"]
    );
    // What is already typed in full is not suggested again.
    assert_eq!(completions("accept", names), ["Accept-Encoding"]);
    assert_eq!(completions("zzz", names), Vec::<&str>::new());
}

#[test]
fn a_variable_is_being_typed_after_an_open_tag() {
    assert_eq!(variable_prefix("{{ho", 4), Some("ho"));
    assert_eq!(variable_prefix("x/{{ ho", 7), Some("ho"));
    assert_eq!(variable_prefix("{{", 2), Some(""));
    assert_eq!(variable_prefix("{{host}}/", 9), None);
    assert_eq!(variable_prefix("{{$rand", 7), None);
    assert_eq!(variable_prefix("\\{{ho", 5), None);
    assert_eq!(variable_prefix("ñ{{h", 4), Some("h"));
}

#[test]
fn completing_closes_the_tag_and_puts_the_cursor_after_it() {
    assert_eq!(complete_variable("{{ho", 4, "host"), ("{{host}}".into(), 8));
    assert_eq!(
        complete_variable("{{ho/users", 4, "host"),
        ("{{host}}/users".into(), 8)
    );
    // An existing `}}`, and the rest of a name, are reused.
    assert_eq!(
        complete_variable("{{ho}}/users", 4, "host"),
        ("{{host}}/users".into(), 8)
    );
    assert_eq!(
        complete_variable("{{hoXX}}/u", 4, "host"),
        ("{{host}}/u".into(), 8)
    );
}

// --- Through the interface --------------------------------------------------

struct Panel {
    request: HttpRequest,
    panel: RequestPanel,
}

fn panel(tab: RequestTab, request: HttpRequest) -> Harness<'static, Panel> {
    let hints = VariableHints::new();
    let context = TabContext::default();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, 500.0))
        .build_ui_state(
            move |ui, state: &mut Panel| {
                let _ = state
                    .panel
                    .show(ui, &mut state.request, &context, &hints, &EN);
            },
            Panel {
                request,
                panel: RequestPanel { tab },
            },
        );
    harness.run();
    harness
}

fn typing<S>(harness: &mut Harness<'_, S>, text: &str) {
    for c in text.chars() {
        harness
            .input_mut()
            .events
            .push(egui::Event::Text(c.to_string()));
        harness.run();
    }
}

fn request() -> HttpRequest {
    HttpRequest::new("r", Method::Get, "{{host}}/users")
}

fn focus_new_header_name(harness: &mut Harness<'_, Panel>) {
    harness
        .get_all_by_role_and_label(Role::TextInput, EN.name)
        .last()
        .unwrap()
        .focus();
    harness.run();
}

#[test]
fn header_names_are_suggested_and_picked_with_the_keyboard() {
    let mut harness = panel(RequestTab::Headers, request());
    focus_new_header_name(&mut harness);
    typing(&mut harness, "con");
    harness.get_by_label("Connection");
    harness.get_by_label("Content-Type");

    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().request.headers[0].name,
        "Content-Disposition"
    );
    // The list closed and the field kept the focus: typing goes on.
    assert!(harness.query_by_label("Connection").is_none());
    typing(&mut harness, "!");
    assert_eq!(
        harness.state().request.headers[0].name,
        "Content-Disposition!"
    );
}

#[test]
fn a_suggestion_can_be_clicked_and_esc_hides_them() {
    let mut harness = panel(RequestTab::Headers, request());
    focus_new_header_name(&mut harness);
    typing(&mut harness, "user");
    harness.get_by_label("User-Agent").click();
    harness.run();
    assert_eq!(harness.state().request.headers[0].name, "User-Agent");

    focus_new_header_name(&mut harness);
    typing(&mut harness, "acc");
    harness.get_by_label("Accept-Encoding");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Accept-Encoding").is_none());
    assert_eq!(harness.state().request.headers[1].name, "acc");
}

#[test]
fn bulk_edit_rewrites_the_rows_and_keeps_path_parameters() {
    let mut request = HttpRequest::new("r", Method::Get, "{{host}}/u/:id?a=1");
    request.params = vec![
        Param {
            name: "a".into(),
            value: "1".into(),
            kind: ParamKind::Query,
            enabled: true,
        },
        Param {
            name: "id".into(),
            value: "7".into(),
            kind: ParamKind::Path,
            enabled: true,
        },
    ];
    let mut harness = panel(RequestTab::Params, request);
    harness.get_by_label(EN.bulk_edit).click();
    harness.run();
    let editor = harness.get_by_role_and_label(Role::MultilineTextInput, EN.bulk_text);
    assert_eq!(editor.value().as_deref(), Some("a: 1"));
    editor.focus();
    harness.run();
    harness.key_press(egui::Key::End);
    // A new line comes from the Enter key, not from text.
    harness.key_press(egui::Key::Enter);
    harness.run();
    typing(&mut harness, "//b: 2");
    let params = &harness.state().request.params;
    let summary: Vec<_> = params
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str(), p.kind, p.enabled))
        .collect();
    assert_eq!(
        summary,
        [
            ("id", "7", ParamKind::Path, true),
            ("a", "1", ParamKind::Query, true),
            ("b", "2", ParamKind::Query, false),
        ]
    );

    // Back to the table, which shows the rows from the text.
    harness.get_by_label(EN.bulk_edit).click();
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::MultilineTextInput, EN.bulk_text)
            .is_none()
    );
}

#[test]
fn headers_typed_as_bulk_text_become_rows() {
    let mut harness = panel(RequestTab::Headers, request());
    harness.get_by_label(EN.bulk_edit).click();
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, EN.bulk_text)
        .focus();
    harness.run();
    typing(&mut harness, "Accept: */*");
    assert_eq!(
        harness.state().request.headers,
        [Header::new("Accept", "*/*")]
    );
}

#[test]
fn variables_are_suggested_after_an_open_tag() {
    let hints = VariableHints::from([
        (
            "host".to_owned(),
            VariableHint {
                value: "http://x".into(),
                origin: "Collection".into(),
            },
        ),
        (
            "token".to_owned(),
            VariableHint {
                value: "t".into(),
                origin: "Collection".into(),
            },
        ),
    ]);
    let texts = TemplateFieldTexts {
        placeholder: "URL",
        missing: "missing",
        generated: "generated",
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(500.0, 300.0))
        .build_ui_state(
            move |ui, text: &mut String| {
                TemplateField::new(text, &hints, texts)
                    .accessible_name("URL")
                    .show(ui);
            },
            String::new(),
        );
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "URL")
        .focus();
    harness.run();
    typing(&mut harness, "{{h");
    harness.get_by_label("host");
    assert!(harness.query_by_label("token").is_none());
    harness.key_press(egui::Key::Enter);
    harness.run();
    typing(&mut harness, "/users");
    assert_eq!(harness.state(), "{{host}}/users");
}
