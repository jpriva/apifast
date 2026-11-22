//! The runner window (docs/09): what to run and how, the progress of the
//! run as it goes, and its results, each step opening its request with its
//! response.

use std::path::PathBuf;
use std::sync::mpsc;

use apifast_app::{ReportFormat, ReportMeta, StepResult, assertion_text, duration};
use eframe::egui::{self, RichText};

use crate::core::{RunSettings, RunView};
use crate::strings::{Strings, fill};
use crate::theme;

/// What the user asked of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerEvent {
    /// Start a run.
    Run {
        /// The collection.
        root: PathBuf,
        /// What and how.
        settings: RunSettings,
    },
    /// Stop the run.
    Stop,
    /// Forget the run that ended.
    Close,
    /// Open the request of a step.
    OpenStep(usize),
}

/// The window and its form.
#[derive(Debug, Default)]
pub struct RunnerWindow {
    /// Whether it is shown.
    pub open: bool,
    /// The collection and folder it runs, and how it names them.
    target: Option<(PathBuf, Option<String>, String)>,
    settings: RunSettings,
    tags: String,
    exclude_tags: String,
    picking: Option<mpsc::Receiver<Option<PathBuf>>>,
    /// Where the last report went, or why it did not.
    exported: Option<String>,
}

fn list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

impl RunnerWindow {
    /// Opens the window to run `folder` (or all) of the collection at
    /// `root`, called `label`.
    pub fn open_for(&mut self, root: PathBuf, folder: Option<String>, label: String) {
        let same = self
            .target
            .as_ref()
            .is_some_and(|(r, f, _)| *r == root && *f == folder);
        if !same {
            self.settings = RunSettings {
                folder: folder.clone(),
                ..RunSettings::default()
            };
            self.tags.clear();
            self.exclude_tags.clear();
        }
        self.target = Some((root, folder, label));
        self.exported = None;
        self.open = true;
    }

