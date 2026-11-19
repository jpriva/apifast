//! Importing again into an imported collection (ADR 0010): choose the
//! source's new version, review what happens to each request and choose,
//! then apply. Only the reviewed plan can be applied.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use apifast_app::{ReimportAction, ReimportPlan};
use eframe::egui::{self, RichText};

use crate::core::ReimportView;
use crate::strings::Strings;
use crate::theme;

/// Width of the file field, in points.
const FIELD_WIDTH: f32 = 380.0;
/// Height of the plan's list, in points.
const PLAN_HEIGHT: f32 = 260.0;

/// What the window asks of the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReimportEvent {
    /// Plan importing this file again.
    Review(PathBuf),
    /// Apply the reviewed plan, with the requests of these keys chosen.
    Apply(Vec<String>),
    /// Forget the reimport.
    Close,
}

/// The window's state.
#[derive(Debug, Default)]
pub struct ReimportWindow {
    /// Whether it is open.
    pub open: bool,
    file: String,
    /// The keys chosen, for the plan they were chosen in.
    chosen: HashSet<String>,
    chosen_for: Option<Arc<ReimportPlan>>,
    /// The collection the file was proposed for.
    proposed_for: Option<PathBuf>,
    picking: Option<Receiver<Option<PathBuf>>>,
}

impl ReimportWindow {
    /// Opens the window, for the collection the core is told about.
    pub fn open(&mut self) {
        self.open = true;
        self.file.clear();
        self.proposed_for = None;
        self.chosen_for = None;
    }

