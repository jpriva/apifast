//! Suggestions while typing in the code editor: a list under the cursor,
//! moved through with the arrows and picked with Enter or Tab (or a click);
//! Esc closes it, and `Ctrl+Space` opens it where it would not open alone.
//!
//! While it is open its keys are taken before the editor sees them, so the
//! arrows do not also move the cursor.

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, Key, Modifiers, Order, RichText};

use crate::code::{Completion, CompletionItem};
use crate::theme;

/// How many suggestions the list shows.
const SHOWN: usize = 10;

/// Works out the suggestions of a text at a byte of it.
pub type Completer<'a> = &'a dyn Fn(&str, usize) -> Completion;

/// What the list keeps between frames.
#[derive(Debug, Clone, Default)]
pub(super) struct State {
    open: bool,
    selected: usize,
    /// The word replaced: from `start` to `cursor`, in bytes.
    start: usize,
    cursor: usize,
    items: Vec<CompletionItem>,
    /// A click on the list, put in on the next frame.
    clicked: Option<usize>,
}

/// Before the editor: the list's keys, and a pick put in `text`. Returns
/// whether `text` changed, and whether `Ctrl+Space` asked for the list.
pub(super) fn before(ui: &egui::Ui, id: egui::Id, text: &mut String) -> (bool, bool) {
    let key = id.with("completion");
    let mut state: State = ui.data(|d| d.get_temp(key)).unwrap_or_default();
    let mut focused = ui.memory(|m| m.has_focus(id));
    // egui lets go of the focus on Esc before any widget runs: with the list
    // open, Esc closes the list only.
    if state.open && !focused && ui.input(|i| i.key_pressed(Key::Escape)) {
        ui.memory_mut(|m| m.request_focus(id));
        focused = true;
    }
    let mut forced = false;
    let mut pick = state.clicked.take();
    if focused {
        ui.input_mut(|i| {
            forced = i.consume_key(Modifiers::CTRL, Key::Space);
            if !state.open || state.items.is_empty() {
                return;
            }
            let count = state.items.len().min(SHOWN);
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                state.selected = (state.selected + 1) % count;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                state.selected = state.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter)
                || i.consume_key(Modifiers::NONE, Key::Tab)
            {
                pick = Some(state.selected);
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                state.open = false;
            }
        });
    }
    let mut changed = false;
    if let Some(item) = pick.and_then(|index| state.items.get(index)).cloned()
        && state.start <= state.cursor
        && state.cursor <= text.len()
        && text.is_char_boundary(state.start)
        && text.is_char_boundary(state.cursor)
    {
        text.replace_range(state.start..state.cursor, &item.label);
        let at = text[..state.start + item.label.len()].chars().count();
        if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), id) {
            edit.cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
            edit.store(ui.ctx(), id);
        }
        ui.memory_mut(|m| m.request_focus(id));
        state.open = false;
        changed = true;
    }
    ui.data_mut(|d| d.insert_temp(key, state));
    (changed, forced)
}

/// After the editor: the suggestions at its cursor, and the list under it.
pub(super) fn after(
    ui: &egui::Ui,
    (id, output): (egui::Id, &egui::text_edit::TextEditOutput),
    text: &str,
    (completer, typed, forced): (Completer<'_>, bool, bool),
) {
    let key = id.with("completion");
    let mut state: State = ui.data(|d| d.get_temp(key)).unwrap_or_default();
    let Some(range) = output
        .cursor_range
        .filter(|_| output.response.response.has_focus())
    else {
        state.open = false;
        ui.data_mut(|d| d.insert_temp(key, state));
        return;
    };
    let chars = range.primary.index.0;
    let cursor = text
        .char_indices()
        .nth(chars)
        .map_or(text.len(), |(at, _)| at);
    if typed || forced {
        let completion = completer(text, cursor);
        let before = &text[..cursor];
        // Typing a name, or what is followed by one, opens it.
        let trigger = before
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '(' | '{' | ':' | ' '));
        state.open = !completion.items.is_empty() && (forced || (typed && trigger));
        state.start = completion.start;
        state.items = completion.items;
        state.selected = 0;
    } else if cursor != state.cursor {
        // The cursor went elsewhere.
        state.open = false;
    }
    state.cursor = cursor;
    if state.open {
        let at = output.galley.pos_from_cursor(CCursor::new(chars));
        let position = output.galley_pos + at.left_bottom().to_vec2();
        egui::Area::new(key)
            .order(Order::Foreground)
            .fixed_pos(position)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let muted = theme::palette(ui).text_muted;
                    for (index, item) in state.items.iter().take(SHOWN).enumerate() {
                        let row = ui.horizontal(|ui| {
                            let label = RichText::new(&item.label).monospace();
                            let row = ui.selectable_label(index == state.selected, label);
                            ui.label(RichText::new(&item.detail).color(muted).small());
                            row
                        });
                        // On press: by the release the editor lost its focus.
                        if row.inner.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                            state.clicked = Some(index);
                            ui.ctx().request_repaint();
                        }
                    }
                });
            });
    }
    ui.data_mut(|d| d.insert_temp(key, state));
}
