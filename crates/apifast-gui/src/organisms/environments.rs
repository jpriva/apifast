//! The environment editor: the list of environments on the left and the
//! chosen one on the right (name, color, what it extends and its variables),
//! plus a view of every variable the active request sees and where each
//! comes from.
//!
//! Edits go to a working copy, saved with one button; the files change only
//! then.

use apifast_domain::{Environment, ValueKind, Variable, VariableValue};
use eframe::egui::{self, RichText};

use crate::atoms::{ColorSwatch, TemplateField, TemplateFieldTexts, VariableHints};
use crate::core::TabContext;
use crate::strings::Strings;
use crate::theme;

/// Width of the list of environments, in points.
const LIST_WIDTH: f32 = 160.0;

/// What the editor asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentEvent {
    /// Create an empty environment with this name.
    Create(String),
    /// Rename an environment.
    Rename {
        /// Its name.
        from: String,
        /// The new name.
        to: String,
    },
    /// Save these contents.
    Save(Environment),
    /// Delete an environment.
    Delete(String),
}

/// Which part of the editor is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EnvironmentsView {
    /// The environments and their variables.
    #[default]
    Environments,
    /// Every variable the active request sees.
    Variables,
}

/// The editor's state.
#[derive(Debug, Default)]
pub struct EnvironmentEditor {
    /// Whether the window is open.
    pub open: bool,
    /// Which part is shown.
    pub view: EnvironmentsView,
    /// The environment chosen in the list.
    selected: Option<String>,
    /// The working copy, and the contents it started from.
    working: Option<(Environment, Environment)>,
    /// The name field, applied on Enter.
    name: String,
    /// The name of a new environment.
    new_name: String,
    /// Asking to confirm the deletion of this environment.
    deleting: Option<String>,
    /// A rename asked for and not seen in the files yet: the selection and
    /// the working copy follow it once it is.
    renaming: Option<(String, String)>,
    /// An environment asked for and not in the files yet: the selection
    /// waits for it.
    creating: Option<String>,
}

impl EnvironmentEditor {
    /// The environment chosen in the list.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Chooses `name` in the list.
    pub fn select(&mut self, name: &str) {
        self.selected = Some(name.to_owned());
        self.working = None;
    }

    /// Chooses `name`, which the files will have soon: a new environment.
    fn select_pending(&mut self, name: &str) {
        self.select(name);
        self.creating = Some(name.to_owned());
    }

    /// Whether the working copy has changes that are not saved.
    pub fn is_dirty(&self) -> bool {
        self.working
            .as_ref()
            .is_some_and(|(work, base)| work != base)
    }