    /// Draws the window, if open, over the core's `view` of the reimport.
    /// Returns what it asks.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        view: Option<&ReimportView>,
        strings: &Strings,
    ) -> Option<ReimportEvent> {
        if !self.open {
            return None;
        }
        self.take_picked();
        if let Some(view) = view {
            self.propose(view);
        }
        let mut event = None;
        let mut open = true;
        egui::Window::new(strings.reimport_title)
            .id(egui::Id::unique("reimport-window"))
            .open(&mut open)
            .default_width(620.0)
            .collapsible(false)
            .show(ctx, |ui| {
                if let Some(view) = view {
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            strings.reimport_into,
                            view.root.display()
                        ))
                        .small(),
                    );
                }
                event = self.file_form(ui, strings);
                ui.separator();
                if let Some(chosen) = self.plan(ui, view, strings) {
                    event = Some(chosen);
                }
            });
        if !open {
            self.open = false;
            event = Some(ReimportEvent::Close);
        }
        if matches!(event, Some(ReimportEvent::Apply(_))) {
            self.open = false;
        }
        event
    }

    /// The source's file, and the button that reviews it.
    fn file_form(&mut self, ui: &mut egui::Ui, strings: &Strings) -> Option<ReimportEvent> {
        let picking = self.picking.is_some();
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label(strings.reimport_file);
            ui.add(egui::TextEdit::singleline(&mut self.file).desired_width(FIELD_WIDTH))
                .accessible_name(strings.reimport_file);
            if ui
                .add_enabled(!picking, egui::Button::new(strings.choose))
                .clicked()
            {
                pick = Some(false);
            }
            if ui
                .add_enabled(!picking, egui::Button::new(strings.import_bruno_folder))
                .on_hover_text(strings.import_bruno_folder_hint)
                .clicked()
            {
                pick = Some(true);
            }
        });
        if let Some(folder) = pick {
            self.pick(ui.ctx(), folder);
        }
        let ready = !self.file.trim().is_empty();
        ui.add_enabled(ready, egui::Button::new(strings.import_review))
            .clicked()
            .then(|| ReimportEvent::Review(PathBuf::from(self.file.trim())))
    }

    /// The plan the core made, a choice per request, and the button that
    /// applies it.
    fn plan(
        &mut self,
        ui: &mut egui::Ui,
        view: Option<&ReimportView>,
        strings: &Strings,
    ) -> Option<ReimportEvent> {
        let palette = theme::palette(ui);
        let plan = match view.and_then(|v| v.plan.as_ref()) {
            None => {
                ui.label(RichText::new(strings.reimport_hint).color(palette.text_muted));
                return None;
            }
            Some(Err(why)) => {
                ui.label(RichText::new(why.as_str()).color(palette.danger));
                return None;
            }
            Some(Ok(plan)) => Arc::clone(plan),
        };
        self.choose_defaults(&plan);
        let changed: Vec<_> = (plan.entries.iter())
            .filter(|e| e.action != ReimportAction::Unchanged)
            .collect();
        ui.label(
            RichText::new(format!(
                "{} {}, {} {}, {} {}",
                plan.count(ReimportAction::Create),
                strings.reimport_new,
                plan.count(ReimportAction::Update) + plan.count(ReimportAction::Conflict),
                strings.reimport_changed,
                plan.count(ReimportAction::Unchanged),
                strings.reimport_unchanged,
            ))
            .strong(),
        );
        if changed.is_empty() {
            ui.label(RichText::new(strings.reimport_nothing).color(palette.text_muted));
        }
        egui::ScrollArea::vertical()
            .id_salt("reimport-plan")
            .max_height(PLAN_HEIGHT)
            .show(ui, |ui| {
                for entry in changed {
                    let what = match entry.action {
                        ReimportAction::Create => strings.reimport_new,
                        ReimportAction::Update => strings.reimport_changed,
                        ReimportAction::Conflict => strings.reimport_conflict,
                        ReimportAction::Deleted => strings.reimport_deleted,
                        ReimportAction::Removed | ReimportAction::Unchanged => {
                            strings.reimport_removed
                        }
                    };
                    let mut on = self.chosen.contains(&entry.key);
                    let label = format!("{}: {what}", entry.path);
                    if ui.checkbox(&mut on, label).changed() {
                        if on {
                            self.chosen.insert(entry.key.clone());
                        } else {
                            self.chosen.remove(&entry.key);
                        }
                    }
                }
            });
        if !plan.warnings.is_empty() {
            let title = format!("{} ({})", strings.import_not_kept, plan.warnings.len());
            egui::CollapsingHeader::new(RichText::new(title).color(palette.warning))
                .id_salt("reimport-warnings")
                .show(ui, |ui| {
                    for warning in &plan.warnings {
                        let at = if warning.at.is_empty() {
                            strings.import_collection
                        } else {
                            warning.at.as_str()
                        };
                        ui.label(RichText::new(format!("{at}: {}", warning.message)).small());
                    }
                });
        }
        ui.add_space(theme::space::S);
        // Only what was reviewed can be applied.
        let reviewed = view
            .and_then(|v| v.file.as_ref())
            .is_some_and(|file| file.as_path() == std::path::Path::new(self.file.trim()));
        if !reviewed {
            ui.label(
                RichText::new(strings.import_review_again)
                    .color(palette.warning)
                    .small(),
            );
        }
        let apply = ui
            .add_enabled(
                reviewed,
                egui::Button::new(RichText::new(strings.reimport_apply).strong()),
            )
            .clicked();
        apply.then(|| {
            let mut keys: Vec<String> = self.chosen.iter().cloned().collect();
            keys.sort();
            ReimportEvent::Apply(keys)
        })
    }

    /// For a new plan, chooses what it selects by default.
    fn choose_defaults(&mut self, plan: &Arc<ReimportPlan>) {
        if self
            .chosen_for
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, plan))
        {
            return;
        }
        self.chosen = (plan.entries.iter())
            .filter(|e| e.selected)
            .map(|e| e.key.clone())
            .collect();
        self.chosen_for = Some(Arc::clone(plan));
    }

    /// The file last imported from, once per collection, if none is typed.
    fn propose(&mut self, view: &ReimportView) {
        if self.proposed_for.as_ref() == Some(&view.root) {
            return;
        }
        self.proposed_for = Some(view.root.clone());
        if let (true, Some(previous)) = (self.file.is_empty(), &view.previous_source) {
            self.file = previous.display().to_string();
        }
    }

    /// Opens the system's dialog, for a file or a folder, off the
    /// interface's thread.
    fn pick(&mut self, ctx: &egui::Context, folder: bool) {
        let (sender, receiver) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let picked = if folder {
                dialog.pick_folder()
            } else {
                dialog
                    .add_filter("Postman, Insomnia, OpenAPI", &["json", "yaml", "yml"])
                    .add_filter("All files", &["*"])
                    .pick_file()
            };
            let _ = sender.send(picked);
            ctx.request_repaint();
        });
        self.picking = Some(receiver);
    }

    fn take_picked(&mut self) {
        let Some(receiver) = &self.picking else {
            return;
        };
        let picked = match receiver.try_recv() {
            Ok(picked) => picked,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        self.picking = None;
        if let Some(file) = picked {
            self.file = file.display().to_string();
        }
    }
}
