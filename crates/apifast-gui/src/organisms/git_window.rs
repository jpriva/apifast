//! The git window (docs/09, F10): the files of the collection that changed
//! since the last commit, what changed in the one picked field by field, a
//! commit of the files checked, and the branches.

use std::collections::BTreeSet;

use apifast_app::{FileChange, FileDiff};
use apifast_domain::FieldChange;
use eframe::egui::{self, RichText};

use crate::core::{GitCommand, GitView};
use crate::strings::Strings;
use crate::theme;

/// The window's state.
#[derive(Debug, Default)]
pub struct GitWindow {
    /// Whether it is open.
    pub open: bool,
    /// The files left out of the next commit; any other changed one goes.
    unchecked: BTreeSet<String>,
    /// The file whose diff shows.
    picked: Option<String>,
    /// The commit's message.
    message: String,
    /// The branch to switch to.
    branch: Option<String>,
}

fn letter(change: FileChange) -> &'static str {
    match change {
        FileChange::Added => "A",
        FileChange::Modified => "M",
        FileChange::Deleted => "D",
    }
}

impl GitWindow {
    /// Opens it, asking for the repository's status anew.
    pub fn open(&mut self) -> GitCommand {
        self.open = true;
        GitCommand::Refresh
    }

    /// Draws the window over `view`. Returns what the user asked for.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        view: Option<&GitView>,
        strings: &Strings,
    ) -> Vec<GitCommand> {
        if !self.open {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let mut open = self.open;
        egui::Window::new(strings.git_title)
            .id(egui::Id::unique("git-window"))
            .open(&mut open)
            .default_width(760.0)
            .collapsible(false)
            .show(ctx, |ui| {
                let Some(view) = view else {
                    let muted = theme::palette(ui).text_muted;
                    ui.label(RichText::new(strings.git_no_repository).color(muted));
                    return;
                };
                commands.extend(self.header(ui, view, strings));
                ui.separator();
                commands.extend(self.changes(ui, view, strings));
                ui.separator();
                commands.extend(self.commit_form(ui, view, strings));
            });
        self.open = open;
        commands
    }

    /// The branch, the switch to another, refresh, and what happened last.
    fn header(&mut self, ui: &mut egui::Ui, view: &GitView, strings: &Strings) -> Vec<GitCommand> {
        let mut commands = Vec::new();
        let palette = theme::palette(ui);
        ui.horizontal(|ui| {
            let branch = view.branch.as_deref().unwrap_or(strings.git_detached);
            ui.label(RichText::new(format!("{}: {branch}", strings.git_branch)).strong());
            let others: Vec<&String> = (view.branches.iter())
                .filter(|b| view.branch.as_ref() != Some(*b))
                .collect();
            if !others.is_empty() {
                let shown = self
                    .branch
                    .clone()
                    .unwrap_or_else(|| strings.git_branch_pick.to_owned());
                egui::ComboBox::from_id_salt("git-branch")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for branch in others {
                            ui.selectable_value(&mut self.branch, Some(branch.clone()), branch);
                        }
                    })
                    .response
                    .accessible_name(strings.git_branch);
                let pick = self
                    .branch
                    .clone()
                    .filter(|b| view.branch.as_ref() != Some(b));
                if ui
                    .add_enabled(pick.is_some(), egui::Button::new(strings.git_switch))
                    .clicked()
                    && let Some(branch) = pick
                {
                    self.branch = None;
                    commands.push(GitCommand::Switch(branch));
                }
            }
            if ui.button(strings.git_refresh).clicked() {
                commands.push(GitCommand::Refresh);
            }
        });
        if let Some(notice) = &view.notice {
            ui.label(RichText::new(notice).color(palette.success));
        }
        if let Some(error) = &view.error {
            ui.label(RichText::new(error).color(palette.danger));
        }
        commands
    }

    /// The files that changed, each with its box, and the diff of the one
    /// picked beside them.
    fn changes(&mut self, ui: &mut egui::Ui, view: &GitView, strings: &Strings) -> Vec<GitCommand> {
        let mut commands = Vec::new();
        let palette = theme::palette(ui);
        if view.changes.is_empty() {
            ui.label(RichText::new(strings.git_nothing_changed).color(palette.text_muted));
            return commands;
        }
        // Boxes of files that no longer changed are forgotten.
        self.unchecked
            .retain(|path| view.changes.iter().any(|c| c.path == *path));
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical()
                .id_salt("git-files")
                .max_height(320.0)
                .show(&mut columns[0], |ui| {
                    for change in &view.changes {
                        ui.horizontal(|ui| {
                            let mut checked = !self.unchecked.contains(&change.path);
                            let name = format!("{}: {}", strings.git_include, change.path);
                            if ui
                                .checkbox(&mut checked, "")
                                .accessible_name(name)
                                .changed()
                            {
                                if checked {
                                    self.unchecked.remove(&change.path);
                                } else {
                                    self.unchecked.insert(change.path.clone());
                                }
                            }
                            ui.label(RichText::new(letter(change.change)).monospace().strong());
                            let picked = self.picked.as_deref() == Some(change.path.as_str());
                            if ui.selectable_label(picked, &change.path).clicked() {
                                self.picked = Some(change.path.clone());
                                commands.push(GitCommand::Diff(change.path.clone()));
                            }
                        });
                    }
                });
            egui::ScrollArea::vertical()
                .id_salt("git-diff")
                .max_height(320.0)
                .show(&mut columns[1], |ui| {
                    let diff =
                        (view.diff.as_ref()).filter(|d| self.picked.as_ref() == Some(&d.path));
                    match diff {
                        Some(diff) => diff_view(ui, diff, strings),
                        None => {
                            ui.label(
                                RichText::new(strings.git_pick_file).color(palette.text_muted),
                            );
                        }
                    }
                });
        });
        commands
    }

    /// The message and the commit button, for the files checked.
    fn commit_form(
        &mut self,
        ui: &mut egui::Ui,
        view: &GitView,
        strings: &Strings,
    ) -> Vec<GitCommand> {
        let paths: Vec<String> = (view.changes.iter())
            .filter(|c| !self.unchecked.contains(&c.path))
            .map(|c| c.path.clone())
            .collect();
        ui.add(
            egui::TextEdit::multiline(&mut self.message)
                .hint_text(strings.git_message)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        let can = !paths.is_empty() && !self.message.trim().is_empty();
        let label = format!("{} ({})", strings.git_commit, paths.len());
        if ui.add_enabled(can, egui::Button::new(label)).clicked() {
            let message = std::mem::take(&mut self.message);
            return vec![GitCommand::Commit { paths, message }];
        }
        Vec::new()
    }
}