    /// Draws the window, if open. `environments` are the files' contents;
    /// `context` is what the active request sees, if there is one.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        environments: &[Environment],
        context: Option<&TabContext>,
        hints: &VariableHints,
        strings: &Strings,
    ) -> Option<EnvironmentEvent> {
        if !self.open {
            return None;
        }
        self.follow(environments);
        let mut open = self.open;
        let mut event = None;
        egui::Window::new(strings.environments)
            .id(egui::Id::unique("environment-editor"))
            .open(&mut open)
            .default_size([720.0, 420.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.view,
                        EnvironmentsView::Environments,
                        strings.environments,
                    );
                    ui.selectable_value(
                        &mut self.view,
                        EnvironmentsView::Variables,
                        strings.all_variables,
                    );
                });
                ui.separator();
                event = match self.view {
                    EnvironmentsView::Environments => {
                        self.environments(ui, environments, hints, strings)
                    }
                    EnvironmentsView::Variables => {
                        all_variables(ui, context, strings);
                        None
                    }
                };
            });
        self.open = open;
        event.or_else(|| self.confirm_delete(ctx, strings))
    }

    /// Keeps the selection and the working copy in step with the files: a
    /// selection that is gone moves to the first environment, and a copy
    /// without changes follows what the files say now.
    fn follow(&mut self, environments: &[Environment]) {
        let exists = |name: &str| environments.iter().any(|e| e.name == name);
        if let Some((from, to)) = &self.renaming
            && !exists(from)
            && exists(to)
        {
            self.selected = Some(to.clone());
            if let Some((work, base)) = &mut self.working {
                work.name.clone_from(to);
                base.name.clone_from(to);
            }
            self.renaming = None;
        }
        if let Some(name) = &self.creating {
            if exists(name) {
                self.creating = None;
            } else if self.selected.as_ref() == Some(name) {
                self.working = None;
                return;
            }
        }
        if self.selected.as_deref().is_none_or(|name| !exists(name)) {
            self.selected = environments.first().map(|e| e.name.clone());
            self.working = None;
        }
        let Some(current) =
            (self.selected.as_ref()).and_then(|name| environments.iter().find(|e| e.name == *name))
        else {
            self.working = None;
            return;
        };
        let stale = match &self.working {
            Some((work, base)) => work == base && base != current,
            None => true,
        };
        if stale {
            self.working = Some((current.clone(), current.clone()));
            self.name.clone_from(&current.name);
        }
    }

    fn environments(
        &mut self,
        ui: &mut egui::Ui,
        environments: &[Environment],
        hints: &VariableHints,
        strings: &Strings,
    ) -> Option<EnvironmentEvent> {
        let mut event = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH);
                event = self.list(ui, environments, strings);
            });
            ui.separator();
            ui.vertical(|ui| {
                let found = self.details(ui, environments, hints, strings);
                event = event.take().or(found);
            });
        });
        event
    }

    fn list(
        &mut self,
        ui: &mut egui::Ui,
        environments: &[Environment],
        strings: &Strings,
    ) -> Option<EnvironmentEvent> {
        for environment in environments {
            let selected = self.selected.as_deref() == Some(environment.name.as_str());
            ui.horizontal(|ui| {
                ui.add(ColorSwatch::new(environment.color.as_deref()));
                if ui.selectable_label(selected, &environment.name).clicked() && !selected {
                    self.select(&environment.name);
                }
            });
        }
        if environments.is_empty() {
            ui.label(RichText::new(strings.no_environments).color(theme::palette(ui).text_muted));
        }
        ui.separator();
        let field = ui
            .add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text(strings.new_environment)
                    .desired_width(f32::INFINITY),
            )
            .accessible_name(strings.new_environment);
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let add = ui
            .add_enabled(
                !self.new_name.trim().is_empty(),
                egui::Button::new(strings.add),
            )
            .clicked();
        if (entered || add) && !self.new_name.trim().is_empty() {
            let name = std::mem::take(&mut self.new_name).trim().to_owned();
            self.select_pending(&name);
            return Some(EnvironmentEvent::Create(name));
        }
        None
    }

    fn details(
        &mut self,
        ui: &mut egui::Ui,
        environments: &[Environment],
        hints: &VariableHints,
        strings: &Strings,
    ) -> Option<EnvironmentEvent> {
        let dirty = self.is_dirty();
        let (work, base) = self.working.as_mut()?;
        let mut event = None;
        egui::Grid::new("environment-fields")
            .num_columns(2)
            .spacing([theme::space::M, theme::space::S])
            .show(ui, |ui| {
                ui.label(strings.name);
                let field = ui
                    .add(egui::TextEdit::singleline(&mut self.name).desired_width(240.0))
                    .accessible_name(strings.environment_name)
                    .on_hover_text(strings.rename_hint);
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let to = self.name.trim().to_owned();
                if entered && to != base.name && !to.is_empty() {
                    self.renaming = Some((base.name.clone(), to.clone()));
                    event = Some(EnvironmentEvent::Rename {
                        from: base.name.clone(),
                        to,
                    });
                }
                ui.end_row();

                ui.label(strings.color);
                ui.horizontal(|ui| {
                    ui.add(ColorSwatch::new(work.color.as_deref()));
                    let mut color = work.color.clone().unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut color)
                                .hint_text("#22aa55")
                                .desired_width(90.0),
                        )
                        .accessible_name(strings.color)
                        .changed()
                    {
                        work.color = Some(color).filter(|c| !c.trim().is_empty());
                    }
                });
                ui.end_row();

                ui.label(strings.extends);
                let others: Vec<&str> = environments
                    .iter()
                    .map(|e| e.name.as_str())
                    .filter(|name| *name != base.name)
                    .collect();
                egui::ComboBox::from_id_salt("environment-extends")
                    .selected_text(work.extends.as_deref().unwrap_or(strings.extends_none))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut work.extends, None, strings.extends_none);
                        for name in others {
                            ui.selectable_value(&mut work.extends, Some(name.to_owned()), name);
                        }
                    })
                    .response
                    .on_hover_text(strings.extends);
                ui.end_row();
            });
        ui.add_space(theme::space::S);
        variables(ui, &mut work.variables, hints, strings);
        ui.add_space(theme::space::S);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(dirty, egui::Button::new(strings.save))
                .clicked()
            {
                event = Some(EnvironmentEvent::Save(work.clone()));
                *base = work.clone();
            }
            if ui
                .add_enabled(dirty, egui::Button::new(strings.revert))
                .clicked()
            {
                *work = base.clone();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(strings.delete).clicked() {
                    self.deleting = Some(base.name.clone());
                }
            });
        });
        event
    }

    fn confirm_delete(
        &mut self,
        ctx: &egui::Context,
        strings: &Strings,
    ) -> Option<EnvironmentEvent> {
        let name = self.deleting.clone()?;
        let mut event = None;
        let modal = egui::Modal::new(egui::Id::unique("delete-environment")).show(ctx, |ui| {
            ui.label(format!("{} `{name}`?", strings.delete_environment));
            ui.horizontal(|ui| {
                if ui.button(strings.delete).clicked() {
                    event = Some(EnvironmentEvent::Delete(name.clone()));
                    self.deleting = None;
                }
                if ui.button(strings.cancel).clicked() {
                    self.deleting = None;
                }
            });
        });
        if modal.should_close() {
            self.deleting = None;
        }
        event
    }
}

