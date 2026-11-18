//! The editors of multipart and file bodies: parts with text or files, and
//! the file a body sends. Files are picked with the system's dialog, off the
//! interface's thread, and kept relative to the collection when inside it,
//! as Bruno does.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use apifast_domain::{MultipartPart, MultipartValue};
use eframe::egui::{self, RichText};

use crate::atoms::{CloseButton, TemplateField, TemplateFieldTexts, VariableHints};
use crate::strings::Strings;
use crate::theme;

/// Width of a part's name, in points.
const NAME_WIDTH: f32 = 140.0;
/// Width of a content type, in points.
const TYPE_WIDTH: f32 = 140.0;

/// Where picked files go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The files of the multipart part at this index.
    Part(usize),
    /// The file of a file body.
    File,
}

/// What the editors remember: the collection's folder, and a file dialog
/// that is open.
#[derive(Debug, Default)]
pub struct BodyFiles {
    /// The folder of the request's collection, to keep paths inside it
    /// relative.
    pub root: Option<PathBuf>,
    picking: Option<(Target, Receiver<Vec<PathBuf>>)>,
}

impl BodyFiles {
    /// Whether a file dialog is open.
    pub fn picking(&self) -> bool {
        self.picking.is_some()
    }

    /// Opens the system's dialog for one file (`many` false) or several.
    fn pick(&mut self, ctx: &egui::Context, target: Target, many: bool) {
        let (sender, receiver) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let files = if many {
                dialog.pick_files().unwrap_or_default()
            } else {
                dialog.pick_file().into_iter().collect()
            };
            let _ = sender.send(files);
            ctx.request_repaint();
        });
        self.picking = Some((target, receiver));
    }

    /// The files picked for `target`, once the dialog closes, as they are
    /// written in the request.
    fn picked(&mut self, target: Target) -> Option<Vec<String>> {
        let (waiting, receiver) = self.picking.as_ref()?;
        if *waiting != target {
            return None;
        }
        let files = match receiver.try_recv() {
            Ok(files) => files,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        self.picking = None;
        let root = self.root.as_deref();
        Some(files.iter().map(|file| written_path(file, root)).collect())
    }
}

/// `file` as a request writes it: relative to `root`, with `/`, when it is
/// inside it; otherwise as it is.
pub fn written_path(file: &Path, root: Option<&Path>) -> String {
    match root.and_then(|root| file.strip_prefix(root).ok()) {
        Some(relative) => (relative.components())
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        None => file.display().to_string(),
    }
}

/// The name a path shows: its last part.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The parts of a multipart body. Returns whether they changed.
pub fn multipart(
    ui: &mut egui::Ui,
    parts: &mut Vec<MultipartPart>,
    files: &mut BodyFiles,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let texts = TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    };
    let id = ui.make_persistent_id("multipart-parts");
    let mut changed = false;
    let mut remove = None;
    for (index, part) in parts.iter_mut().enumerate() {
        if let Some(picked) = files.picked(Target::Part(index))
            && !picked.is_empty()
            && let MultipartValue::Files(paths) = &mut part.value
        {
            paths.extend(picked);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            changed |= ui
                .checkbox(&mut part.enabled, "")
                .accessible_name(strings.enabled)
                .on_hover_text(strings.enabled)
                .changed();
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut part.name)
                        .id(id.with((index, "name")))
                        .hint_text(strings.name)
                        .desired_width(NAME_WIDTH),
                )
                .accessible_name(strings.name)
                .changed();
            changed |= part_kind(ui, part, (id, index), strings);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(part.content_type.get_or_insert_default())
                        .id(id.with((index, "type")))
                        .hint_text(strings.part_content_type_hint)
                        .desired_width(TYPE_WIDTH),
                )
                .accessible_name(strings.part_content_type)
                .changed();
            // An empty content type is none: the transport picks one.
            if part.content_type.as_deref() == Some("") {
                part.content_type = None;
            }
            if ui.add(CloseButton::new(strings.remove_row)).clicked() {
                remove = Some(index);
            }
        });
        match &mut part.value {
            MultipartValue::Text(text) => {
                ui.horizontal(|ui| {
                    ui.add_space(ui.spacing().interact_size.y + theme::space::S);
                    changed |= TemplateField::new(text, hints, texts)
                        .id(id.with((index, "value")))
                        .desired_width(ui.available_width())
                        .accessible_name(strings.value)
                        .show(ui)
                        .changed();
                });
            }
            MultipartValue::Files(paths) => {
                changed |= file_list(ui, paths, strings);
                ui.horizontal(|ui| {
                    ui.add_space(ui.spacing().interact_size.y + theme::space::S);
                    let button = egui::Button::new(strings.choose_files);
                    if ui.add_enabled(!files.picking(), button).clicked() {
                        files.pick(ui.ctx(), Target::Part(index), true);
                    }
                });
            }
        }
        ui.add_space(theme::space::S);
    }
    if let Some(index) = remove {
        parts.remove(index);
        changed = true;
    }
    if ui.button(strings.add_part).clicked() {
        parts.push(MultipartPart {
            name: String::new(),
            value: MultipartValue::Text(String::new()),
            content_type: None,
            enabled: true,
        });
        changed = true;
    }
    changed
}

