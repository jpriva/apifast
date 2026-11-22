//! Importing a collection: choose a Postman export and its environments,
//! review the plan (what is created, and what is not kept) and the folder
//! it goes in, then import. Only the reviewed plan can be imported.

use crate::messages::Say as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, RichText};

use crate::atoms::CloseButton;
use crate::core::ImportView;
use crate::organisms::folder_name;
use crate::strings::Strings;
use crate::theme;

/// Width of the path fields, in points.
const FIELD_WIDTH: f32 = 380.0;
/// Height of the plan's list, in points.
const PLAN_HEIGHT: f32 = 220.0;

/// What the window asks of the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportEvent {
    /// Make the plan of importing these files.
    Review {
        /// The collection's export.
        collection: PathBuf,
        /// Its environments' exports.
        environments: Vec<PathBuf>,
    },
    /// Import the reviewed plan into this new folder.
    Import(PathBuf),
    /// Forget the import.
    Close,
}

/// Which file dialog is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Collection,
    BrunoFolder,
    Environments,
    Parent,
}

/// The window's state.
#[derive(Debug, Default)]
pub struct ImportWindow {
    /// Whether it is open.
    pub open: bool,
    collection: String,
    environments: Vec<String>,
    parent: String,
    folder: String,
    /// The folder was typed by the user: a new plan does not change it.
    folder_typed: bool,
    /// The plan the folder was proposed for.
    proposed_for: Option<String>,
    picking: Option<(Pick, Receiver<Vec<PathBuf>>)>,
}

impl ImportWindow {
    /// Opens the window, the new collection going in `parent` unless the
    /// user picks another folder.
    pub fn open_in(&mut self, parent: &Path) {
        self.open = true;
        if self.parent.is_empty() {
            self.parent = parent.display().to_string();
        }
    }

    /// The files of the window, as the core plans them.
    fn files(&self) -> Vec<PathBuf> {
        let mut files = vec![PathBuf::from(self.collection.trim())];
        files.extend(self.environments.iter().map(|e| PathBuf::from(e.trim())));
        files
    }