    /// Draws the window over `run`, the last run.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        run: Option<&RunView>,
        strings: &Strings,
    ) -> Option<RunnerEvent> {
        if !self.open {
            return None;
        }
        if let Some(picked) = self.picked() {
            self.settings.data = Some(picked);
        }
        let mut open = true;
        let mut event = None;
        let title = match &self.target {
            Some((_, _, label)) => format!("{} · {label}", strings.runner),
            None => strings.runner.to_owned(),
        };
        egui::Window::new(title)
            .id(egui::Id::unique("runner-window"))
            .open(&mut open)
            .default_size([560.0, 480.0])
            .show(ctx, |ui| {
                let running = run.is_some_and(|r| r.running);
                if !running {
                    event = self.form(ui, strings);
                    ui.separator();
                }
                if let Some(run) = run {
                    let progress = self.progress(ui, run, strings);
                    if event.is_none() {
                        event = progress;
                    }
                }
            });
        if !open {
            self.open = false;
        }
        event
    }

    fn form(&mut self, ui: &mut egui::Ui, strings: &Strings) -> Option<RunnerEvent> {
        let settings = &mut self.settings;
        egui::Grid::new("runner-form")
            .num_columns(2)
            .spacing([theme::space::M, theme::space::S])
            .show(ui, |ui| {
                ui.label(strings.run_recursive);
                ui.checkbox(&mut settings.recursive, "")
                    .accessible_name(strings.run_recursive);
                ui.end_row();
                ui.label(strings.run_tags);
                ui.add(egui::TextEdit::singleline(&mut self.tags).hint_text("smoke, auth"))
                    .accessible_name(strings.run_tags);
                ui.end_row();
                ui.label(strings.run_exclude_tags);
                ui.add(egui::TextEdit::singleline(&mut self.exclude_tags).hint_text("slow"))
                    .accessible_name(strings.run_exclude_tags);
                ui.end_row();
                ui.label(strings.run_delay);
                ui.add(
                    egui::DragValue::new(&mut settings.delay_ms)
                        .suffix(" ms")
                        .range(0..=60_000),
                )
                .accessible_name(strings.run_delay);
                ui.end_row();
                ui.label(strings.run_iterations);
                ui.add_enabled(
                    settings.data.is_none(),
                    egui::DragValue::new(&mut settings.iterations).range(1..=10_000),
                )
                .accessible_name(strings.run_iterations);
                ui.end_row();
                ui.label(strings.run_data);
                ui.horizontal(|ui| {
                    if let Some(file) = &settings.data {
                        let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
                        ui.label(RichText::new(name.unwrap_or_default()).monospace())
                            .on_hover_text(file.display().to_string());
                        if ui.small_button(strings.run_data_clear).clicked() {
                            settings.data = None;
                        }
                    } else {
                        let picking = self.picking.is_some();
                        if ui
                            .add_enabled(!picking, egui::Button::new(strings.run_data_pick))
                            .clicked()
                        {
                            self.picking = Some(pick_data(
                                ui.ctx().clone(),
                                strings.run_data,
                                strings.csv_or_json,
                            ));
                        }
                    }
                });
                ui.end_row();
                ui.label(strings.run_bail);
                ui.checkbox(&mut settings.bail, "")
                    .accessible_name(strings.run_bail);
                ui.end_row();
            });
        ui.add_space(theme::space::S);
        let (root, _, _) = self.target.as_ref()?;
        if ui
            .button(RichText::new(strings.run_start).strong())
            .clicked()
        {
            let mut settings = self.settings.clone();
            settings.tags = list(&self.tags);
            settings.exclude_tags = list(&self.exclude_tags);
            return Some(RunnerEvent::Run {
                root: root.clone(),
                settings,
            });
        }
        None
    }

    fn progress(
        &mut self,
        ui: &mut egui::Ui,
        run: &RunView,
        strings: &Strings,
    ) -> Option<RunnerEvent> {
        let palette = theme::palette(ui);
        let mut event = None;
        let all = run.total * run.iterations as usize;
        ui.horizontal(|ui| {
            let text = format!("{} / {all}", run.steps.len());
            ui.add(
                egui::ProgressBar::new(run.progress())
                    .text(text)
                    .desired_width(240.0),
            );
            if run.running {
                if ui.button(strings.run_stop).clicked() {
                    event = Some(RunnerEvent::Stop);
                }
            } else if ui.button(strings.run_clear).clicked() {
                event = Some(RunnerEvent::Close);
            }
        });
        if let Some((iteration, _, path)) = &run.current {
            let mut text = path.to_string();
            if run.iterations > 1 {
                let iteration = fill(
                    strings.run_iteration,
                    &[("n", iteration), ("total", &run.iterations)],
                );
                text = format!("{text} · {iteration}");
            }
            ui.label(RichText::new(text).color(palette.text_muted).monospace());
        }
        if let Some(error) = &run.error {
            ui.label(RichText::new(error.as_str()).color(palette.danger));
        }
        if let Some(report) = &run.report {
            let s = report.summary();
            let text = strings
                .run_summary
                .replace("{passed}", &s.passed.to_string())
                .replace("{failed}", &s.failed.to_string())
                .replace("{errors}", &s.errors.to_string())
                .replace("{skipped}", &s.skipped.to_string())
                .replace("{time}", &duration(report.duration_ms));
            let color = if report.succeeded() {
                palette.success
            } else {
                palette.danger
            };
            ui.label(RichText::new(text).color(color).strong());
            ui.horizontal(|ui| {
                ui.label(strings.run_export);
                for (label, format, extension) in [
                    ("HTML", ReportFormat::Html, "html"),
                    ("JUnit", ReportFormat::Junit, "xml"),
                    ("JSON", ReportFormat::Json, "json"),
                ] {
                    if ui.small_button(label).clicked() {
                        let meta = ReportMeta {
                            collection: run.collection.clone(),
                            environment: run.environment.clone(),
                            started_ms: run.started_ms,
                        };
                        self.exported = Some(export(&format.render(report, &meta), extension));
                    }
                }
            });
            if let Some(exported) = &self.exported {
                ui.label(
                    RichText::new(exported.as_str())
                        .color(palette.text_muted)
                        .small(),
                );
            }
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("runner-steps")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (index, step) in run.steps.iter().enumerate() {
                    if step_row(ui, step, run.iterations > 1, strings) {
                        event = Some(RunnerEvent::OpenStep(index));
                    }
                }
                if let Some(report) = &run.report {
                    for skipped in &report.skipped {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(strings.run_skip)
                                    .color(palette.warning)
                                    .monospace(),
                            );
                            ui.label(RichText::new(skipped.path.as_str()).monospace());
                            ui.label(
                                RichText::new(skipped.reason.as_str()).color(palette.text_muted),
                            );
                        });
                    }
                }
            });
        event
    }

    fn picked(&mut self) -> Option<PathBuf> {
        let receiver = self.picking.as_ref()?;
        match receiver.try_recv() {
            Ok(file) => {
                self.picking = None;
                file
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.picking = None;
                None
            }
        }
    }
}

