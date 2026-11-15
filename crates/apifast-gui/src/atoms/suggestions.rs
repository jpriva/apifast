//! Suggestions under a text field: a short list the user moves through with
//! the arrows and picks with Enter or a click; Esc hides it until the text
//! changes.

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, Key, Order};

/// How many suggestions are shown at most.
pub const MAX_SUGGESTIONS: usize = 8;

/// The candidates that complete `input`: those that start with it,
/// ignoring case, and are not already equal to it. In their own order, at
/// most [`MAX_SUGGESTIONS`].
pub fn completions<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let input = input.to_lowercase();
    candidates
        .into_iter()
        .filter(|c| {
            let lower = c.to_lowercase();
            lower.starts_with(&input) && lower != input
        })
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// What the list remembers between frames, per field.
#[derive(Debug, Clone, Default)]
struct State {
    selected: usize,
    /// The text the user dismissed the list for, with Esc.
    dismissed_for: Option<String>,
}

/// Shows `items` under `field` while it has the focus. `text` is the
/// field's text, to forget an Esc once it changes. Returns the index the
/// user picked; the caller puts it in the text and calls [`refocus`].
pub fn suggestions(
    ui: &egui::Ui,
    field: &egui::Response,
    text: &str,
    items: &[&str],
) -> Option<usize> {
    let id = field.id.with("suggestions");
    let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if state.dismissed_for.as_deref().is_some_and(|t| t != text) {
        state.dismissed_for = None;
    }
    // Enter makes a one-line field lose its focus; that is the pick.
    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let open = (field.has_focus() || entered) && !items.is_empty() && state.dismissed_for.is_none();
    if !open {
        ui.data_mut(|d| d.insert_temp(id, state));
        return None;
    }
    state.selected = state.selected.min(items.len() - 1);
    let mut picked = entered.then_some(state.selected);
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
            state.selected = (state.selected + 1) % items.len();
        }
        if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
            state.selected = state.selected.checked_sub(1).unwrap_or(items.len() - 1);
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
            state.dismissed_for = Some(text.to_owned());
        }
    });
    if state.dismissed_for.is_none() && picked.is_none() {
        egui::Area::new(id)
            .order(Order::Foreground)
            .fixed_pos(field.rect.left_bottom())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(field.rect.width());
                    for (index, item) in items.iter().enumerate() {
                        let row = ui.selectable_label(index == state.selected, *item);
                        // On press, not on release: by the release the field
                        // has lost its focus and the list is gone.
                        if row.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                            picked = Some(index);
                        }
                    }
                });
            });
    }
    if picked.is_some() {
        state.selected = 0;
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    picked
}

/// Gives `field` the focus back with the cursor at character `at`, after a
/// suggestion changed its text.
pub fn refocus(ui: &egui::Ui, field: &egui::Response, at: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        state.store(ui.ctx(), field.id);
    }
    field.request_focus();
}
