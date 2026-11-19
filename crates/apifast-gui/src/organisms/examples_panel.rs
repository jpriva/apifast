//! The request's Examples tab: its saved examples, each with its name and
//! description to edit, what it answered, and a button to delete it. The
//! changes are edits of the request, saved with it.

use apifast_domain::Example;
use eframe::egui::{self, RichText};

use crate::atoms::{CloseButton, StatusBadge};
use crate::strings::Strings;
use crate::theme;

/// Lines of an example's body shown before scrolling.
const BODY_ROWS: usize = 12;

/// Draws the examples of a request. Returns whether they changed.
pub fn examples_panel(ui: &mut egui::Ui, examples: &mut Vec<Example>, strings: &Strings) -> bool {
    let palette = theme::palette(ui);
    if examples.is_empty() {
        ui.label(RichText::new(strings.examples_empty).color(palette.text_muted));
        return false;
    }
    let mut changed = false;
    let mut remove = None;
    for (index, example) in examples.iter_mut().enumerate() {
        let id = ui.make_persistent_id(("example", index));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| {
                if let Some(status) = example.response.as_ref().and_then(|r| r.status) {
                    ui.add(StatusBadge::new(status));
                }
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut example.name).desired_width(220.0))
                    .accessible_name(strings.example_name)
                    .changed();
                if ui.add(CloseButton::new(strings.example_delete)).clicked() {
                    remove = Some(index);
                }
            })
            .body(|ui| {
                let mut description = example.description.clone().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut description)
                            .hint_text(strings.example_description)
                            .desired_width(f32::INFINITY),
                    )
                    .accessible_name(strings.example_description)
                    .changed()
                {
                    example.description = (!description.trim().is_empty()).then_some(description);
                    changed = true;
                }
                details(ui, example, strings);
            });
    }
    if let Some(index) = remove {
        examples.remove(index);
        changed = true;
    }
    changed
}

/// What an example sent and answered, read-only.
fn details(ui: &mut egui::Ui, example: &Example, strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    if let Some(request) = &example.request {
        let line = format!("{} {}", request.method.as_str(), request.url);
        ui.label(RichText::new(line).monospace().color(muted));
    }
    let Some(response) = &example.response else {
        return;
    };
    for header in &response.headers {
        ui.label(
            RichText::new(format!("{}: {}", header.name, header.value))
                .monospace()
                .small()
                .color(muted),
        );
    }
    match &response.body {
        Some(body) => {
            let mut text = body.data.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_rows(BODY_ROWS.min(body.data.lines().count().max(1)))
                    .desired_width(f32::INFINITY),
            )
            .accessible_name(strings.example_body);
        }
        None => {
            ui.label(RichText::new(strings.example_no_body).color(muted));
        }
    }
}
