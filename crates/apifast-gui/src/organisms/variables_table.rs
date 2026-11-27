//! The variables of a request, a folder or the collection: a table to edit
//! them, and the window that edits those of a folder or the collection.
//!
//! Only an environment keeps secret values (in the vault, by environment),
//! so here a secret keeps its name and flag as the file has them, and its
//! value cannot be typed.

use std::path::PathBuf;

use apifast_domain::{ValueKind, Variable, VariableValue};
use eframe::egui::{self, RichText};

use crate::atoms::{CloseButton, TemplateField, TemplateFieldTexts, VariableHints};
use crate::strings::Strings;
use crate::theme;

/// The name a kind of value goes by.
pub(crate) fn kind_label(kind: ValueKind, strings: &Strings) -> &str {
    match kind {
        ValueKind::String => strings.kind_text,
        ValueKind::Number => strings.kind_number,
        ValueKind::Bool => strings.kind_bool,
        ValueKind::Object => strings.kind_object,
    }
}

/// A cell of `width`, its contents centered on the row's line.
pub(crate) fn cell<R>(
    ui: &mut egui::Ui,
    width: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            content(ui)
        },
    )
    .inner
}

/// A combo box to choose the kind of `variable`'s value. Whether it
/// changed.
pub(crate) fn kind_choice(
    ui: &mut egui::Ui,
    variable: &mut Variable,
    id: egui::Id,
    width: f32,
    strings: &Strings,
) -> bool {
    let kind = variable
        .value
        .as_ref()
        .map_or(ValueKind::String, |v| v.kind);
    let mut changed = false;
    ui.add_enabled_ui(!variable.secret, |ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(kind_label(kind, strings))
            .width(width)
            .show_ui(ui, |ui| {
                for option in [
                    ValueKind::String,
                    ValueKind::Number,
                    ValueKind::Bool,
                    ValueKind::Object,
                ] {
                    if ui
                        .selectable_label(option == kind, kind_label(option, strings))
                        .clicked()
                        && let Some(value) = variable.value.as_mut()
                        && value.kind != option
                    {
                        value.kind = option;
                        changed = true;
                    }
                }
            })
            .response
            .on_hover_text(strings.kind);
    });
    changed
}

/// Widths of the columns.
struct Columns {
    check: f32,
    name: f32,
    value: f32,
    kind: f32,
}

impl Columns {
    fn new(ui: &egui::Ui) -> Self {
        let gap = theme::space::S;
        let check = ui.spacing().icon_width;
        let (kind, close) = (90.0, 18.0);
        let flexible = (ui.available_width() - check - kind - close - 4.0 * gap).max(240.0);
        let name = flexible * 0.35;
        Self {
            check,
            name,
            value: flexible - name,
            kind,
        }
    }
}

/// The table of `variables`, a row each and an empty row to add one; `id`
/// tells it apart from other tables. Whether anything changed.
///
/// Rows are laid out by hand, not with a grid: a field in a grid cell only
/// gets the width its column measured before, so it never grows.
pub fn variables_table(
    ui: &mut egui::Ui,
    variables: &mut Vec<Variable>,
    id: egui::Id,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let texts = TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
        edit: strings.variable_edit,
    };
    let columns = Columns::new(ui);
    let mut changed = false;
    let mut remove = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::space::S;
        for (header, width) in [
            ("", columns.check),
            (strings.column_name, columns.name),
            (strings.column_value, columns.value),
            (strings.kind, columns.kind),
        ] {
            cell(ui, width, |ui| ui.label(RichText::new(header).strong()));
        }
    });
    let rows = variables.len();
    for (index, variable) in variables.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            changed |= row(ui, variable, (id, index), &columns, (hints, texts), strings);
            if ui.add(CloseButton::new(strings.remove_row)).clicked() {
                remove = Some(index);
            }
        });
    }
    // The empty row: typing a name adds the variable. Its field has the id
    // the new row's will have, so the focus stays.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::space::S;
        cell(ui, columns.check, |_| ());
        let mut name = String::new();
        if ui
            .add(
                egui::TextEdit::singleline(&mut name)
                    .id(id.with((rows, 0)))
                    .hint_text(strings.new_variable)
                    .desired_width(columns.name),
            )
            .accessible_name(strings.new_variable)
            .changed()
        {
            variables.push(Variable::text(name, ""));
            changed = true;
        }
    });
    if let Some(index) = remove {
        variables.remove(index);
        changed = true;
    }
    changed
}

