//! A key/value table: a checkbox to send each row, its name, its value
//! (with variables highlighted) and a button to remove it, plus an empty row
//! at the end that becomes a new row as soon as something is typed in it.
//!
//! A switch turns the table into text, one row per line, to paste or edit
//! many rows at once.

use apifast_domain::{FormField, Header, Param};
use eframe::egui::{self, Id};

use crate::atoms::{
    CloseButton, TemplateField, TemplateFieldTexts, VariableHints, completions, refocus,
    suggestions,
};
use crate::strings::Strings;
use crate::theme;

/// Width of the name column, as a share of the table's width.
const NAME_SHARE: f32 = 0.35;
/// The least the two fields of a row take together, in points, however
/// narrow the panel.
const MIN_FIELDS: f32 = 200.0;
/// Rows the bulk editor shows before scrolling.
const BULK_ROWS: usize = 8;
/// Starts a disabled row in the bulk text.
const DISABLED: &str = "//";

/// A row the table can edit.
pub trait KvRow {
    /// Whether it is sent.
    fn enabled(&mut self) -> &mut bool;
    /// Its name.
    fn name(&mut self) -> &mut String;
    /// Its value.
    fn value(&mut self) -> &mut String;
}

macro_rules! kv_row {
    ($type:ty) => {
        impl KvRow for $type {
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
    };
}

kv_row!(Header);
kv_row!(FormField);
kv_row!(Param);

/// One row of the bulk text: whether it is enabled, its name and its value.
pub type BulkRow = (bool, String, String);

/// Rows as bulk text: `name: value` per line, `//` before disabled ones.
pub fn to_bulk(rows: impl IntoIterator<Item = BulkRow>) -> String {
    rows.into_iter()
        .map(|(enabled, name, value)| {
            let prefix = if enabled { "" } else { DISABLED };
            if value.is_empty() {
                format!("{prefix}{name}")
            } else {
                format!("{prefix}{name}: {value}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bulk text as rows. Blank lines are skipped; the name ends at the first
/// `:`, and spaces around the name and the value are dropped.
pub fn from_bulk(text: &str) -> Vec<BulkRow> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (enabled, line) = match line.strip_prefix(DISABLED) {
                Some(rest) => (false, rest.trim_start()),
                None => (true, line),
            };
            if line.is_empty() {
                return None;
            }
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            Some((enabled, name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

/// What the table remembers between frames: the bulk text while it is
/// edited as text, kept as typed so its spacing does not jump.
#[derive(Debug, Clone, Default)]
struct BulkState {
    text: Option<String>,
}

/// The table over some of the rows of a list.
pub struct KvTable<'a, T> {
    id_salt: &'a str,
    rows: &'a mut Vec<T>,
    shows: fn(&T) -> bool,
    new: fn(String, String) -> T,
    fixed_names: bool,
    names: &'a [&'a str],
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
            names: &[],
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

    /// Names suggested while typing one, such as well-known headers.
    #[must_use]
    pub fn name_suggestions(mut self, names: &'a [&'a str]) -> Self {
        self.names = names;
        self
    }

    /// Draws the table. Returns whether the user changed the rows.
    pub fn show(mut self, ui: &mut egui::Ui) -> bool {
        // Relative to `ui`, so two tables with one name in different places
        // (the two themes of the gallery) do not share their fields.
        let id: Id = ui.make_persistent_id(self.id_salt);
        if self.fixed_names {
            return self.grid(ui, id);
        }
        let mut state: BulkState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let strings = self.strings;
        let mut bulk = state.text.is_some();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut bulk, strings.bulk_edit);
            });
        });
        let changed = match (bulk, state.text.take()) {
            (true, Some(mut text)) => {
                let changed = self.bulk(ui, id, &mut text);
                state.text = Some(text);
                changed
            }
            // Just switched to text: start from the rows.
            (true, None) => {
                state.text = Some(to_bulk(self.shown()));
                false
            }
            (false, _) => self.grid(ui, id),
        };
        ui.data_mut(|d| d.insert_temp(id, state));
        changed
    }

    /// The rows the table shows, as bulk rows.
    fn shown(&mut self) -> Vec<BulkRow> {
        let shows = self.shows;
        self.rows
            .iter_mut()
            .filter(|row| shows(row))
            .map(|row| (*row.enabled(), row.name().clone(), row.value().clone()))
            .collect()
    }

    fn bulk(self, ui: &mut egui::Ui, id: Id, text: &mut String) -> bool {
        ui.label(egui::RichText::new(self.strings.bulk_hint).color(theme::palette(ui).text_muted));
        let edited = ui
            .add(
                egui::TextEdit::multiline(text)
                    .id(id.with("bulk"))
                    .code_editor()
                    .desired_rows(BULK_ROWS)
                    .desired_width(f32::INFINITY),
            )
            .accessible_name(self.strings.bulk_text)
            .changed();
        if !edited {
            return false;
        }
        // The rows follow the text as it is typed. Rows the table does not
        // show (path parameters, next to query ones) are kept.
        let (shows, new) = (self.shows, self.new);
        self.rows.retain(|row| !shows(row));
        for (enabled, name, value) in from_bulk(text) {
            let mut row = new(name, value);
            *row.enabled() = enabled;
            self.rows.push(row);
        }
        true
    }

    fn grid(self, ui: &mut egui::Ui, id: Id) -> bool {
        let Self {
            rows,
            shows,
            new,
            fixed_names,
            names,
            hints,
            strings,
            ..
        } = self;
        let texts = TemplateFieldTexts {
            placeholder: strings.value,
            missing: strings.variable_missing,
            generated: strings.variable_generated,
        };
        // Rows laid out by hand, not with a grid: a field in a grid cell only
        // gets the width its column measured before, so it never grows.
        let widths = Widths::new(ui, fixed_names);
        let mut changed = false;
        let mut remove = None;
        for (index, row) in rows.iter_mut().enumerate() {
            if !shows(row) {
                continue;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::S;
                changed |= ui
                    .checkbox(row.enabled(), "")
                    .accessible_name(strings.enabled)
                    .on_hover_text(strings.enabled)
                    .changed();
                if fixed_names {
                    let size = egui::vec2(widths.name, ui.spacing().interact_size.y);
                    ui.allocate_ui_with_layout(
                        size,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| ui.label(egui::RichText::new(row.name().as_str()).monospace()),
                    );
                } else {
                    let cell = id.with((index, 0));
                    changed |= name_field(ui, row.name(), cell, widths.name, names, strings);
                }
                changed |= TemplateField::new(row.value(), hints, texts)
                    .id(id.with((index, 1)))
                    .desired_width(widths.value)
                    .accessible_name(strings.value)
                    .show(ui)
                    .changed();
                if !fixed_names && ui.add(CloseButton::new(strings.remove_row)).clicked() {
                    remove = Some(index);
                }
            });
        }
        if !fixed_names {
            // The empty row. Its fields have the ids the row will have once
            // added, so typing keeps the focus.
            let index = rows.len();
            let (mut name, mut value) = (String::new(), String::new());
            let (typed_name, typed_value) = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::space::S;
                    ui.add_enabled(false, egui::Checkbox::without_text(&mut true))
                        .accessible_name(strings.enabled);
                    let cell = id.with((index, 0));
                    let typed_name = name_field(ui, &mut name, cell, widths.name, names, strings);
                    let typed_value = TemplateField::new(&mut value, hints, texts)
                        .id(id.with((index, 1)))
                        .desired_width(widths.value)
                        .accessible_name(strings.value)
                        .show(ui)
                        .changed();
                    (typed_name, typed_value)
                })
                .inner;
            if typed_name || typed_value {
                rows.push(new(name, value));
                changed = true;
            }
        }
        if let Some(index) = remove {
            rows.remove(index);
            changed = true;
        }
        changed
    }
}

