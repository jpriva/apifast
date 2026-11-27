//! A key/value table: a checkbox to send each row, its name, its value
//! (with variables highlighted) and a button to remove it, plus an empty row
//! at the end that becomes a new row as soon as something is typed in it.
//!
//! A switch turns the table into text, one row per line, to paste or edit
//! many rows at once.

use apifast_domain::{FormField, Header, Param};
use eframe::egui::{self, Id, RichText};

use crate::atoms::{
    Icon, IconButton, TemplateField, TemplateFieldTexts, VariableHints, completions, refocus,
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
/// How visible the name and value of a row that is not sent are.
const DISABLED_OPACITY: f32 = 0.5;
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
            edit: strings.variable_edit,
        };
        // Rows laid out by hand, not with a grid: a field in a grid cell only
        // gets the width its column measured before, so it never grows.
        let columns = Columns::new(ui, fixed_names);
        let mut changed = false;
        let mut remove = None;
        ui.scope(|ui| {
            // The lines between rows are their only gap.
            ui.spacing_mut().item_spacing.y = 0.0;
            let top = ui.cursor().min.y;
            header(ui, &columns, strings);
            for (index, row) in rows.iter_mut().enumerate() {
                if !shows(row) {
                    continue;
                }
                let cells = Row::new(ui, &columns);
                let enabled = *row.enabled();
                cells.cell(ui, Column::Check, true, |ui| {
                    changed |= ui
                        .add(egui::Checkbox::without_text(row.enabled()))
                        .accessible_name(strings.enabled)
                        .on_hover_text(strings.enabled)
                        .changed();
                });
                cells.cell(ui, Column::Name, enabled, |ui| {
                    if fixed_names {
                        ui.add_space(f32::from(theme::cell_frame().inner_margin.left));
                        ui.label(RichText::new(row.name().as_str()).monospace());
                    } else {
                        let cell = (id.with((index, 0)), columns.name);
                        changed |= name_field(ui, row.name(), cell, names, strings.name, true);
                    }
                });
                cells.cell(ui, Column::Value, enabled, |ui| {
                    changed |= value_field(ui, row.value(), id.with((index, 1)), hints, texts);
                });
                if !fixed_names {
                    cells.cell(ui, Column::Remove, true, |ui| {
                        let button = IconButton::new(Icon::Trash, strings.remove_row)
                            .side(columns.remove * 0.75)
                            .quiet(!cells.hovered);
                        if ui.add(button).clicked() {
                            remove = Some(index);
                        }
                    });
                }
                cells.end(ui);
            }
            if !fixed_names {
                // The empty row. Its fields have the ids the row will have
                // once added, so typing keeps the focus.
                let index = rows.len();
                let (mut name, mut value) = (String::new(), String::new());
                let cells = Row::new(ui, &columns);
                cells.cell(ui, Column::Check, true, |ui| {
                    ui.add_enabled(false, egui::Checkbox::without_text(&mut true))
                        .accessible_name(strings.enabled);
                });
                let mut typed = false;
                cells.cell(ui, Column::Name, true, |ui| {
                    let cell = (id.with((index, 0)), columns.name);
                    typed |= name_field(ui, &mut name, cell, names, strings.name, true);
                });
                cells.cell(ui, Column::Value, true, |ui| {
                    typed |= value_field(ui, &mut value, id.with((index, 1)), hints, texts);
                });
                cells.end(ui);
                if typed {
                    rows.push(new(name, value));
                    changed = true;
                }
            }
            let bottom = ui.cursor().min.y;
            columns.paint_lines(ui, top, bottom);
        });
        if let Some(index) = remove {
            rows.remove(index);
            changed = true;
        }
        changed
    }
}

/// A value cell. Returns whether the value changed.
fn value_field(
    ui: &mut egui::Ui,
    value: &mut String,
    id: Id,
    hints: &VariableHints,
    texts: TemplateFieldTexts<'_>,
) -> bool {
    let response = TemplateField::new(value, hints, texts)
        .id(id)
        .frame(false)
        .desired_width(ui.available_width())
        .accessible_name(texts.placeholder)
        .show(ui);
    outline_focused(ui, &response);
    response.changed()
}

/// Outlines the cell of `field` with the accent while it has the focus:
/// a field without a frame shows nothing else but its cursor.
fn outline_focused(ui: &egui::Ui, field: &egui::Response) {
    if field.has_focus() {
        let stroke = egui::Stroke::new(1.0, theme::palette(ui).accent);
        let rect = ui.max_rect().shrink(1.0);
        ui.painter()
            .rect_stroke(rect, 0, stroke, egui::StrokeKind::Inside);
    }
}

/// The columns of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Check,
    Name,
    Value,
    Remove,
}

/// Where the columns of the table are, from the space there is, and how
/// tall its rows are.
struct Columns {
    left: f32,
    width: f32,
    check: f32,
    name: f32,
    value: f32,
    remove: f32,
    height: f32,
}

