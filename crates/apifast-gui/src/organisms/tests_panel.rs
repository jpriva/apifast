//! The Tests tab of a request: its assertions and captures (docs/08), as
//! rows to edit, each with an empty row at the end that becomes a new one
//! as soon as something is typed in it.

use apifast_domain::{Assertion, Capture, CaptureScope, HttpRequest, Operator};
use eframe::egui::{self, Id, RichText};

use crate::atoms::{CloseButton, TemplateField, TemplateFieldTexts, VariableHints};
use crate::organisms::kv_table::name_field;
use crate::strings::Strings;
use crate::theme;

/// Expressions suggested while typing one.
const EXPRESSIONS: &[&str] = &[
    "res.status",
    "res.statusText",
    "res.headers.",
    "res.body",
    "res.body.",
    "res.responseTime",
    "res.size",
    "req.method",
    "req.url",
    "req.headers.",
    "req.body",
    "req.body.",
];

/// Height of the open operator menu: most of the 28 at once.
const OPERATORS_HEIGHT: f32 = 420.0;
/// Width of the operator and scope menus.
const MENU_WIDTH: f32 = 120.0;
/// The least the text fields of a row take together, however narrow.
const MIN_FIELDS: f32 = 200.0;
/// Share of the expression field in the text fields of a row.
const EXPRESSION_SHARE: f32 = 0.45;

/// Draws the tab over `request`, which it edits in place. Returns whether
/// the user changed something.
pub fn tests_panel(
    ui: &mut egui::Ui,
    request: &mut HttpRequest,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let muted = theme::palette(ui).text_muted;
    ui.label(RichText::new(strings.tests_hint).color(muted).small());
    ui.add_space(theme::space::S);
    ui.label(RichText::new(strings.assertions).strong());
    let mut changed = assertions(ui, &mut request.assertions, hints, strings);
    ui.add_space(theme::space::M);
    ui.label(RichText::new(strings.captures).strong());
    changed |= captures(ui, &mut request.captures, strings);
    changed
}

/// Widths of the two text fields of a row, beside a checkbox, a menu and a
/// remove button.
fn widths(ui: &egui::Ui) -> (f32, f32) {
    let gap = theme::space::S;
    let fixed = ui.spacing().icon_width + MENU_WIDTH + ui.spacing().interact_size.y.min(18.0);
    let flexible = (ui.available_width() - fixed - 4.0 * gap).max(MIN_FIELDS);
    let first = flexible * EXPRESSION_SHARE;
    (first, flexible - first)
}

fn texts(strings: &Strings, placeholder: &'static str) -> TemplateFieldTexts<'static> {
    TemplateFieldTexts {
        placeholder,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
        edit: strings.variable_edit,
    }
}

fn assertions(
    ui: &mut egui::Ui,
    rows: &mut Vec<Assertion>,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let id = ui.make_persistent_id("assertions");
    let (expression_width, value_width) = widths(ui);
    let mut changed = false;
    let mut remove = None;
    for (index, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            changed |= ui
                .checkbox(&mut row.enabled, "")
                .accessible_name(strings.enabled)
                .on_hover_text(strings.enabled)
                .changed();
            changed |= expression(
                ui,
                &mut row.expression,
                id.with((index, 0)),
                expression_width,
                strings,
            );
            changed |= operator(ui, row, id.with((index, "operator")), strings);
            changed |= expected(ui, row, id.with((index, 1)), value_width, hints, strings);
            if ui.add(CloseButton::new(strings.remove_row)).clicked() {
                remove = Some(index);
            }
        });
    }
    // The empty row, with the ids the row will have once added.
    let index = rows.len();
    let mut text = String::new();
    let typed = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            ui.add_enabled(false, egui::Checkbox::without_text(&mut true))
                .accessible_name(strings.enabled);
            expression(
                ui,
                &mut text,
                id.with((index, 0)),
                expression_width,
                strings,
            )
        })
        .inner;
    if typed {
        rows.push(Assertion::new(text, Operator::parse("eq"), Some("")));
        changed = true;
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    changed
}