fn row(
    ui: &mut egui::Ui,
    variable: &mut Variable,
    (id, index): (egui::Id, usize),
    columns: &Columns,
    (hints, texts): (&VariableHints, TemplateFieldTexts<'_>),
    strings: &Strings,
) -> bool {
    let mut changed = false;
    cell(ui, columns.check, |ui| {
        changed |= ui
            .checkbox(&mut variable.enabled, "")
            .accessible_name(strings.enabled)
            .on_hover_text(strings.enabled)
            .changed();
    });
    changed |= ui
        .add(
            egui::TextEdit::singleline(&mut variable.name)
                .id(id.with((index, 0)))
                .desired_width(columns.name),
        )
        .accessible_name(strings.name)
        .changed();
    if variable.secret {
        cell(ui, columns.value, |ui| {
            let muted = theme::palette(ui).text_muted;
            ui.label(
                RichText::new(strings.variable_secret_elsewhere)
                    .color(muted)
                    .small(),
            );
        });
    } else {
        let value = variable
            .value
            .get_or_insert_with(|| VariableValue::text(""));
        changed |= TemplateField::new(&mut value.data, hints, texts)
            .id(id.with((index, 1)))
            .desired_width(columns.value)
            .accessible_name(strings.value)
            .show(ui)
            .changed();
    }
    changed |= kind_choice(ui, variable, id.with((index, 2)), columns.kind, strings);
    changed
}

/// Whose variables the window edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariablesTarget {
    /// The collection.
    pub root: PathBuf,
    /// The folder, relative to it; `None` for the collection itself.
    pub folder: Option<String>,
}

/// The window with the variables of a folder or the collection.
#[derive(Debug, Default)]
pub struct VariablesWindow {
    /// Whose variables, how the title names it, and the variables as
    /// edited.
    editing: Option<(VariablesTarget, String, Vec<Variable>)>,
}

impl VariablesWindow {
    /// Opens the window on the variables of `target`, called `name`.
    pub fn open_for(&mut self, target: VariablesTarget, name: String, variables: Vec<Variable>) {
        self.editing = Some((target, name, variables));
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.editing.is_some()
    }

    /// Draws the window. Returns whose variables to save and the
    /// variables, once the user saves.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        hints: &VariableHints,
        strings: &Strings,
    ) -> Option<(VariablesTarget, Vec<Variable>)> {
        let (target, name, variables) = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        let heading = strings.variables_menu.trim_end_matches('…');
        let title = format!("{heading} · {name}");
        crate::molecules::window(
            ctx,
            egui::Window::new(&title)
                .id(egui::Id::unique("variables-window"))
                .default_width(560.0),
            (&title, strings.close_window),
            &mut open,
            |ui| {
                let muted = theme::palette(ui).text_muted;
                ui.label(RichText::new(strings.variables_hint).color(muted).small());
                ui.add_space(theme::space::S);
                let id = ui.make_persistent_id("variables-window-table");
                egui::ScrollArea::vertical()
                    .id_salt("variables-window-scroll")
                    .max_height(ui.available_height().max(200.0) - 40.0)
                    .show(ui, |ui| variables_table(ui, variables, id, hints, strings));
                ui.add_space(theme::space::S);
                if ui.button(RichText::new(strings.save).strong()).clicked() {
                    saved = Some((target.clone(), variables.clone()));
                }
            },
        );
        if saved.is_some() || !open {
            self.editing = None;
        }
        saved
    }
}
