//! The screen shown while no collection is open.

use std::path::PathBuf;
use std::sync::mpsc;

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
    /// The system's folder picker while it is open: it answers with the
    /// folder chosen, or nothing.
    picking: Option<mpsc::Receiver<Option<PathBuf>>>,
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
            let (clicked, browse) = ui
                .horizontal(|ui| {
                    // Centered: the row takes only what its buttons need.
                    let open = ui.add_enabled(filled, egui::Button::new(strings.open));
                    let browse = ui
                        .add_enabled(self.picking.is_none(), egui::Button::new(strings.browse))
                        .on_hover_text(strings.pick_collection);
                    (open.clicked(), browse.clicked())
                })
                .inner;
            if browse {
                self.picking = Some(pick_folder(ui.ctx().clone(), strings.pick_collection));
            }
            let picked = self.picked();
            if let Some(folder) = &picked {
                self.folder = folder.display().to_string();
            }
            if let Some(error) = error {
                ui.add_space(theme::space::M);
                ui.label(RichText::new(error).color(theme::palette(ui).danger));
            }
            picked.map(Command::OpenCollection).or_else(|| {
                ((clicked || entered) && filled)
                    .then(|| Command::OpenCollection(PathBuf::from(self.folder.trim())))
            })
        })
    }

    /// The folder chosen in the picker, once it answers.
    fn picked(&mut self) -> Option<PathBuf> {
        let answer = self.picking.as_ref()?.try_recv();
        match answer {
            Ok(folder) => {
                self.picking = None;
                folder
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.picking = None;
                None
            }
        }
    }
}

/// Opens the system's folder picker on a thread of its own, since it blocks
/// until closed; repaints when it answers.
fn pick_folder(ctx: egui::Context, title: &str) -> mpsc::Receiver<Option<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    let title = title.to_owned();
    std::thread::spawn(move || {
        let folder = rfd::FileDialog::new().set_title(title).pick_folder();
        let _ = sender.send(folder);
        ctx.request_repaint();
    });
    receiver
}