impl Columns {
    fn new(ui: &egui::Ui, fixed_names: bool) -> Self {
        let height = ui.spacing().interact_size.y;
        let check = ui.spacing().icon_width + 2.0 * theme::space::S;
        // Without a remove button, the value takes its place.
        let remove = if fixed_names { 0.0 } else { height };
        let width = ui.available_width().max(MIN_FIELDS + check + remove);
        let flexible = width - check - remove;
        let name = flexible * NAME_SHARE;
        Self {
            left: ui.cursor().min.x,
            width,
            check,
            name,
            value: flexible - name,
            remove,
            height,
        }
    }

    /// Where `column` starts and how wide it is.
    fn span(&self, column: Column) -> (f32, f32) {
        match column {
            Column::Check => (0.0, self.check),
            Column::Name => (self.check, self.name),
            Column::Value => (self.check + self.name, self.value),
            Column::Remove => (self.check + self.name + self.value, self.remove),
        }
    }

    /// The table's frame, and the lines between its rows and columns,
    /// from `top` to `bottom`.
    fn paint_lines(&self, ui: &egui::Ui, top: f32, bottom: f32) {
        let stroke = egui::Stroke::new(1.0, theme::palette(ui).border);
        let painter = ui.painter();
        let rect = egui::Rect::from_x_y_ranges(self.left..=self.left + self.width, top..=bottom);
        painter.rect_stroke(rect, theme::radius::SMALL, stroke, egui::StrokeKind::Inside);
        let mut y = top + self.height;
        while y < bottom - 1.0 {
            painter.hline(rect.x_range().shrink(1.0), y, stroke);
            y += self.height;
        }
        for column in [Column::Name, Column::Value] {
            let x = self.left + self.span(column).0;
            painter.vline(x, rect.y_range().shrink(1.0), stroke);
        }
        if self.remove > 0.0 {
            let x = self.left + self.span(Column::Remove).0;
            painter.vline(x, rect.y_range().shrink(1.0), stroke);
        }
    }
}

/// The titles of the columns, muted.
fn header(ui: &mut egui::Ui, columns: &Columns, strings: &Strings) {
    let cells = Row::new(ui, columns);
    let muted = theme::palette(ui).text_muted;
    for (column, title) in [
        (Column::Name, strings.column_name),
        (Column::Value, strings.column_value),
    ] {
        cells.cell(ui, column, true, |ui| {
            ui.add_space(f32::from(theme::cell_frame().inner_margin.left));
            ui.label(RichText::new(title).small().strong().color(muted));
        });
    }
    cells.end(ui);
}

/// A row of the table: its cells are drawn into it, then [`Row::end`]
/// takes its place, below the row before.
struct Row<'c> {
    columns: &'c Columns,
    rect: egui::Rect,
    /// Whether the pointer is over it, which lights it up.
    hovered: bool,
}

impl<'c> Row<'c> {
    fn new(ui: &mut egui::Ui, columns: &'c Columns) -> Self {
        let size = egui::vec2(columns.width, columns.height);
        let min = egui::pos2(columns.left, ui.cursor().min.y);
        let rect = egui::Rect::from_min_size(min, size);
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            let fill = ui
                .visuals()
                .widgets
                .hovered
                .weak_bg_fill
                .gamma_multiply(0.5);
            ui.painter().rect_filled(rect.shrink(1.0), 0, fill);
        }
        Self {
            columns,
            rect,
            hovered,
        }
    }

    /// Takes the row's place, so the next one goes under it. After the
    /// cells: the last one placed would else decide where the next row
    /// starts, and a centered button is shorter than the row.
    fn end(self, ui: &mut egui::Ui) {
        ui.allocate_rect(self.rect, egui::Sense::hover());
    }

    /// Draws `add` into `column`'s cell, centered in its height; dimmed
    /// unless `enabled`, and outlined with the accent while one of its
    /// fields has the focus.
    fn cell(
        &self,
        ui: &mut egui::Ui,
        column: Column,
        enabled: bool,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        let (x, width) = self.columns.span(column);
        let rect = egui::Rect::from_min_size(
            self.rect.min + egui::vec2(x, 0.0),
            egui::vec2(width, self.rect.height()),
        );
        // The checkbox and the button, centered in their cells; the fields
        // fill theirs.
        let rect = match column {
            Column::Check => egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(ui.spacing().icon_width, rect.height()),
            ),
            Column::Remove => {
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(width * 0.75))
            }
            Column::Name | Column::Value => rect,
        };
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
            if !enabled {
                ui.multiply_opacity(DISABLED_OPACITY);
            }
            add(ui);
        });
    }
}

/// A name field, with `names` suggested as it is typed; `label` is its
/// placeholder and accessible name. As a table's `cell`, it has no frame
/// and its text is monospace. Returns whether the name changed.
pub(crate) fn name_field(
    ui: &mut egui::Ui,
    name: &mut String,
    (id, width): (Id, f32),
    names: &[&str],
    label: &str,
    cell: bool,
) -> bool {
    let mut edit = egui::TextEdit::singleline(name)
        .id(id)
        .hint_text(label)
        .desired_width(width);
    if cell {
        edit = edit
            .frame(theme::cell_frame())
            .font(egui::TextStyle::Monospace)
            .desired_width(ui.available_width());
    }
    let field = ui.add(edit).accessible_name(label);
    if cell {
        outline_focused(ui, &field);
    }
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