/// Widths of the name and value fields of a row, from the space there is.
struct Widths {
    name: f32,
    value: f32,
}

impl Widths {
    fn new(ui: &egui::Ui, fixed_names: bool) -> Self {
        let gap = theme::space::S;
        let check = ui.spacing().icon_width;
        let close = ui.spacing().interact_size.y.min(18.0);
        // Without a remove button, the value takes its place.
        let buttons = if fixed_names { 0.0 } else { close + gap };
        let flexible = (ui.available_width() - check - 2.0 * gap - buttons).max(MIN_FIELDS);
        let name = flexible * NAME_SHARE;
        Self {
            name,
            value: flexible - name,
        }
    }
}

/// A name cell, with `names` suggested as it is typed. Returns whether the
/// name changed.
fn name_field(
    ui: &mut egui::Ui,
    name: &mut String,
    id: Id,
    width: f32,
    names: &[&str],
    strings: &Strings,
) -> bool {
    let field = ui
        .add(
            egui::TextEdit::singleline(name)
                .id(id)
                .hint_text(strings.name)
                .desired_width(width),
        )
        .accessible_name(strings.name);
    let mut changed = field.changed();
    if !names.is_empty() && !name.is_empty() {
        let found = completions(name, names.iter().copied());
        if let Some(index) = suggestions(ui, &field, name, &found) {
            found[index].clone_into(name);
            refocus(ui, &field, name.chars().count());
            changed = true;
        }
    }
    changed
}