    /// Draws the window, if open, over the core's `view` of the import.
    /// Returns what it asks.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        view: Option<&ImportView>,
        strings: &Strings,
    ) -> Option<ImportEvent> {
        if !self.open {
            return None;
        }
        self.take_picked();
        self.propose_folder(view);
        let mut event = None;
        let mut open = true;
        egui::Window::new(strings.import_title)
            .id(egui::Id::unique("import-window"))
            .open(&mut open)
            .default_width(620.0)
            .collapsible(false)
            .show(ctx, |ui| {
                event = self.files_form(ui, strings);
                ui.separator();
                if let Some(chosen) = self.plan(ui, view, strings) {
                    event = Some(chosen);
                }
            });
        if !open {
            self.open = false;
            event = Some(ImportEvent::Close);
        }
        if matches!(event, Some(ImportEvent::Import(_))) {
            self.open = false;
        }
        event
    }

    /// The files and the folder, and the button that reviews them.
    fn files_form(&mut self, ui: &mut egui::Ui, strings: &Strings) -> Option<ImportEvent> {
        let picking = self.picking.is_some();
        let mut pick = None;
        egui::Grid::new("import-files")
            .num_columns(2)
            .spacing([theme::space::L, theme::space::S])
            .show(ui, |ui| {
                ui.label(strings.import_collection_file);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.collection)
                            .hint_text("shop.postman_collection.json")
                            .desired_width(FIELD_WIDTH),
                    )
                    .accessible_name(strings.import_collection_file);
                    if ui
                        .add_enabled(!picking, egui::Button::new(strings.choose))
                        .clicked()
                    {
                        pick = Some(Pick::Collection);
                    }
                    if ui
                        .add_enabled(!picking, egui::Button::new(strings.import_bruno_folder))
                        .on_hover_text(strings.import_bruno_folder_hint)
                        .clicked()
                    {
                        pick = Some(Pick::BrunoFolder);
                    }
                });
                ui.end_row();
                ui.label(strings.import_environments);
                ui.vertical(|ui| {
                    let mut remove = None;
                    for (i, file) in self.environments.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(file.as_str()).monospace());
                            if ui.add(CloseButton::new(strings.remove_file)).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        self.environments.remove(i);
                    }
                    if ui
                        .add_enabled(!picking, egui::Button::new(strings.import_add_environments))
                        .clicked()
                    {
                        pick = Some(Pick::Environments);
                    }
                });
                ui.end_row();
            });
        if let Some(pick) = pick {
            self.pick(ui.ctx(), pick, strings.all_files);
        }
        let ready = !self.collection.trim().is_empty();
        ui.add_enabled(ready, egui::Button::new(strings.import_review))
            .clicked()
            .then(|| ImportEvent::Review {
                collection: PathBuf::from(self.collection.trim()),
                environments: self.environments.iter().map(PathBuf::from).collect(),
            })
    }

    /// The plan the core made, the folder it goes in, and the button that
    /// imports it.
    fn plan(
        &mut self,
        ui: &mut egui::Ui,
        view: Option<&ImportView>,
        strings: &Strings,
    ) -> Option<ImportEvent> {
        let palette = theme::palette(ui);
        let Some(view) = view else {
            ui.label(RichText::new(strings.import_hint).color(palette.text_muted));
            return None;
        };
        let plan = match &view.plan {
            Ok(plan) => plan,
            Err(why) => {
                ui.label(RichText::new(why.as_str()).color(palette.danger));
                return None;
            }
        };
        let (folders, requests) = plan.counts();
        ui.label(
            RichText::new(format!(
                "{}: {}, {}, {}",
                plan.collection,
                strings.count(&strings.import_folders, folders, &[]),
                strings.count(&strings.import_requests, requests, &[]),
                strings.count(
                    &strings.import_environments_count,
                    plan.environments.len(),
                    &[]
                ),
            ))
            .strong(),
        );
        egui::ScrollArea::vertical()
            .id_salt("import-plan")
            .max_height(PLAN_HEIGHT)
            .show(ui, |ui| {
                for entry in &plan.entries {
                    let depth = entry.path.matches('/').count();
                    let mark = if entry.is_folder { "+ " } else { "  " };
                    let line = format!("{}{mark}{}", "    ".repeat(depth), entry.path);
                    ui.label(RichText::new(line).monospace().small());
                }
                for environment in &plan.environments {
                    let line = format!("  environments/{environment}");
                    ui.label(RichText::new(line).monospace().small());
                }
            });
        if !plan.warnings.is_empty() {
            ui.add_space(theme::space::S);
            let title = format!("{} ({})", strings.import_not_kept, plan.warnings.len());
            egui::CollapsingHeader::new(RichText::new(title).color(palette.warning))
                .id_salt("import-warnings")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("import-warnings-list")
                        .max_height(PLAN_HEIGHT / 2.0)
                        .show(ui, |ui| {
                            for warning in &plan.warnings {
                                let at = if warning.at.is_empty() {
                                    strings.import_collection
                                } else {
                                    warning.at.as_str()
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{at}: {}",
                                        warning.message.say(strings)
                                    ))
                                    .small(),
                                );
                            }
                        });
                });
        }
        ui.add_space(theme::space::S);
        let target = self.target_form(ui, strings);
        // Only what was reviewed can be imported.
        let reviewed = view.files == self.files();
        if !reviewed {
            ui.label(
                RichText::new(strings.import_review_again)
                    .color(palette.warning)
                    .small(),
            );
        }
        let ready = reviewed && target.is_some();
        let import = ui
            .add_enabled(
                ready,
                egui::Button::new(RichText::new(strings.import).strong()),
            )
            .clicked();
        import.then_some(()).and(target).map(ImportEvent::Import)
    }

    /// Where the collection goes: a folder in a parent. `None` until both
    /// are given.
    fn target_form(&mut self, ui: &mut egui::Ui, strings: &Strings) -> Option<PathBuf> {
        let picking = self.picking.is_some();
        let mut pick = false;
        egui::Grid::new("import-target")
            .num_columns(2)
            .spacing([theme::space::L, theme::space::S])
            .show(ui, |ui| {
                ui.label(strings.collection_parent);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.parent).desired_width(FIELD_WIDTH))
                        .accessible_name(strings.collection_parent);
                    pick = ui
                        .add_enabled(!picking, egui::Button::new(strings.choose))
                        .clicked();
                });
                ui.end_row();
                ui.label(strings.import_folder);
                if ui
                    .add(egui::TextEdit::singleline(&mut self.folder).desired_width(FIELD_WIDTH))
                    .accessible_name(strings.import_folder)
                    .changed()
                {
                    self.folder_typed = true;
                }
                ui.end_row();
            });
        if pick {
            self.pick(ui.ctx(), Pick::Parent, strings.all_files);
        }
        let (parent, folder) = (self.parent.trim(), self.folder.trim());
        (!parent.is_empty() && !folder.is_empty()).then(|| Path::new(parent).join(folder))
    }

    /// A folder after the plan's collection, unless the user typed one.
    fn propose_folder(&mut self, view: Option<&ImportView>) {
        let Some(Ok(plan)) = view.map(|v| &v.plan) else {
            return;
        };
        if self.folder_typed || self.proposed_for.as_deref() == Some(plan.collection.as_str()) {
            return;
        }
        self.folder = folder_name(&plan.collection);
        self.proposed_for = Some(plan.collection.clone());
    }

    /// Opens the system's dialog for `pick`, off the interface's thread.
    fn pick(&mut self, ctx: &egui::Context, pick: Pick, all_files: &str) {
        let all_files = all_files.to_owned();
        let (sender, receiver) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let json = || {
                rfd::FileDialog::new()
                    .add_filter("Postman, Insomnia, OpenAPI", &["json", "yaml", "yml"])
                    .add_filter(&all_files, &["*"])
            };
            let picked = match pick {
                Pick::Collection => json().pick_file().into_iter().collect(),
                Pick::Environments => json().pick_files().unwrap_or_default(),
                Pick::Parent | Pick::BrunoFolder => {
                    rfd::FileDialog::new().pick_folder().into_iter().collect()
                }
            };
            let _ = sender.send(picked);
            ctx.request_repaint();
        });
        self.picking = Some((pick, receiver));
    }

    fn take_picked(&mut self) {
        let Some((pick, receiver)) = &self.picking else {
            return;
        };
        let picked = match receiver.try_recv() {
            Ok(picked) => picked,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        let pick = *pick;
        self.picking = None;
        let text = |p: &PathBuf| p.display().to_string();
        match pick {
            Pick::Collection | Pick::BrunoFolder => {
                if let Some(file) = picked.first() {
                    self.collection = text(file);
                }
            }
            Pick::Environments => self.environments.extend(picked.iter().map(text)),
            Pick::Parent => {
                if let Some(folder) = picked.first() {
                    self.parent = text(folder);
                }
            }
        }
    }
}
