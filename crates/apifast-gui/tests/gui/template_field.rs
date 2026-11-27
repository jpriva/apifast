//! The template field: how text is split for highlighting, and what the
//! hover shows over a variable.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_gui::atoms::{
    Span, SpanKind, TemplateField, TemplateFieldTexts, VariableHint, VariableHints, spans,
};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const TEXTS: TemplateFieldTexts<'static> = TemplateFieldTexts {
    placeholder: "URL",
    missing: "not defined",
    generated: "made when sending",
    edit: "edit",
};

fn hints() -> VariableHints {
    VariableHints::from([(
        "host".to_owned(),
        VariableHint {
            value: "http://localhost:8080".into(),
            origin: "Environment local".into(),
        },
    )])
}

fn kinds(text: &str) -> Vec<(&str, SpanKind)> {
    spans(text, &hints())
        .into_iter()
        .map(|Span { range, kind, .. }| (&text[range], kind))
        .collect()
}

#[test]
fn variables_are_defined_missing_generated_or_invalid() {
    assert_eq!(
        kinds("{{host}}/users/{{id}}?t={{$timestamp}}&q={{ oops"),
        [
            ("{{host}}", SpanKind::Defined),
            ("/users/", SpanKind::Text),
            ("{{id}}", SpanKind::Missing),
            ("?t=", SpanKind::Text),
            ("{{$timestamp}}", SpanKind::Generated),
            ("&q=", SpanKind::Text),
            ("{{ oops", SpanKind::Invalid),
        ]
    );
    // An escaped tag is text, and multibyte text keeps its byte ranges.
    assert_eq!(
        kinds("ñ\\{{host}}"),
        [
            ("ñ", SpanKind::Text),
            ("\\{{", SpanKind::Text),
            ("host}}", SpanKind::Text)
        ]
    );
}

#[test]
fn spans_name_their_variable() {
    let found = spans("{{ host }}", &hints());
    assert_eq!(found[0].variable.as_deref(), Some("host"));
    assert_eq!(found[0].kind, SpanKind::Defined);
}

/// The field's text, and the measures needed to point at a character.
struct State {
    text: String,
    char_width: f32,
    margin: f32,
}

/// A harness with the field over `text`, and tooltips that show at once.
fn field(text: &str) -> Harness<'static, State> {
    let hints = hints();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(500.0, 200.0))
        .build_ui_state(
            move |ui, state: &mut State| {
                // Tooltips read the context's style, not this `ui`'s.
                ui.ctx().all_styles_mut(|style| {
                    style.interaction.tooltip_delay = 0.0;
                    style.interaction.show_tooltips_only_when_still = false;
                });
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                state.char_width = ui.fonts_mut(|f| f.glyph_width(&font, 'x'));
                state.margin = ui.spacing().button_padding.x;
                TemplateField::new(&mut state.text, &hints, TEXTS).show(ui);
            },
            State {
                text: text.to_owned(),
                char_width: 0.0,
                margin: 0.0,
            },
        );
    harness.run();
    harness
}

/// Points at the `n`th character of the field's text.
fn hover_char(harness: &mut Harness<'_, State>, n: usize) {
    let rect = harness.get_by_role(egui::accesskit::Role::TextInput).rect();
    let State {
        char_width, margin, ..
    } = *harness.state();
    #[expect(clippy::cast_precision_loss, reason = "a small character count")]
    let x = rect.left() + margin + char_width * (n as f32 + 0.5);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(x, rect.center().y)));
    harness.run_steps(3);
}

#[test]
fn hovering_a_variable_shows_its_value_and_origin() {
    let mut harness = field("{{host}}/users/{{id}}");
    hover_char(&mut harness, 3);
    harness.get_by_label("http://localhost:8080");
    harness.get_by_label("Environment local");

    hover_char(&mut harness, 17);
    harness.get_by_label("not defined");
    assert!(harness.query_by_label("Environment local").is_none());
}

#[test]
fn hovering_plain_text_shows_nothing() {
    let mut harness = field("{{host}}/users/{{id}}");
    hover_char(&mut harness, 10);
    assert!(harness.query_by_label("http://localhost:8080").is_none());
    assert!(harness.query_by_label("not defined").is_none());
}

/// Clicks the `n`th character of the field's text, with `modifiers` held.
fn click_char(harness: &mut Harness<'_, State>, n: usize, modifiers: egui::Modifiers) {
    hover_char(harness, n);
    let rect = harness.get_by_role(egui::accesskit::Role::TextInput).rect();
    let State {
        char_width, margin, ..
    } = *harness.state();
    #[expect(clippy::cast_precision_loss, reason = "a small character count")]
    let pos = egui::pos2(
        rect.left() + margin + char_width * (n as f32 + 0.5),
        rect.center().y,
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
        harness.step();
    }
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run_steps(2);
}

#[test]
fn ctrl_click_on_a_variable_asks_to_edit_it_and_the_cursor_notes_it() {
    use apifast_gui::atoms::variable_edit;
    let mut harness = field("{{host}}/users/{{id}}");
    hover_char(&mut harness, 3);
    harness.get_by_label(TEXTS.edit);

    // A plain click only places the text cursor, and the field notes the
    // variable under it for the shortcut.
    click_char(&mut harness, 17, egui::Modifiers::NONE);
    assert_eq!(variable_edit::take(&harness.ctx), None);
    assert_eq!(
        variable_edit::at_cursor(&harness.ctx).as_deref(),
        Some("id")
    );

    // Ctrl+click asks, for a variable defined or not.
    click_char(&mut harness, 3, egui::Modifiers::COMMAND);
    assert_eq!(variable_edit::take(&harness.ctx).as_deref(), Some("host"));
    assert_eq!(variable_edit::take(&harness.ctx), None, "taken once");
    click_char(&mut harness, 10, egui::Modifiers::COMMAND);
    assert_eq!(variable_edit::take(&harness.ctx), None, "plain text");
}
