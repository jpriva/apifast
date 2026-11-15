//! The main screen: collection tree, environment, tabs, URL bar, request
//! panel and response.

use std::sync::Arc;

use apifast_app::RequestPath;
use apifast_domain::{HttpRequest, sync_params_from_url, sync_url_from_params};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use crate::atoms::{ColorSwatch, HintsCache, VariableHint, VariableHints};
use crate::core::{Command, ResponseState, Snapshot, Tab, TabContext};
use crate::layout;
use crate::molecules::EnvironmentSelector;
use crate::organisms::{
    EnvironmentEditor, EnvironmentEvent, HistoryView, RequestChange, RequestPanel, ResponseView,
    TabBar, TabEvent, TreeEvent, TreeView, UrlBar, UrlBarEvent,
};
use crate::strings::Strings;
use crate::theme;

const SEND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const CANCEL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_ALL: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const ENVIRONMENTS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);

/// Default height of the request panel, above the response, in points.
const REQUEST_PANEL_HEIGHT: f32 = 280.0;
/// Lowest the request panel can be resized to, in points.
const REQUEST_PANEL_MIN_HEIGHT: f32 = 120.0;

/// What the sidebar shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sidebar {
    /// The collection's tree.
    #[default]
    Collection,
    /// The history of sends.
    History,
}

/// The state the main screen keeps between frames.
#[derive(Debug, Default)]
pub struct MainScreen {
    /// What the sidebar shows.
    pub sidebar: Sidebar,
    tree: TreeView,
    history: HistoryView,
    /// The environment editor window.
    pub environments: EnvironmentEditor,
    tabs: TabBar,
    request: RequestPanel,
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
        let (sidebar, history) = (&mut self.sidebar, &mut self.history);
        let (tree, tabs, panels, editor) = (
            &mut self.tree,
            &mut self.tabs,
            (&mut self.request, &mut self.response),
            &mut self.editor,
        );

        let (
            (environment, edit_environments),
            (tree_event, picked),
            (tab_event, (url_event, edited)),
        ) = layout::workbench(
            ui,
            |ui| top_bar(ui, &collection.tree.collection.name, snapshot, strings),
            |ui| sidebar_area(ui, (sidebar, tree, history), snapshot, selected, strings),
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
                    return (tab_event, (None, false));
                };
                let edits = editor_area(
                    ui,
                    tab,
                    &mut editor.request,
                    panels,
                    &hints,
                    sending,
                    strings,
                );
                (tab_event, edits)
            },
        );

        let mut commands = Vec::new();
        if edit_environments || ui.input_mut(|i| i.consume_shortcut(&ENVIRONMENTS)) {
            self.environments.open = !self.environments.open;
        }
        let context = active.map(|tab| tab.context.as_ref());
        let files = &snapshot.environment_files;
        if let Some(event) = self
            .environments
            .show(ui.ctx(), files, context, &hints, strings)
        {
            commands.push(environment_command(event));
        }
        commands.extend(environment);
        if let Some(event) = tree_event {
            commands.push(tree_command(event));
        }
        if let Some(index) = picked {
            commands.push(Command::OpenHistory(index));
        }
        if let (true, Some(editor)) = (edited, &self.editor) {
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
                let context = &tab.context;
                self.hints.get(context.revision, || hints(context, strings))
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

/// The URL bar, the request panel and the response, one under the other.
/// Returns the URL bar's event and whether the request was edited, with the
/// URL and the parameter list kept in step.
fn editor_area(
    ui: &mut egui::Ui,
    tab: &Tab,
    request: &mut HttpRequest,
    (panel, response): (&mut RequestPanel, &mut ResponseView),
    hints: &VariableHints,
    sending: bool,
    strings: &Strings,
) -> (Option<UrlBarEvent>, bool) {
    let event = UrlBar::new(
        &mut request.method,
        &mut request.url,
        hints,
        sending,
        strings,
    )
    .show(ui);
    let mut edited = event == Some(UrlBarEvent::Edited);
    if edited {
        sync_params_from_url(request);
    }
    ui.separator();
    let change = egui::Panel::top("request-panel")
        .resizable(true)
        .default_size(REQUEST_PANEL_HEIGHT)
        .min_size(REQUEST_PANEL_MIN_HEIGHT)
        .show(ui, |ui| {
            panel.show(ui, request, &tab.context, hints, strings)
        })
        .inner;
    match change {
        Some(RequestChange::Params) => {
            sync_url_from_params(request);
            edited = true;
        }
        Some(RequestChange::Other) => edited = true,
        None => {}
    }
    response.show(ui, &tab.response, strings);
    (event, edited)
}

/// What the fields show about each variable: its value (hidden if secret)
/// and its origin, in the interface's language.
fn hints(context: &TabContext, strings: &Strings) -> VariableHints {
    context
        .variables
        .iter()
        .map(|(name, variable)| {
            let value = match (&variable.value, variable.secret) {
                (Some(value), false) => value.data.clone(),
                _ => strings.secret_hidden.to_owned(),
            };
            let origin = strings.origin(&variable.origin, &context.folders);
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

/// The top bar: the collection's name, then the environment, its color and
/// the button of the editor. Returns the environment chosen and whether the
/// editor was asked for.
fn top_bar(
    ui: &mut egui::Ui,
    name: &str,
    snapshot: &Snapshot,
    strings: &Strings,
) -> (Option<Command>, bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let edit = ui
                .button(strings.environments)
                .on_hover_text(strings.edit_environments)
                .clicked();
            let color = (snapshot.environment_files.iter())
                .find(|e| Some(e.name.as_str()) == snapshot.environment.as_deref())
                .and_then(|e| e.color.as_deref());
            let choice = EnvironmentSelector::new(
                &snapshot.environments,
                snapshot.environment.as_deref(),
                strings.no_environment,
                strings.environment,
            )
            .show(ui);
            ui.add(ColorSwatch::new(color));
            (choice.map(Command::SelectEnvironment), edit)
        })
        .inner
    })
    .inner
}

