//! The environment editor: the list of environments on the left and the
//! chosen one on the right (name, color, what it extends and its variables),
//! plus a view of every variable the active request sees and where each
//! comes from.
//!
//! Edits go to a working copy, saved with one button; the files change only
//! then.

use std::collections::BTreeMap;

use apifast_domain::{Environment, Secret, ValueKind, Variable, VariableValue};
use apifast_vault::{Orphan, SecretKind};
use eframe::egui::{self, RichText};

use crate::atoms::{CloseButton, ColorSwatch, TemplateField, TemplateFieldTexts, VariableHints};
use crate::core::{SecretStatus, TabContext, VaultView};
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
    /// Store the value of a secret.
    Secret {
        /// Its environment.
        environment: String,
        /// Its variable.
        name: String,
        /// The value.
        value: Secret,
        /// Shared with the team rather than kept on this machine.
        shared: bool,
    },
    /// Show the value of a secret for a while.
    Reveal {
        /// Its environment.
        environment: String,
        /// Its variable.
        name: String,
    },
    /// Hide a value shown.
    Hide,
    /// Forget a stored value no secret uses.
    Forget(Orphan),
}

/// A value typed for a secret, stored when the environment is saved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Pending {
    text: String,
    shared: bool,
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
    /// Values typed for secrets of the working copy, by variable.
    secret_values: BTreeMap<String, Pending>,
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
        self.secret_values.clear();
    }

    /// Chooses `name`, which the files will have soon: a new environment.
    fn select_pending(&mut self, name: &str) {
        self.select(name);
        self.creating = Some(name.to_owned());
    }

    /// Whether the working copy has changes that are not saved, or values
    /// typed for secrets.
    pub fn is_dirty(&self) -> bool {
        self.working
            .as_ref()
            .is_some_and(|(work, base)| work != base)
            || self.secret_values.values().any(|p| !p.text.is_empty())
    }

    /// Draws the window, if open. `environments` are the files' contents;
    /// `context` is what the active request sees, if there is one.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        (environments, vault): (&[Environment], &VaultView),
        context: Option<&TabContext>,
        hints: &VariableHints,
        strings: &Strings,
    ) -> Vec<EnvironmentEvent> {
        if !self.open {
            return Vec::new();
        }
        self.follow(environments);
        let mut open = self.open;
        let mut events = Vec::new();
        egui::Window::new(strings.environments)
            .id(egui::Id::unique("environment-editor"))
            .open(&mut open)
            .default_size([920.0, 460.0])
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
                match self.view {
                    EnvironmentsView::Environments => {
                        events = self.environments(ui, (environments, vault), hints, strings);
                    }
                    EnvironmentsView::Variables => all_variables(ui, context, strings),
                }
            });
        self.open = open;
        events.extend(self.confirm_delete(ctx, strings));
        events
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
            self.secret_values.clear();
        }
    }

    fn environments(
        &mut self,
        ui: &mut egui::Ui,
        (environments, vault): (&[Environment], &VaultView),
        hints: &VariableHints,
        strings: &Strings,
    ) -> Vec<EnvironmentEvent> {
        let mut events = Vec::new();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH);
                events.extend(self.list(ui, environments, strings));
            });
            ui.separator();
            ui.vertical(|ui| {
                events.extend(self.details(ui, (environments, vault), hints, strings));
                events.extend(orphans(ui, &vault.orphans, strings));
            });
        });
        events
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
        (environments, vault): (&[Environment], &VaultView),
        hints: &VariableHints,
        strings: &Strings,
    ) -> Vec<EnvironmentEvent> {
        let dirty = self.is_dirty();
        let Some((work, base)) = self.working.as_mut() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let mut event = None;
        egui::Grid::new("environment-fields")
            .num_columns(2)
            .spacing([theme::space::M, theme::space::S])
            .show(ui, |ui| {
                ui.label(strings.column_name);
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
        let secrets = Secrets {
            environment: &base.name,
            vault,
            values: &mut self.secret_values,
        };
        events.extend(variables(ui, &mut work.variables, secrets, hints, strings));
        ui.add_space(theme::space::S);
        events.extend(buttons(
            ui,
            (work, base),
            (&mut self.secret_values, &mut self.deleting),
            dirty,
            strings,
        ));
        events.extend(event);
        events
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

/// Widths of the columns of the variables table.
struct Columns {
    check: f32,
    name: f32,
    value: f32,
    kind: f32,
    secret: f32,
}

impl Columns {
    fn new(ui: &egui::Ui) -> Self {
        let gap = theme::space::S;
        let check = ui.spacing().icon_width;
        let (kind, secret, close) = (90.0, 48.0, 18.0);
        let flexible =
            (ui.available_width() - check - kind - secret - close - 5.0 * gap).max(240.0);
        let name = flexible * 0.35;
        Self {
            check,
            name,
            value: flexible - name,
            kind,
            secret,
        }
    }
}

/// A cell of `width`, its contents centered on the row's line.
fn cell<R>(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
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

/// Save, Revert and Delete. Save writes the environment if it changed, and
/// the values typed for its secrets.
fn buttons(
    ui: &mut egui::Ui,
    (work, base): (&mut Environment, &mut Environment),
    (values, deleting): (&mut BTreeMap<String, Pending>, &mut Option<String>),
    dirty: bool,
    strings: &Strings,
) -> Vec<EnvironmentEvent> {
    let mut events = Vec::new();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(dirty, egui::Button::new(strings.save))
            .clicked()
        {
            if work != base {
                events.push(EnvironmentEvent::Save(work.clone()));
                *base = work.clone();
            }
            // Values typed for secrets that are still secrets.
            for (name, pending) in std::mem::take(values) {
                let secret = work.variables.iter().any(|v| v.secret && v.name == name);
                if secret && !pending.text.is_empty() {
                    events.push(EnvironmentEvent::Secret {
                        environment: base.name.clone(),
                        name,
                        value: Secret::new(pending.text),
                        shared: pending.shared,
                    });
                }
            }
        }
        if ui
            .add_enabled(dirty, egui::Button::new(strings.revert))
            .clicked()
        {
            *work = base.clone();
            values.clear();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(strings.delete).clicked() {
                *deleting = Some(base.name.clone());
            }
        });
    });
    events
}

/// What the table needs to show and edit the secrets of one environment.
struct Secrets<'a> {
    environment: &'a str,
    vault: &'a VaultView,
    values: &'a mut BTreeMap<String, Pending>,
}

