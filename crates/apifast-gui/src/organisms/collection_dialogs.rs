//! Creating and opening collections: the system's folder picker and the
//! "New collection" dialog.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;

use crate::strings::Strings;
use crate::theme;

/// Width of the text fields of the dialog, in points.
const FIELD_WIDTH: f32 = 360.0;

/// What the dialogs ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionRequest {
    /// Open the collection in this folder.
    Open(PathBuf),
    /// Create a collection named `name` in `folder`, which is created.
    Create {
        /// Its folder.
        folder: PathBuf,
        /// Its name.
        name: String,
    },
}

/// Which picker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picking {
    /// A collection to open.
    Collection,
    /// The folder a new collection goes in.
    Parent,
}

/// The "New collection" dialog's fields.
#[derive(Debug, Default)]
struct NewCollection {
    name: String,
    parent: String,
    /// Whether the name took the focus, which it does once.
    focused: bool,
}

/// The dialogs and the picker.
#[derive(Debug, Default)]
pub struct CollectionDialogs {
    /// The system's folder picker while it is open: it answers with the
    /// folder chosen, or nothing.
    picking: Option<(Picking, mpsc::Receiver<Option<PathBuf>>)>,
    /// A picker asked for, opened on the next frame.
    wanted: Option<Picking>,
    creating: Option<NewCollection>,
}

/// The folder name for a collection named `name`: lowercase letters,
/// digits and dashes.
pub fn folder_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "collection".into()
    } else {
        out
    }
}

impl CollectionDialogs {
    /// Opens the system's picker to choose a collection.
    pub fn open_collection(&mut self) {
        self.wanted = Some(Picking::Collection);
    }

    /// Opens the "New collection" dialog, its folder set to `parent`.
    pub fn new_collection(&mut self, parent: &Path) {
        self.creating = Some(NewCollection {
            name: String::new(),
            parent: parent.display().to_string(),
            focused: false,
        });
    }

    /// Whether the dialog is open.
    pub fn is_creating(&self) -> bool {
        self.creating.is_some()
    }

    /// Draws the dialog, if open, and checks the picker. Returns what the
    /// user asked for.
    pub fn show(&mut self, ctx: &egui::Context, strings: &Strings) -> Option<CollectionRequest> {
        if let Some(kind) = self.wanted.take()
            && self.picking.is_none()
        {
            let title = match kind {
                Picking::Collection => strings.pick_collection,
                Picking::Parent => strings.pick_parent,
            };
            self.picking = Some((kind, pick_folder(ctx.clone(), title)));
        }
        if let Some((kind, folder)) = self.picked() {
            match (kind, &mut self.creating) {
                (Picking::Parent, Some(dialog)) => dialog.parent = folder.display().to_string(),
                (Picking::Collection, _) => return Some(CollectionRequest::Open(folder)),
                (Picking::Parent, None) => {}
            }
        }
        let dialog = self.creating.as_mut()?;
        let mut browse = false;
        let mut done = None;
        let modal = egui::Modal::new(egui::Id::unique("new-collection")).show(ctx, |ui| {
            ui.set_max_width(FIELD_WIDTH + 120.0);
            ui.heading(strings.new_collection_title);
            ui.add_space(theme::space::M);
            ui.label(strings.collection_name);
            let name = ui
                .add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .hint_text(strings.new_collection_hint)
                        .desired_width(FIELD_WIDTH),
                )
                .accessible_name(strings.collection_name);
            // Asked until it has it: a modal's first frame only sizes it.
            if name.has_focus() {
                dialog.focused = true;
            } else if !dialog.focused {
                name.request_focus();
            }
            let entered = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(theme::space::S);
            ui.label(strings.collection_parent);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut dialog.parent).desired_width(FIELD_WIDTH))
                    .accessible_name(strings.collection_parent);
                browse = ui.button(strings.browse).clicked();
            });
            let ready = !dialog.name.trim().is_empty() && !dialog.parent.trim().is_empty();
            let folder = PathBuf::from(dialog.parent.trim()).join(folder_name(&dialog.name));
            if ready {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}",
                        strings.collection_goes_in,
                        folder.display()
                    ))
                    .color(theme::palette(ui).text_muted)
                    .small(),
                );
            }
            ui.add_space(theme::space::M);
            ui.horizontal(|ui| {
                let create = ui.add_enabled(ready, egui::Button::new(strings.create));
                if ready && (create.clicked() || entered) {
                    done = Some(Some(CollectionRequest::Create {
                        folder: folder.clone(),
                        name: dialog.name.trim().to_owned(),
                    }));
                }
                if ui.button(strings.cancel).clicked() {
                    done = Some(None);
                }
            });
        });
        if browse {
            self.wanted = Some(Picking::Parent);
        }
        if modal.should_close() && self.picking.is_none() {
            done.get_or_insert(None);
        }
        let request = done?;
        self.creating = None;
        request
    }

    /// The folder chosen in the picker, once it answers.
    fn picked(&mut self) -> Option<(Picking, PathBuf)> {
        let (kind, receiver) = self.picking.as_ref()?;
        let kind = *kind;
        match receiver.try_recv() {
            Ok(folder) => {
                self.picking = None;
                folder.map(|f| (kind, f))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_plain() {
        assert_eq!(folder_name("Shop API"), "shop-api");
        assert_eq!(folder_name("  Pagos / v2!  "), "pagos-v2");
        assert_eq!(folder_name("Ñandú"), "ñandú");
        assert_eq!(folder_name("!!!"), "collection");
    }
}