fn kind_label(kind: ValueKind, strings: &Strings) -> &str {
    match kind {
        ValueKind::String => strings.kind_text,
        ValueKind::Number => strings.kind_number,
        ValueKind::Bool => strings.kind_bool,
        ValueKind::Object => strings.kind_object,
    }
}

/// The table of an environment's variables, with an empty row to add one.
fn variables(
    ui: &mut egui::Ui,
    variables: &mut Vec<Variable>,
    hints: &VariableHints,
    strings: &Strings,
) {
    let texts = TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    };
    let id = ui.make_persistent_id("environment-variables");
    let mut remove = None;
    egui::ScrollArea::vertical()
        .id_salt("environment-variables-scroll")
        .max_height(ui.available_height() - 40.0)
        .show(ui, |ui| {
            egui::Grid::new(id)
                .num_columns(6)
                .striped(true)
                .spacing([theme::space::S, theme::space::XS])
                .show(ui, |ui| {
                    for header in [
                        "",
                        strings.name,
                        strings.value,
                        strings.kind,
                        strings.secret,
                        "",
                    ] {
                        ui.label(RichText::new(header).strong());
                    }
                    ui.end_row();
                    let rows = variables.len();
                    for (index, variable) in variables.iter_mut().enumerate() {
                        row(ui, variable, (id, index), hints, texts, strings);
                        if ui
                            .small_button("×")
                            .accessible_name(strings.remove_row)
                            .on_hover_text(strings.remove_row)
                            .clicked()
                        {
                            remove = Some(index);
                        }
                        ui.end_row();
                    }
                    // The empty row: typing a name adds the variable. Its
                    // field has the id the new row's will have, so the focus
                    // stays.
                    ui.label("");
                    let mut name = String::new();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut name)
                                .id(id.with((rows, 0)))
                                .hint_text(strings.new_variable)
                                .desired_width(160.0),
                        )
                        .accessible_name(strings.new_variable)
                        .changed()
                    {
                        variables.push(Variable::text(name, ""));
                    }
                    ui.end_row();
                });
        });
    if let Some(index) = remove {
        variables.remove(index);
    }
}

fn row(
    ui: &mut egui::Ui,
    variable: &mut Variable,
    (id, index): (egui::Id, usize),
    hints: &VariableHints,
    texts: TemplateFieldTexts<'_>,
    strings: &Strings,
) {
    ui.checkbox(&mut variable.enabled, "")
        .accessible_name(strings.enabled)
        .on_hover_text(strings.enabled);
    ui.add(
        egui::TextEdit::singleline(&mut variable.name)
            .id(id.with((index, 0)))
            .desired_width(160.0),
    )
    .accessible_name(strings.name);
    if variable.secret {
        ui.label(RichText::new(strings.secret_in_vault).color(theme::palette(ui).text_muted));
    } else {
        let value = variable
            .value
            .get_or_insert_with(|| VariableValue::text(""));
        TemplateField::new(&mut value.data, hints, texts)
            .id(id.with((index, 1)))
            .desired_width(260.0)
            .accessible_name(strings.value)
            .show(ui);
    }
    let kind = variable
        .value
        .as_ref()
        .map_or(ValueKind::String, |v| v.kind);
    ui.add_enabled_ui(!variable.secret, |ui| {
        egui::ComboBox::from_id_salt(id.with((index, 2)))
            .selected_text(kind_label(kind, strings))
            .width(80.0)
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
                    {
                        value.kind = option;
                    }
                }
            })
            .response
            .on_hover_text(strings.kind);
    });
    // Secrets live in the vault, which arrives later: marking one here
    // would only drop its value from the file.
    let mut secret = variable.secret;
    ui.add_enabled(false, egui::Checkbox::without_text(&mut secret))
        .accessible_name(strings.secret)
        .on_disabled_hover_text(strings.secret_later);
}

/// Every variable the active request sees: its value, where it comes from
/// and the scopes it overrides.
fn all_variables(ui: &mut egui::Ui, context: Option<&TabContext>, strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    let Some(context) = context else {
        ui.label(RichText::new(strings.no_request).color(muted));
        return;
    };
    if context.variables.is_empty() {
        ui.label(RichText::new(strings.no_variables).color(muted));
        return;
    }
    ui.label(RichText::new(strings.all_variables_hint).color(muted));
    egui::ScrollArea::vertical()
        .id_salt("all-variables")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("all-variables-grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    for header in [strings.name, strings.value, strings.from, strings.overrides] {
                        ui.label(RichText::new(header).strong());
                    }
                    ui.end_row();
                    for (name, variable) in context.variables.iter() {
                        ui.label(RichText::new(name).monospace());
                        let value = match (&variable.value, variable.secret) {
                            (Some(value), false) => value.data.as_str(),
                            _ => strings.secret_hidden,
                        };
                        ui.label(RichText::new(value).monospace());
                        ui.label(strings.origin(&variable.origin, &context.folders));
                        let overridden: Vec<String> = (variable.overridden.iter())
                            .map(|origin| strings.origin(origin, &context.folders))
                            .collect();
                        ui.label(RichText::new(overridden.join(", ")).color(muted));
                        ui.end_row();
                    }
                });
        });
}