/// The table of an environment's variables, with an empty row to add one.
/// Rows are laid out by hand, not with a grid: a field in a grid cell only
/// gets the width its column measured before, so it never grows.
fn variables(
    ui: &mut egui::Ui,
    variables: &mut Vec<Variable>,
    mut secrets: Secrets<'_>,
    hints: &VariableHints,
    strings: &Strings,
) -> Vec<EnvironmentEvent> {
    let texts = TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    };
    let id = ui.make_persistent_id("environment-variables");
    let columns = Columns::new(ui);
    let mut remove = None;
    let mut events = Vec::new();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::space::S;
        for (header, width) in [
            ("", columns.check),
            (strings.column_name, columns.name),
            (strings.column_value, columns.value),
            (strings.kind, columns.kind),
            (strings.secret, columns.secret),
        ] {
            cell(ui, width, |ui| ui.label(RichText::new(header).strong()));
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("environment-variables-scroll")
        .max_height(ui.available_height() - 40.0)
        .show(ui, |ui| {
            let rows = variables.len();
            for (index, variable) in variables.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::space::S;
                    let ids = (id, index);
                    let found = row(
                        ui,
                        variable,
                        ids,
                        &columns,
                        &mut secrets,
                        (hints, texts),
                        strings,
                    );
                    events.extend(found);
                    if ui.add(CloseButton::new(strings.remove_row)).clicked() {
                        remove = Some(index);
                    }
                });
            }
            // The empty row: typing a name adds the variable. Its field has
            // the id the new row's will have, so the focus stays.
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
                }
            });
        });
    if let Some(index) = remove {
        variables.remove(index);
    }
    events
}

fn status_text(status: SecretStatus, strings: &Strings) -> &str {
    match status {
        SecretStatus::Local => strings.secret_local,
        SecretStatus::Shared => strings.secret_shared,
        SecretStatus::Locked => strings.secret_locked,
        SecretStatus::Missing => strings.secret_missing,
    }
}

