//! The main screen: collection tree, environment, tabs, URL bar and
//! response.

use std::sync::Arc;

use apifast_app::RequestPath;
use apifast_domain::{HttpRequest, Origin};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use crate::atoms::{HintsCache, VariableHint, VariableHints};
use crate::core::{Command, ResponseState, Snapshot, Tab, TabVariables};
use crate::layout;
use crate::molecules::EnvironmentSelector;
use crate::organisms::{ResponseView, TabBar, TabEvent, TreeEvent, TreeView, UrlBar, UrlBarEvent};
use crate::strings::Strings;
use crate::theme;

const SEND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const CANCEL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_ALL: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);

/// The state the main screen keeps between frames.
#[derive(Debug, Default)]
pub struct MainScreen {
    tree: TreeView,
    tabs: TabBar,
    response: ResponseView,
    editor: Option<Editor>,
    hints: HintsCache,
}

/// The interface's own copy of the active tab's contents. Edits change it
/// at once and are sent to the core; it is replaced from the snapshot only
/// when another tab is shown or the core changes the contents itself (a
/// new [`Tab::revision`]), so keys typed before the core answers are kept.
#[derive(Debug)]
struct Editor {
    path: RequestPath,
    revision: u64,
    request: HttpRequest,
}

impl Editor {
    fn follows(&self, tab: &Tab) -> bool {
        self.path == tab.path && self.revision == tab.revision
    }
}

impl MainScreen {
    /// Draws `snapshot` and returns the commands the user asked for.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        strings: &Strings,
    ) -> Vec<Command> {
        let Some(collection) = &snapshot.collection else {
            return Vec::new();
        };
        let active = snapshot.active_tab();
        let hints = self.follow(active, strings);
        let sending = active.is_some_and(|tab| tab.response == ResponseState::Sending);
        let selected = snapshot.active.as_ref();
        let (tree, tabs, response, editor) = (
            &mut self.tree,
            &mut self.tabs,
            &mut self.response,
            &mut self.editor,
        );

        let (environment, tree_event, (tab_event, url_event)) = layout::workbench(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&collection.tree.collection.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        EnvironmentSelector::new(
                            &snapshot.environments,
                            snapshot.environment.as_deref(),
                            strings.no_environment,
                            strings.environment,
                        )
                        .show(ui)
                    })
                    .inner
                })
                .inner
            },
            |ui| tree.show(ui, &collection.tree, snapshot.version, selected, strings),
            |ui| {
                let tab_event = tabs.show(ui, &snapshot.tabs, selected, strings);
                ui.separator();
                if let Some(error) = &snapshot.error {
                    ui.label(RichText::new(error.as_str()).color(theme::palette(ui).danger));
                    ui.add_space(theme::space::M);
                }
                let (Some(tab), Some(editor)) = (active, editor.as_mut()) else {
                    let muted = theme::palette(ui).text_muted;
                    ui.label(RichText::new(strings.no_request).color(muted));
                    return (tab_event, None);
                };
                let request = &mut editor.request;
                let event = UrlBar::new(
                    &mut request.method,
                    &mut request.url,
                    &hints,
                    sending,
                    strings,
                )
                .show(ui);
                ui.separator();
                response.show(ui, &tab.response, strings);
                (tab_event, event)
            },
        );

        let mut commands = Vec::new();
        if let Some(choice) = environment {
            commands.push(Command::SelectEnvironment(choice));
        }
        if let Some(event) = tree_event {
            commands.push(tree_command(event));
        }
        if let (Some(UrlBarEvent::Edited), Some(editor)) = (url_event, &self.editor) {
            commands.push(Command::Edit {
                path: editor.path.clone(),
                request: Box::new(editor.request.clone()),
            });
        }
        let tab_event = tab_event.or_else(|| self.close_shortcut(ui, active));
        match tab_event {
            Some(TabEvent::Activate(path)) => commands.push(Command::ActivateTab(path)),
            Some(TabEvent::Close(path)) => commands.push(Command::CloseTab(path)),
            Some(TabEvent::SaveAndClose(path)) => {
                commands.push(Command::Save(path.clone()));
                commands.push(Command::CloseTab(path));
            }
            None => {}
        }
        commands.extend(shortcuts(ui, active, sending));
        match url_event {
            Some(UrlBarEvent::Send) => commands.push(Command::Send),
            Some(UrlBarEvent::Cancel) => commands.push(Command::Cancel),
            Some(UrlBarEvent::Edited) | None => {}
        }
        commands
    }

    /// Keeps the editor on the active tab, and returns the hints for its
    /// variables.
    fn follow(&mut self, active: Option<&Tab>, strings: &Strings) -> Arc<VariableHints> {
        match (active, &self.editor) {
            (Some(tab), Some(editor)) if editor.follows(tab) => {}
            (Some(tab), _) => {
                self.editor = Some(Editor {
                    path: tab.path.clone(),
                    revision: tab.revision,
                    request: tab.request().clone(),
                });
            }
            (None, _) => self.editor = None,
        }
        match active {
            Some(tab) => {
                let variables = &tab.variables;
                self.hints
                    .get(variables.revision, || hints(variables, strings))
            }
            None => Arc::default(),
        }
    }

    /// `Ctrl+W` closes the active tab, asking first if it has changes.
    fn close_shortcut(&mut self, ui: &egui::Ui, active: Option<&Tab>) -> Option<TabEvent> {
        let tab = active?;
        if ui.input_mut(|i| i.consume_shortcut(&CLOSE_TAB)) {
            self.tabs.request_close(tab)
        } else {
            None
        }
    }
}