/// Whether a part is text or files. Changing it starts the value empty.
fn part_kind(
    ui: &mut egui::Ui,
    part: &mut MultipartPart,
    (id, index): (egui::Id, usize),
    strings: &Strings,
) -> bool {
    let is_text = matches!(part.value, MultipartValue::Text(_));
    let mut text = is_text;
    egui::ComboBox::from_id_salt(id.with((index, "kind")))
        .selected_text(if is_text {
            strings.part_text
        } else {
            strings.part_files
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut text, true, strings.part_text);
            ui.selectable_value(&mut text, false, strings.part_files);
        })
        .response
        .on_hover_text(strings.part_kind);
    if text == is_text {
        return false;
    }
    part.value = if text {
        MultipartValue::Text(String::new())
    } else {
        MultipartValue::Files(Vec::new())
    };
    true
}

/// The files of a part, each with a button to take it out.
fn file_list(ui: &mut egui::Ui, paths: &mut Vec<String>, strings: &Strings) -> bool {
    let muted = theme::palette(ui).text_muted;
    let mut remove = None;
    for (index, path) in paths.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().interact_size.y + theme::space::S);
            ui.label(RichText::new(file_name(path)).monospace())
                .on_hover_text(path.as_str());
            ui.label(RichText::new(path.as_str()).color(muted).small());
            if ui.add(CloseButton::new(strings.remove_file)).clicked() {
                remove = Some(index);
            }
        });
    }
    if paths.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().interact_size.y + theme::space::S);
            ui.label(RichText::new(strings.no_files).color(muted));
        });
    }
    remove.map(|index| paths.remove(index)).is_some()
}

/// The file a file body sends, and its content type. Returns whether they
/// changed.
pub fn file(
    ui: &mut egui::Ui,
    (path, content_type): (&mut String, &mut Option<String>),
    files: &mut BodyFiles,
    strings: &Strings,
) -> bool {
    let mut changed = false;
    if let Some(picked) = files.picked(Target::File)
        && let Some(first) = picked.into_iter().next()
    {
        *path = first;
        changed = true;
    }
    egui::Grid::new("file-body")
        .num_columns(2)
        .spacing([theme::space::L, theme::space::S])
        .show(ui, |ui| {
            ui.label(strings.body_file_path);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(path)
                            .hint_text("files/upload.bin")
                            .desired_width(320.0),
                    )
                    .accessible_name(strings.body_file_path)
                    .changed();
                let button = egui::Button::new(strings.choose);
                if ui.add_enabled(!files.picking(), button).clicked() {
                    files.pick(ui.ctx(), Target::File, false);
                }
            });
            ui.end_row();
            ui.label(strings.part_content_type);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(content_type.get_or_insert_default())
                        .hint_text(strings.part_content_type_hint)
                        .desired_width(320.0),
                )
                .accessible_name(strings.part_content_type)
                .changed();
            if content_type.as_deref() == Some("") {
                *content_type = None;
            }
            ui.end_row();
        });
    let muted = theme::palette(ui).text_muted;
    ui.label(RichText::new(strings.body_file_hint).color(muted).small());
    changed
}
