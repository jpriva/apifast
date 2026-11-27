//! The editor of one `{{variable}}`, opened from a field (Ctrl+click, or
//! the shortcut with the text cursor in it), as in Bruno: what the
//! variable is now, a value, and where to save it: the request, one of its
//! folders, the active environment or the collection.
//!
//! It only says what to save; the screen turns that into a command.

use apifast_domain::{ValueKind, Variable, VariableValue};
use eframe::egui::{self, RichText};

use crate::atoms::VariableHint;
use crate::strings::Strings;
use crate::theme;

/// Where a variable can be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableHome {
    /// The request, in its draft: saved with it.
    Request,
    /// A folder of the request, by its path in the collection.
    Folder(String),
    /// The environment of this name.
    Environment(String),
    /// The collection.
    Collection,
}

/// One place to save the variable in, as the editor offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeChoice {
    /// The place.
    pub home: VariableHome,
    /// How the editor names it.
    pub label: String,
    /// What the place defines for the variable now, if anything.
    pub current: Option<Variable>,
}

/// What the user chose to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableEdit {
    /// The variable.
    pub name: String,
    /// Where.
    pub home: VariableHome,
    /// Its new value.
    pub value: VariableValue,
}

#[derive(Debug)]
struct Editing {
    name: String,
    now: Option<VariableHint>,
    choices: Vec<HomeChoice>,
    chosen: usize,
    value: String,
}

/// The window.
#[derive(Debug, Default)]
pub struct VariableEditor {
    editing: Option<Editing>,
    /// The shortcut asked for the variable under the text cursor.
    pub at_cursor: bool,
}

/// `variable` set to `value` in `list`, enabled, or added to it. A value
/// keeps the kind it had.
pub fn set_variable(list: &mut Vec<Variable>, name: &str, value: VariableValue) {
    match list.iter_mut().find(|v| v.name == name) {
        Some(variable) => {
            variable.value = Some(value);
            variable.enabled = true;
        }
        None => list.push(Variable {
            name: name.to_owned(),
            value: Some(value),
            enabled: true,
            secret: false,
        }),
    }
}

impl VariableEditor {
    /// Opens on the variable `name`, which now is `now` (`None` when not
    /// defined), offering `choices` with `chosen` picked. The value starts
    /// as the chosen place's own, or else `fallback`.
    pub fn open(
        &mut self,
        (name, now): (String, Option<VariableHint>),
        choices: Vec<HomeChoice>,
        chosen: usize,
        fallback: String,
    ) {
        if choices.is_empty() {
            return;
        }
        let chosen = chosen.min(choices.len() - 1);
        let value = own_value(choices.get(chosen)).unwrap_or(fallback);
        self.editing = Some(Editing {
            name,
            now,
            choices,
            chosen,
            value,
        });
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.editing.is_some()
    }

    /// Draws the window. Returns what to save, once the user saves.
    pub fn show(&mut self, ctx: &egui::Context, strings: &Strings) -> Option<VariableEdit> {
        let editing = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        let mut cancelled = false;
        let title = format!("{{{{{}}}}}", editing.name);
        crate::molecules::window(
            ctx,
            egui::Window::new(&title)
                .id(egui::Id::unique("variable-editor"))
                .default_width(420.0)
                .collapsible(false),
            (&title, strings.close_window),
            &mut open,
            |ui| {
                let muted = theme::palette(ui).text_muted;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(strings.variable_now).strong());
                    if let Some(hint) = &editing.now {
                        ui.label(RichText::new(hint.value.as_str()).monospace());
                        ui.label(RichText::new(hint.origin.as_str()).color(muted));
                    } else {
                        let missing = theme::palette(ui).variable_missing;
                        ui.label(RichText::new(strings.variable_missing).color(missing));
                    }
                });
                ui.add_space(theme::space::S);
                let before = editing.chosen;
                egui::ComboBox::from_id_salt("variable-editor-home")
                    .selected_text(editing.choices[editing.chosen].label.as_str())
                    .show_ui(ui, |ui| {
                        for (index, choice) in editing.choices.iter().enumerate() {
                            ui.selectable_value(&mut editing.chosen, index, choice.label.as_str());
                        }
                    })
                    .response
                    .on_hover_text(strings.variable_save_in)
                    .accessible_name(strings.variable_save_in);
                if editing.chosen != before {
                    let choice = editing.choices.get(editing.chosen);
                    if let Some(value) = own_value(choice) {
                        editing.value = value;
                    }
                }
                let choice = &editing.choices[editing.chosen];
                let secret = choice.current.as_ref().is_some_and(|v| v.secret);
                if secret {
                    let why = if matches!(choice.home, VariableHome::Environment(_)) {
                        strings.variable_secret_in_environments
                    } else {
                        strings.variable_secret_elsewhere
                    };
                    ui.label(RichText::new(why).color(muted));
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut editing.value)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(strings.value)
                            .desired_width(f32::INFINITY),
                    )
                    .accessible_name(&title);
                }
                ui.add_space(theme::space::S);
                ui.horizontal(|ui| {
                    let save = egui::Button::new(RichText::new(strings.save).strong());
                    if ui.add_enabled(!secret, save).clicked() {
                        let kind = (choice.current.as_ref())
                            .and_then(|v| v.value.as_ref())
                            .map_or(ValueKind::String, |v| v.kind);
                        saved = Some(VariableEdit {
                            name: editing.name.clone(),
                            home: choice.home.clone(),
                            value: VariableValue {
                                kind,
                                data: editing.value.clone(),
                            },
                        });
                    }
                    if ui.button(strings.cancel).clicked() {
                        cancelled = true;
                    }
                });
            },
        );
        if !open || cancelled || saved.is_some() {
            self.editing = None;
        }
        saved
    }
}

/// The value `choice` defines, unless it is a secret.
fn own_value(choice: Option<&HomeChoice>) -> Option<String> {
    let variable = choice?.current.as_ref()?;
    if variable.secret {
        return None;
    }
    Some(
        variable
            .value
            .as_ref()
            .map(|v| v.data.clone())
            .unwrap_or_default(),
    )
}