/// What the fields show about each variable: its value (hidden if secret)
/// and its origin, in the interface's language.
fn hints(variables: &TabVariables, strings: &Strings) -> VariableHints {
    variables
        .resolved
        .iter()
        .map(|(name, variable)| {
            let value = match (&variable.value, variable.secret) {
                (Some(value), false) => value.data.clone(),
                _ => strings.secret_hidden.to_owned(),
            };
            let origin = match &variable.origin {
                Origin::Default => strings.origin_default.to_owned(),
                Origin::Global => strings.origin_global.to_owned(),
                Origin::Collection => strings.origin_collection.to_owned(),
                Origin::Environment(env) => format!("{} {env}", strings.origin_environment),
                Origin::Folder(index) => {
                    let folder = variables.folders.get(*index).map_or("", String::as_str);
                    format!("{} {folder}", strings.origin_folder)
                }
                Origin::Request => strings.origin_request.to_owned(),
                Origin::Runtime => strings.origin_runtime.to_owned(),
                Origin::Prompt => strings.origin_prompt.to_owned(),
                // A scope added after this version: say nothing wrong.
                _ => String::new(),
            };
            (name.to_owned(), VariableHint { value, origin })
        })
        .collect()
}

fn tree_command(event: TreeEvent) -> Command {
    match event {
        TreeEvent::RequestSelected(path) => Command::OpenRequest(path),
        TreeEvent::CreateRequest { folder, name } => Command::CreateRequest { folder, name },
        TreeEvent::CreateFolder { parent, name } => Command::CreateFolder { parent, name },
        TreeEvent::Rename { item, name } => Command::Rename { item, name },
        TreeEvent::Delete(item) => Command::Delete(item),
        TreeEvent::Move { item, placement } => Command::Move { item, placement },
    }
}

/// `Ctrl+Enter` sends and `Esc` cancels; `Ctrl+S` saves the active tab and
/// `Ctrl+Shift+S` every tab.
fn shortcuts(ui: &egui::Ui, active: Option<&Tab>, sending: bool) -> Vec<Command> {
    let Some(tab) = active else {
        return Vec::new();
    };
    ui.input_mut(|i| {
        let mut commands = Vec::new();
        // The longer shortcut first: `Ctrl+S` would also match it.
        if i.consume_shortcut(&SAVE_ALL) {
            commands.push(Command::SaveAll);
        } else if i.consume_shortcut(&SAVE) {
            commands.push(Command::Save(tab.path.clone()));
        }
        if sending {
            if i.consume_shortcut(&CANCEL) {
                commands.push(Command::Cancel);
            }
        } else if i.consume_shortcut(&SEND) {
            commands.push(Command::Send);
        }
        commands
    })
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Variable, VariableValue, resolve_variables};

    use super::*;
    use crate::strings::EN;

    #[test]
    fn hints_hide_secrets_and_name_folders() {
        let folder = [Variable::text("resource", "users")];
        let env = [
            Variable::text("host", "http://x"),
            Variable {
                value: Some(VariableValue::text("s3cr3t")),
                ..Variable::secret("token")
            },
        ];
        let variables = TabVariables {
            resolved: resolve_variables(vec![
                (Origin::Environment("local".into()), &env[..]),
                (Origin::Folder(0), &folder[..]),
            ]),
            folders: vec!["Users".into()],
            revision: 1,
        };
        let hints = hints(&variables, &EN);
        assert_eq!(hints["host"].origin, "Environment local");
        assert_eq!(hints["resource"].origin, "Folder Users");
        assert_eq!(hints["token"].value, EN.secret_hidden);
        assert!(hints.values().all(|h| !h.value.contains("s3cr3t")));
    }
}
