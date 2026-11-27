//! Molecules: small groups of atoms and egui widgets with one purpose.

use eframe::egui;

/// Picks the active environment, or none.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentSelector<'a> {
    environments: &'a [String],
    active: Option<&'a str>,
    none_label: &'a str,
    label: &'a str,
}

impl<'a> EnvironmentSelector<'a> {
    /// A selector over `environments` with `active` chosen. `none_label` is
    /// the entry for "no environment" and `label` the accessible name.
    pub fn new(
        environments: &'a [String],
        active: Option<&'a str>,
        none_label: &'a str,
        label: &'a str,
    ) -> Self {
        Self {
            environments,
            active,
            none_label,
            label,
        }
    }

    /// Draws the selector. Returns the new choice when the user changes it.
    pub fn show(self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let mut choice = None;
        // Told apart by its name: the global one sits next to it.
        let response = egui::ComboBox::from_id_salt(("environment", self.label))
            .selected_text(self.active.unwrap_or(self.none_label))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.active.is_none(), self.none_label)
                    .clicked()
                {
                    choice = Some(None);
                }
                for name in self.environments {
                    if ui
                        .selectable_label(self.active == Some(name.as_str()), name)
                        .clicked()
                    {
                        choice = Some(Some(name.clone()));
                    }
                }
            })
            .response;
        response.on_hover_text(self.label);
        // Picking the active entry again is not a change.
        choice.filter(|new| new.as_deref() != self.active)
    }
}

/// What an empty area says: what is missing, what to do, and buttons that
/// do it, each with its shortcut.
#[derive(Debug, Clone, Default)]
pub struct EmptyState {
    /// What is missing: "No request open".
    pub title: String,
    /// What to do about it.
    pub hint: String,
    /// The buttons, each with its shortcut if it has one.
    pub actions: Vec<(String, Option<String>)>,
}

/// Draws `state` centered in the space left. Returns the index of the
/// action pressed.
pub fn empty_state(ui: &mut egui::Ui, state: &EmptyState) -> Option<usize> {
    let palette = crate::theme::palette(ui);
    let mut pressed = None;
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 4.0);
        ui.label(egui::RichText::new(&state.title).heading());
        ui.add_space(crate::theme::space::S);
        ui.label(egui::RichText::new(&state.hint).color(palette.text_muted));
        ui.add_space(crate::theme::space::L);
        for (index, (label, shortcut)) in state.actions.iter().enumerate() {
            // Every label to the left, with or without a shortcut after it.
            let shortcut = shortcut.as_deref().unwrap_or_default();
            let button = egui::Button::new(label)
                .min_size(egui::vec2(220.0, 0.0))
                .right_text(egui::RichText::new(shortcut).color(palette.text_muted));
            if ui.add(button).clicked() {
                pressed = Some(index);
            }
        }
    });
    pressed
}

/// Shows `window` with apifast's own title bar instead of egui's: `title`
/// on the left and a drawn × on the right, named `close` for screen
/// readers, over a line. The window is dragged by any free space, as
/// egui's are. `open` turns false when it is closed, by the × or by egui.
/// Returns what `add` returned, while the window is shown.
pub fn window<R>(
    ctx: &egui::Context,
    window: egui::Window<'_>,
    (title, close): (&str, &str),
    open: &mut bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let mut shown = *open;
    let mut closed = false;
    let inner = window
        .title_bar(false)
        .open(&mut shown)
        .show(ctx, |ui| {
            closed = title_bar(ui, title, close);
            add(ui)
        })
        .and_then(|response| response.inner);
    *open = shown && !closed;
    inner
}

/// The title bar of a [`window`]. Returns whether its × was clicked.
fn title_bar(ui: &mut egui::Ui, title: &str, close: &str) -> bool {
    let clicked = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(crate::atoms::CloseButton::new(close)).clicked()
            })
            .inner
        })
        .inner;
    ui.separator();
    clicked
}