fn row(
    ui: &mut egui::Ui,
    variable: &mut Variable,
    (id, index): (egui::Id, usize),
    columns: &Columns,
    secrets: &mut Secrets<'_>,
    (hints, texts): (&VariableHints, TemplateFieldTexts<'_>),
    strings: &Strings,
) -> Option<EnvironmentEvent> {
    cell(ui, columns.check, |ui| {
        ui.checkbox(&mut variable.enabled, "")
            .accessible_name(strings.enabled)
            .on_hover_text(strings.enabled);
    });
    ui.add(
        egui::TextEdit::singleline(&mut variable.name)
            .id(id.with((index, 0)))
            .desired_width(columns.name),
    )
    .accessible_name(strings.name);
    let mut event = None;
    if variable.secret {
        event = secret_value(
            ui,
            variable,
            id.with((index, 1)),
            columns.value,
            secrets,
            strings,
        );
    } else {
        let value = variable
            .value
            .get_or_insert_with(|| VariableValue::text(""));
        TemplateField::new(&mut value.data, hints, texts)
            .id(id.with((index, 1)))
            .desired_width(columns.value)
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
            .width(columns.kind)
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
    cell(ui, columns.secret, |ui| {
        let mut secret = variable.secret;
        let changed = ui
            .add(egui::Checkbox::without_text(&mut secret))
            .accessible_name(strings.secret)
            .on_hover_text(strings.secret_hint)
            .changed();
        if changed {
            make_secret(variable, secret, secrets.values);
        }
    });
    event
}

/// Marks `variable` as secret or not. A value in the clear moves to the
/// values to store on save; the file keeps only the name.
fn make_secret(variable: &mut Variable, secret: bool, values: &mut BTreeMap<String, Pending>) {
    variable.secret = secret;
    if secret {
        let text = variable.value.take().map(|v| v.data).unwrap_or_default();
        if !text.is_empty() {
            values.insert(
                variable.name.clone(),
                Pending {
                    text,
                    shared: false,
                },
            );
        }
    } else {
        variable.value = Some(VariableValue::text(
            values
                .remove(&variable.name)
                .map(|p| p.text)
                .unwrap_or_default(),
        ));
    }
}

/// The value of a secret: a hidden field to type a new one, whether it is
/// shared, and a button to show the stored one for a while.
fn secret_value(
    ui: &mut egui::Ui,
    variable: &Variable,
    id: egui::Id,
    width: f32,
    secrets: &mut Secrets<'_>,
    strings: &Strings,
) -> Option<EnvironmentEvent> {
    let status = secrets.vault.status(secrets.environment, &variable.name);
    let revealed = (secrets.vault.revealed.as_ref())
        .filter(|r| r.environment == secrets.environment && r.name == variable.name);
    let mut event = None;
    cell(ui, width, |ui| {
        let buttons = 150.0;
        if let Some(revealed) = revealed {
            ui.add_sized(
                [width - buttons, ui.spacing().interact_size.y],
                egui::Label::new(RichText::new(revealed.value.expose()).monospace()).truncate(),
            );
        } else {
            // A new value goes where the stored one is, unless changed.
            let shared = matches!(status, SecretStatus::Shared | SecretStatus::Locked);
            let pending =
                (secrets.values.entry(variable.name.clone())).or_insert_with(|| Pending {
                    text: String::new(),
                    shared,
                });
            ui.add(
                egui::TextEdit::singleline(&mut pending.text)
                    .id(id)
                    .password(true)
                    .hint_text(status_text(status, strings))
                    .desired_width(width - buttons),
            )
            // The status in the name too, so screen readers say it.
            .accessible_name(format!(
                "{}: {}",
                strings.secret_value,
                status_text(status, strings)
            ))
            .on_hover_text(status_text(status, strings));
            ui.checkbox(&mut pending.shared, strings.shared)
                .on_hover_text(strings.shared_hint);
        }
        let stored = matches!(status, SecretStatus::Local | SecretStatus::Shared);
        if revealed.is_some() {
            if ui.button(strings.hide).clicked() {
                event = Some(EnvironmentEvent::Hide);
            }
        } else if ui
            .add_enabled(stored, egui::Button::new(strings.reveal))
            .clicked()
        {
            event = Some(EnvironmentEvent::Reveal {
                environment: secrets.environment.to_owned(),
                name: variable.name.clone(),
            });
        }
    });
    event
}

/// Stored values no secret variable uses, each with a button to forget it.
fn orphans(ui: &mut egui::Ui, orphans: &[Orphan], strings: &Strings) -> Option<EnvironmentEvent> {
    if orphans.is_empty() {
        return None;
    }
    let muted = theme::palette(ui).text_muted;
    let mut event = None;
    ui.add_space(theme::space::M);
    ui.label(RichText::new(strings.orphans_title).strong());
    ui.label(RichText::new(strings.orphans_hint).color(muted).small());
    for orphan in orphans {
        ui.horizontal(|ui| {
            let kind = match orphan.kind {
                SecretKind::Local => strings.secret_local,
                SecretKind::Shared => strings.secret_shared,
            };
            ui.label(
                RichText::new(format!("{} / {}", orphan.environment, orphan.name)).monospace(),
            );
            ui.label(RichText::new(kind).color(muted));
            if ui.button(strings.forget).clicked() {
                event = Some(EnvironmentEvent::Forget(orphan.clone()));
            }
        });
    }
    event
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
                    for header in [
                        strings.column_name,
                        strings.column_value,
                        strings.from,
                        strings.overrides,
                    ] {
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
