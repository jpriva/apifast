//! The body editor colors code, formats it with a button or Alt+Shift+F,
//! says where it could not, and stops highlighting very large bodies.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{Body, HttpRequest, Method};
use apifast_gui::atoms::{VariableHint, VariableHints, code_layout};
use apifast_gui::code::{HIGHLIGHT_LIMIT, Language};
use apifast_gui::core::TabContext;
use apifast_gui::organisms::{RequestPanel, RequestTab};
use apifast_gui::strings::EN;
use apifast_gui::theme;
use eframe::egui::{self, FontId, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    request: HttpRequest,
    panel: RequestPanel,
}

fn harness(body: Body) -> Harness<'static, State> {
    let mut request = HttpRequest::new("Users", Method::Post, "{{host}}/users");
    request.body = body;
    let state = State {
        request,
        panel: RequestPanel {
            tab: RequestTab::Body,
            ..RequestPanel::default()
        },
    };
    let context = TabContext::default();
    let hints = VariableHints::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            move |ui, state: &mut State| {
                state
                    .panel
                    .show(ui, &mut state.request, &context, &hints, &EN);
            },
            state,
        );
    harness.run();
    harness
}

fn json(harness: &Harness<'_, State>) -> String {
    match &harness.state().request.body {
        Body::Json(text) => text.clone(),
        other => panic!("not JSON: {other:?}"),
    }
}

#[test]
fn the_format_button_formats_json() {
    let mut harness = harness(Body::Json(r#"{"id":{{id}},"tags":["a"]}"#.into()));
    harness.get_by_label(EN.format).click();
    harness.run();
    assert_eq!(
        json(&harness),
        "{\n  \"id\": {{id}},\n  \"tags\": [\n    \"a\"\n  ]\n}"
    );
}

#[test]
fn alt_shift_f_formats_without_typing_a_letter() {
    let mut harness = harness(Body::Json(r#"{"a":1}"#.into()));
    harness.get_by_role(Role::MultilineTextInput).focus();
    harness.run();
    let modifiers = egui::Modifiers::ALT | egui::Modifiers::SHIFT;
    // In one frame, as a window sends them: the key, and on many platforms
    // the letter as text too.
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::ModifiersChanged(modifiers));
    events.push(egui::Event::Key {
        key: egui::Key::F,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    events.push(egui::Event::Text("F".into()));
    harness.run();
    assert_eq!(json(&harness), "{\n  \"a\": 1\n}");
}

#[test]
fn broken_json_says_the_line_until_it_is_edited() {
    let mut harness = harness(Body::Json("{\n\"a\": [1,\n2}".into()));
    harness.get_by_label(EN.format).click();
    harness.run();
    let message = apifast_gui::strings::fill(EN.format_failed, &[("line", &3)]);
    harness.get_by_label(&message);
    assert_eq!(json(&harness), "{\n\"a\": [1,\n2}");

    harness.get_by_role(Role::MultilineTextInput).focus();
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::Text(" ".into()));
    harness.run();
    assert!(harness.query_by_label(&message).is_none());
}

#[test]
fn text_bodies_have_no_format_button() {
    let harness = harness(Body::Text("hi".into()));
    assert!(harness.query_by_label(EN.format).is_none());
}

#[test]
fn a_body_over_the_limit_is_edited_without_highlighting() {
    let big = format!("[{}]", "1,".repeat(HIGHLIGHT_LIMIT / 2));
    let harness = harness(Body::Json(big));
    harness.get_by_label(EN.highlight_off);
    harness.get_by_role(Role::MultilineTextInput);
}

#[test]
fn code_is_colored_and_variables_are_drawn_over_it() {
    let mut hints = VariableHints::new();
    hints.insert(
        "id".into(),
        VariableHint {
            value: "7".into(),
            origin: "Collection".into(),
        },
    );
    let text = r#"{"id": "{{id}}", "n": 1}"#;
    let palette = &theme::LIGHT;
    let job = code_layout(
        text,
        Language::Json,
        &hints,
        &FontId::monospace(12.0),
        palette,
        palette.text,
    );
    let color_of = |part: &str| {
        let start = text.find(part).unwrap();
        job.sections
            .iter()
            .find(|s| s.byte_range.start.0 <= start && start < s.byte_range.end.0)
            .unwrap()
            .format
            .color
    };
    assert_eq!(color_of(r#""id""#), palette.syntax_key);
    assert_eq!(color_of("{{id}}"), palette.variable);
    // The quote before the variable is still part of the string.
    assert_eq!(color_of(r#""{{"#), palette.syntax_string);
    assert_eq!(color_of("1"), palette.syntax_number);
    assert_eq!(color_of(":"), palette.text_muted);
    let covered: usize = job
        .sections
        .iter()
        .map(|s| s.byte_range.end.0 - s.byte_range.start.0)
        .sum();
    assert_eq!(covered, text.len());
}