fn expression(ui: &mut egui::Ui, text: &mut String, id: Id, width: f32, strings: &Strings) -> bool {
    name_field(
        ui,
        text,
        (id, width),
        EXPRESSIONS,
        strings.expression,
        false,
    )
}

/// The operator menu. Switching to an operator that takes no value drops
/// the value; switching back starts an empty one.
fn operator(ui: &mut egui::Ui, row: &mut Assertion, id: Id, strings: &Strings) -> bool {
    let mut chosen = row.operator.clone();
    egui::ComboBox::from_id_salt(id)
        .width(MENU_WIDTH)
        .height(OPERATORS_HEIGHT)
        .selected_text(RichText::new(row.operator.as_str()).monospace())
        .show_ui(ui, |ui| {
            for operator in Operator::all() {
                let label = RichText::new(operator.as_str()).monospace();
                ui.selectable_value(&mut chosen, operator, label);
            }
        })
        .response
        .on_hover_text(strings.operator);
    if chosen == row.operator {
        return false;
    }
    row.value = if chosen.takes_value() {
        Some(row.value.take().unwrap_or_default())
    } else {
        None
    };
    row.operator = chosen;
    true
}

fn expected(
    ui: &mut egui::Ui,
    row: &mut Assertion,
    id: Id,
    width: f32,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let Some(value) = &mut row.value else {
        let muted = theme::palette(ui).text_muted;
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(strings.no_expected).color(muted).italics());
            },
        );
        return false;
    };
    TemplateField::new(value, hints, texts(strings, strings.expected))
        .id(id)
        .desired_width(width)
        .accessible_name(strings.expected)
        .show(ui)
        .changed()
}

const SCOPES: [&str; 4] = ["runtime", "request", "environment", "collection"];

fn captures(ui: &mut egui::Ui, rows: &mut Vec<Capture>, strings: &Strings) -> bool {
    let id = ui.make_persistent_id("captures");
    let (expression_width, variable_width) = widths(ui);
    let mut changed = false;
    let mut remove = None;
    for (index, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            changed |= ui
                .checkbox(&mut row.enabled, "")
                .accessible_name(strings.enabled)
                .on_hover_text(strings.enabled)
                .changed();
            changed |= expression(
                ui,
                &mut row.expression,
                id.with((index, 0)),
                expression_width,
                strings,
            );
            changed |= variable(
                ui,
                &mut row.variable,
                id.with((index, 1)),
                variable_width,
                strings,
            );
            changed |= scope(ui, &mut row.scope, id.with((index, "scope")), strings);
            if ui.add(CloseButton::new(strings.remove_row)).clicked() {
                remove = Some(index);
            }
        });
    }
    let index = rows.len();
    let (mut text, mut name) = (String::new(), String::new());
    let typed = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            ui.add_enabled(false, egui::Checkbox::without_text(&mut true))
                .accessible_name(strings.enabled);
            let a = expression(
                ui,
                &mut text,
                id.with((index, 0)),
                expression_width,
                strings,
            );
            let b = variable(ui, &mut name, id.with((index, 1)), variable_width, strings);
            a || b
        })
        .inner;
    if typed {
        rows.push(Capture::new(text, name));
        changed = true;
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    changed
}

fn variable(ui: &mut egui::Ui, text: &mut String, id: Id, width: f32, strings: &Strings) -> bool {
    ui.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .hint_text(strings.capture_variable)
            .desired_width(width),
    )
    .accessible_name(strings.capture_variable)
    .changed()
}

fn scope(ui: &mut egui::Ui, scope: &mut CaptureScope, id: Id, strings: &Strings) -> bool {
    let mut chosen = scope.clone();
    egui::ComboBox::from_id_salt(id)
        .width(MENU_WIDTH)
        .selected_text(scope.as_str().to_owned())
        .show_ui(ui, |ui| {
            for name in SCOPES {
                ui.selectable_value(&mut chosen, CaptureScope::parse(name), name);
            }
        })
        .response
        .on_hover_text(strings.capture_scope);
    if chosen == *scope {
        false
    } else {
        *scope = chosen;
        true
    }
}