fn environment_command(event: EnvironmentEvent) -> Command {
    match event {
        EnvironmentEvent::Create(name) => Command::CreateEnvironment(name),
        EnvironmentEvent::Rename { from, to } => Command::RenameEnvironment { from, to },
        EnvironmentEvent::Save(environment) => Command::SaveEnvironment(Box::new(environment)),
        EnvironmentEvent::Delete(name) => Command::DeleteEnvironment(name),
    }
}

/// The sidebar: a switch between the collection's tree and the history.
/// Returns what the tree reported and the history entry picked.
fn sidebar_area(
    ui: &mut egui::Ui,
    (sidebar, tree, history): (&mut Sidebar, &mut TreeView, &mut HistoryView),
    snapshot: &Snapshot,
    selected: Option<&RequestPath>,
    strings: &Strings,
) -> (Option<TreeEvent>, Option<usize>) {
    ui.horizontal(|ui| {
        ui.selectable_value(sidebar, Sidebar::Collection, strings.sidebar_collection);
        ui.selectable_value(sidebar, Sidebar::History, strings.sidebar_history);
    });
    ui.separator();
    match (*sidebar, &snapshot.collection) {
        (Sidebar::Collection, Some(collection)) => (
            tree.show(ui, &collection.tree, snapshot.version, selected, strings),
            None,
        ),
        (Sidebar::History, _) => (None, history.show(ui, &snapshot.history, now_ms(), strings)),
        (Sidebar::Collection, None) => (None, None),
    }
}

/// The current time, in milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Origin, Variable, VariableValue, resolve_variables};

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
        let context = TabContext {
            variables: resolve_variables(vec![
                (Origin::Environment("local".into()), &env[..]),
                (Origin::Folder(0), &folder[..]),
            ]),
            folders: vec!["Users".into()],
            revision: 1,
            ..TabContext::default()
        };
        let hints = hints(&context, &EN);
        assert_eq!(hints["host"].origin, "Environment local");
        assert_eq!(hints["resource"].origin, "Folder Users");
        assert_eq!(hints["token"].value, EN.secret_hidden);
        assert!(hints.values().all(|h| !h.value.contains("s3cr3t")));
    }
}