/// One step: its mark, name, file, status and time, and its failing
/// assertions. Returns whether it was clicked.
fn step_row(ui: &mut egui::Ui, step: &StepResult, iterations: bool, strings: &Strings) -> bool {
    let palette = theme::palette(ui);
    let (mark, color) = match &step.outcome {
        Err(_) => (strings.run_error, palette.danger),
        Ok(_) if step.passed() => (strings.pass, palette.success),
        Ok(_) => (strings.fail, palette.danger),
    };
    let clicked = ui
        .horizontal(|ui| {
            ui.label(RichText::new(mark).color(color).monospace().strong());
            let name = if iterations {
                format!("{} [{}]", step.name, step.iteration)
            } else {
                step.name.clone()
            };
            let link = ui.link(name).on_hover_text(strings.run_open_step);
            ui.label(
                RichText::new(step.path.as_str())
                    .color(palette.text_muted)
                    .monospace(),
            );
            let detail = match &step.outcome {
                Ok(outcome) => format!("{} · {}", outcome.head.status, duration(step.duration_ms)),
                Err(_) => duration(step.duration_ms),
            };
            ui.label(RichText::new(detail).color(palette.text_muted));
            link.clicked()
        })
        .inner;
    match &step.outcome {
        Err(message) => {
            ui.indent(("error", step.iteration, step.path.as_str()), |ui| {
                ui.label(RichText::new(message.as_str()).color(palette.danger));
            });
        }
        Ok(outcome) => {
            for assertion in outcome.assertions.iter().filter(|a| !a.verdict.passed) {
                ui.indent(("failed", step.iteration, step.path.as_str()), |ui| {
                    let message = assertion.verdict.message.as_deref().unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{}: {message}", assertion_text(assertion)))
                            .color(palette.danger),
                    );
                });
            }
        }
    }
    clicked
}

/// Opens the system's file picker for a data file, on a thread of its own.
fn pick_data(ctx: egui::Context, title: &str, filter: &str) -> mpsc::Receiver<Option<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    let title = title.to_owned();
    let filter = filter.to_owned();
    std::thread::spawn(move || {
        let file = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, &["csv", "json"])
            .pick_file();
        let _ = sender.send(file);
        ctx.request_repaint();
    });
    receiver
}

/// Asks where to save a report and writes it. The dialog blocks this frame,
/// as saving is a deliberate, rare action. Returns what happened.
fn export(text: &str, extension: &str) -> String {
    let Some(file) = rfd::FileDialog::new()
        .set_file_name(format!("report.{extension}"))
        .save_file()
    else {
        return String::new();
    };
    match std::fs::write(&file, text) {
        Ok(()) => file.display().to_string(),
        Err(e) => format!("{}: {e}", file.display()),
    }
}