/// What changed in a file, field by field: each field's lines before
/// (`-`) and after (`+`).
fn diff_view(ui: &mut egui::Ui, diff: &FileDiff, strings: &Strings) {
    let palette = theme::palette(ui);
    ui.label(RichText::new(&diff.path).strong().monospace());
    match diff.change {
        FileChange::Added => {
            ui.label(RichText::new(strings.git_added).color(palette.text_muted));
        }
        FileChange::Deleted => {
            ui.label(RichText::new(strings.git_deleted).color(palette.text_muted));
        }
        FileChange::Modified if diff.fields.is_empty() => {
            ui.label(RichText::new(strings.git_same).color(palette.text_muted));
        }
        FileChange::Modified => {
            for field in &diff.fields {
                field_view(ui, field, strings);
            }
        }
    }
}

fn field_view(ui: &mut egui::Ui, field: &FieldChange, strings: &Strings) {
    let palette = theme::palette(ui);
    ui.add_space(theme::space::S);
    ui.label(RichText::new(&field.field).strong());
    // Before, removed; after, added; a side that is not there says so.
    for (sign, value, color) in [
        ('-', &field.before, palette.danger),
        ('+', &field.after, palette.success),
    ] {
        if let Some(value) = value {
            for line in value.lines() {
                ui.label(
                    RichText::new(format!("{sign} {line}"))
                        .monospace()
                        .color(color),
                );
            }
        } else {
            let none = format!("{sign} {}", strings.git_none);
            ui.label(RichText::new(none).monospace().color(palette.text_muted));
        }
    }
}
