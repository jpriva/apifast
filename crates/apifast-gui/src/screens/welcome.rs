//! The screen shown while no collection is open.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::core::Command;
use crate::layout;
use crate::strings::Strings;
use crate::theme;

/// Width of the folder field, in points.
const FIELD_WIDTH: f32 = 420.0;

/// The welcome screen and the folder being typed.
#[derive(Debug, Default)]
pub struct Welcome {
    folder: String,
}

impl Welcome {
    /// Draws the screen. `error` is why the last collection did not open.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        error: Option<&str>,
        strings: &Strings,
    ) -> Option<Command> {
        layout::centered(ui, |ui| {
            ui.label(
                RichText::new(strings.welcome_title)
                    .size(theme::text::HEADING)
                    .strong(),
            );
            ui.add_space(theme::space::S);
            ui.label(strings.welcome_hint);
            ui.add_space(theme::space::L);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.folder)
                    .hint_text(strings.folder_placeholder)
                    .desired_width(FIELD_WIDTH),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(theme::space::M);
            let filled = !self.folder.trim().is_empty();
            let clicked = ui
                .add_enabled(filled, egui::Button::new(strings.open))
                .clicked();
            if let Some(error) = error {
                ui.add_space(theme::space::M);
                ui.label(RichText::new(error).color(theme::palette(ui).danger));
            }
            ((clicked || entered) && filled)
                .then(|| Command::OpenCollection(PathBuf::from(self.folder.trim())))
        })
    }
}
