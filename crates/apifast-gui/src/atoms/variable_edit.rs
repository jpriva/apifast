//! How a field asks to edit a `{{variable}}`: fields are everywhere (the
//! URL, tables, editors, windows), so instead of an event each one passes
//! up, the ask is left in egui's memory for the screen to take.
//!
//! - Ctrl+click on a variable asks at once ([`ask`]).
//! - The focused field notes the variable under its text cursor
//!   ([`note_at_cursor`]), so a shortcut can ask for it ([`at_cursor`]).

use eframe::egui;

fn asked_id() -> egui::Id {
    egui::Id::unique("apifast-variable-edit-asked")
}

fn cursor_id() -> egui::Id {
    egui::Id::unique("apifast-variable-edit-cursor")
}

/// Asks the screen to edit the variable `name`.
pub fn ask(ctx: &egui::Context, name: &str) {
    ctx.data_mut(|d| d.insert_temp(asked_id(), name.to_owned()));
}

/// The variable asked for, once: taking it clears it.
pub fn take(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(asked_id()))
}

/// Notes `name` as the variable under the text cursor of the focused
/// field this frame; `None` when the cursor is on none.
pub fn note_at_cursor(ctx: &egui::Context, name: Option<String>) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(cursor_id(), (frame, name)));
}

/// The variable under the text cursor of the focused field, as noted this
/// frame or the one before: shortcuts may be read before fields draw.
pub fn at_cursor(ctx: &egui::Context) -> Option<String> {
    let frame = ctx.cumulative_frame_nr();
    let noted = ctx.data(|d| d.get_temp::<(u64, Option<String>)>(cursor_id()));
    noted
        .filter(|(at, _)| frame.saturating_sub(*at) <= 1)
        .and_then(|(_, name)| name)
}

/// What a field does with a variable `name` found under the pointer or
/// the text cursor: asks on Ctrl+click, and notes it while focused.
pub(crate) fn follow(
    ui: &egui::Ui,
    response: &egui::Response,
    under_pointer: Option<&str>,
    under_cursor: Option<String>,
) {
    if response.clicked()
        && ui.input(|i| i.modifiers.command)
        && let Some(name) = under_pointer
    {
        ask(ui.ctx(), name);
    }
    if response.has_focus() {
        note_at_cursor(ui.ctx(), under_cursor);
    }
}
