//! A key/value table: a checkbox to send each row, its name, its value
//! (with variables highlighted) and a button to remove it, plus an empty row
//! at the end that becomes a new row as soon as something is typed in it.

use apifast_domain::{FormField, Header, Param};
use eframe::egui::{self, Id};

use crate::atoms::{TemplateField, TemplateFieldTexts, VariableHints};
use crate::strings::Strings;
use crate::theme;

/// Width of the name column, as a share of the table's width.
const NAME_SHARE: f32 = 0.35;

/// A row the table can edit.
pub trait KvRow {
    /// Whether it is sent.
    fn enabled(&mut self) -> &mut bool;
    /// Its name.
    fn name(&mut self) -> &mut String;
    /// Its value.
    fn value(&mut self) -> &mut String;
}

impl KvRow for Header {
    fn enabled(&mut self) -> &mut bool {
        &mut self.enabled
    }
    fn name(&mut self) -> &mut String {
        &mut self.name
    }
    fn value(&mut self) -> &mut String {
        &mut self.value
    }
}

impl KvRow for FormField {
    fn enabled(&mut self) -> &mut bool {
        &mut self.enabled
    }
    fn name(&mut self) -> &mut String {
        &mut self.name
    }
    fn value(&mut self) -> &mut String {
        &mut self.value
    }
}

impl KvRow for Param {
    fn enabled(&mut self) -> &mut bool {
        &mut self.enabled
    }
    fn name(&mut self) -> &mut String {
        &mut self.name
    }
    fn value(&mut self) -> &mut String {
        &mut self.value
    }
}

/// The table over some of the rows of a list.
pub struct KvTable<'a, T> {
    id_salt: &'a str,
    rows: &'a mut Vec<T>,
    shows: fn(&T) -> bool,
    new: fn(String, String) -> T,
    fixed_names: bool,
    hints: &'a VariableHints,
    strings: &'a Strings,
}

impl<'a, T: KvRow> KvTable<'a, T> {
    /// A table, named `id_salt`, over the `rows` that `shows` accepts. New
    /// rows are made with `new` and added at the end of the list.
    pub fn new(
        id_salt: &'a str,
        rows: &'a mut Vec<T>,
        shows: fn(&T) -> bool,
        new: fn(String, String) -> T,
        hints: &'a VariableHints,
        strings: &'a Strings,
    ) -> Self {
        Self {
            id_salt,
            rows,
            shows,
            new,
            fixed_names: false,
            hints,
            strings,
        }
    }

    /// Names cannot be edited and rows cannot be added or removed: for
    /// path parameters, which come from the URL.
    #[must_use]
    pub fn fixed_names(mut self) -> Self {
        self.fixed_names = true;
        self
    }

    /// Draws the table. Returns whether the user changed the rows.
    pub fn show(self, ui: &mut egui::Ui) -> bool {
        let Self {
            id_salt,
            rows,
            shows,
            new,
            fixed_names,
            hints,
            strings,
        } = self;
        // Relative to `ui`, so two tables with one name in different places
        // (the two themes of the gallery) do not share their fields.
        let id: Id = ui.make_persistent_id(id_salt);
        let texts = TemplateFieldTexts {
            placeholder: strings.value,
            missing: strings.variable_missing,
            generated: strings.variable_generated,
        };
        let name_width = ui.available_width() * NAME_SHARE;
        let mut changed = false;
        let mut remove = None;
        egui::Grid::new(id)
            .num_columns(4)
            .spacing([theme::space::S, theme::space::XS])
            .show(ui, |ui| {
                for (index, row) in rows.iter_mut().enumerate() {
                    if !shows(row) {
                        continue;
                    }
                    changed |= ui
                        .checkbox(row.enabled(), "")
                        .accessible_name(strings.enabled)
                        .on_hover_text(strings.enabled)
                        .changed();
                    if fixed_names {
                        ui.label(egui::RichText::new(row.name().as_str()).monospace());
                    } else {
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(row.name())
                                    .id(id.with((index, 0)))
                                    .hint_text(strings.name)
                                    .desired_width(name_width),
                            )
                            .accessible_name(strings.name)
                            .changed();
                    }
                    changed |= TemplateField::new(row.value(), hints, texts)
                        .id(id.with((index, 1)))
                        .accessible_name(strings.value)
                        .show(ui)
                        .changed();
                    if !fixed_names
                        && ui
                            .small_button("×")
                            .accessible_name(strings.remove_row)
                            .on_hover_text(strings.remove_row)
                            .clicked()
                    {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
                if fixed_names {
                    return;
                }
                // The empty row. Its fields have the ids the row will have
                // once added, so typing keeps the focus.
                let index = rows.len();
                let (mut name, mut value) = (String::new(), String::new());
                ui.add_enabled(false, egui::Checkbox::without_text(&mut true))
                    .accessible_name(strings.enabled);
                let typed_name = ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .id(id.with((index, 0)))
                            .hint_text(strings.name)
                            .desired_width(name_width),
                    )
                    .accessible_name(strings.name)
                    .changed();
                let typed_value = TemplateField::new(&mut value, hints, texts)
                    .id(id.with((index, 1)))
                    .accessible_name(strings.value)
                    .show(ui)
                    .changed();
                ui.end_row();
                if typed_name || typed_value {
                    rows.push(new(name, value));
                    changed = true;
                }
            });
        if let Some(index) = remove {
            rows.remove(index);
            changed = true;
        }
        changed
    }
}
