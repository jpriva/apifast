//! The main screen: collection tree, environment, tabs, URL bar, request
//! panel and response.

use std::sync::Arc;

use apifast_app::{RequestPath, TreeItem};
use apifast_domain::{HttpRequest, sync_params_from_url, sync_url_from_params};
use eframe::egui::{self, RichText};

use crate::actions::{ALL, Action, Keymap};

use crate::atoms::{ColorSwatch, HintsCache, VariableHint, VariableHints};
use crate::core::{Command, ResponseState, Snapshot, Tab, TabContext};
use crate::layout;
use crate::molecules::EnvironmentSelector;
use crate::organisms::{
    CommandPalette, EnvironmentEditor, EnvironmentEvent, HistoryView, PaletteItem, PaletteMode,
    RequestChange, RequestPanel, ResponseView, SettingsWindow, ShortcutEditor, TabBar, TabEvent,
    TreeEvent, TreeView, UrlBar, UrlBarEvent,
};
use crate::settings::Settings;
use crate::strings::Strings;
use crate::theme::{self, ThemeChoice};

/// Closed tabs remembered, to open again.
const CLOSED_TABS: usize = 20;

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
    /// The shortcuts of every action.
    pub keymap: Keymap,
    /// Whether the keymap changed since [`Self::take_keymap_change`].
    unsaved: Unsaved,
    /// Something to tell about the keymap, such as entries of
    /// `keymap.yml` that were ignored.
    pub notice: Option<String>,
    /// The command palette.
    pub palette: CommandPalette,
    /// The editor of shortcuts.
    pub shortcuts: ShortcutEditor,
    /// Whether the sidebar is hidden.
    pub sidebar_hidden: bool,
    /// Tabs closed, most recent last, to open again.
    closed: Vec<RequestPath>,
    /// Put the cursor in the URL on the next frame.
    focus_url: bool,
    /// The application's settings.
    pub settings: Settings,
    /// The settings window.
    pub settings_window: SettingsWindow,
    /// The theme and scale last applied, to apply a new choice once.
    applied: Option<(ThemeChoice, f32)>,
}

/// What changed and is not saved yet.
#[derive(Debug, Default)]
struct Unsaved {
    keymap: bool,
    settings: bool,
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
        self.request.format_shortcuts = Some(self.keymap.shortcuts(Action::FormatBody));
        self.response.pretty_limit = Some(self.settings.pretty_limit());
        let focus_url = std::mem::take(&mut self.focus_url);
        let notice = self.notice.as_deref();
        let (sidebar, history) = (&mut self.sidebar, &mut self.history);
        let (tree, tabs, panels, editor) = (
            &mut self.tree,
            &mut self.tabs,
            (&mut self.request, &mut self.response),
            &mut self.editor,
        );

        let mut resolve = None;
        let (
            (environment, edit_environments),
            (tree_event, picked),
            (tab_event, (url_event, edited)),
        ) = layout::workbench(
            ui,
            |ui| top_bar(ui, &collection.tree.collection.name, snapshot, strings),
            (!self.sidebar_hidden).then_some(|ui: &mut egui::Ui| {
                sidebar_area(ui, (sidebar, tree, history), snapshot, selected, strings)
            }),
            |ui| {
                let tab_event = tabs.show(ui, &snapshot.tabs, selected, strings);
                ui.separator();
                let palette = theme::palette(ui);
                for (message, color) in [
                    (snapshot.error.as_deref(), palette.danger),
                    (notice, palette.warning),
                ] {
                    if let Some(message) = message {
                        ui.label(RichText::new(message).color(color));
                        ui.add_space(theme::space::M);
                    }
                }
                let (Some(tab), Some(editor)) = (active, editor.as_mut()) else {
                    ui.label(RichText::new(strings.no_request).color(palette.text_muted));
                    return (tab_event, (None, false));
                };
                if tab.conflict {
                    resolve = conflict_banner(ui, strings);
                }
                let area = (panels, &*hints, sending, focus_url);
                let edits = editor_area(ui, tab, &mut editor.request, area, strings);
                (tab_event, edits)
            },
        );

        let mut commands = Vec::new();
        if let (Some(keep_draft), Some(tab)) = (resolve, active) {
            commands.push(Command::ResolveConflict {
                path: tab.path.clone(),
                keep_draft,
            });
        }
        if edit_environments {
            self.environments.open = !self.environments.open;
        }
        commands.extend(self.windows(ui, snapshot, &hints, strings));
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
        if let Some(event) = tab_event {
            commands.extend(self.tab_commands(event));
        }
        match url_event {
            Some(UrlBarEvent::Send) => commands.push(Command::Send),
            Some(UrlBarEvent::Cancel) => commands.push(Command::Cancel),
            Some(UrlBarEvent::Edited) | None => {}
        }
        commands.extend(self.keys(ui, snapshot, sending, strings));
        commands
    }

    /// The commands of the shortcuts pressed this frame. Keys go to the
    /// palette or the shortcut editor while they are up.
    fn keys(
        &mut self,
        ui: &egui::Ui,
        snapshot: &Snapshot,
        sending: bool,
        strings: &Strings,
    ) -> Vec<Command> {
        if self.palette.open.is_some() || self.shortcuts.capturing() {
            return Vec::new();
        }
        let available: Vec<Action> = (ALL.iter().copied())
            .filter(|action| match action {
                // Escape is free for everything else when not sending.
                Action::Cancel => sending,
                Action::Send => !sending,
                // The body editor takes its own shortcut.
                Action::FormatBody => false,
                _ => true,
            })
            .collect();
        let pressed = ui.input_mut(|i| self.keymap.pressed(i, &available));
        pressed
            .into_iter()
            .flat_map(|action| self.run(action, snapshot, strings))
            .collect()
    }

    /// Whether the settings changed since the last call, so they can be
    /// saved and the core told.
    pub fn take_settings_change(&mut self) -> bool {
        std::mem::take(&mut self.unsaved.settings)
    }

    /// Applies the theme and scale of the settings, on every screen. The
    /// scale waits until the mouse is released, so a slider being dragged
    /// does not move under the pointer; a scale changed with `Ctrl+=` or
    /// `Ctrl+-` goes back into the settings.
    pub fn apply_appearance(&mut self, ctx: &egui::Context) {
        let settings = &mut self.settings;
        if let Some((_, scale)) = self.applied
            && (ctx.zoom_factor() - scale).abs() > f32::EPSILON
        {
            settings.ui_scale = ctx.zoom_factor();
            self.applied = Some((settings.theme, settings.ui_scale));
            self.unsaved.settings = true;
        }
        if self.applied == Some((settings.theme, settings.ui_scale)) {
            return;
        }
        theme::apply(ctx, settings.theme);
        if !ctx.input(|i| i.pointer.any_down()) {
            ctx.set_zoom_factor(settings.ui_scale);
            self.applied = Some((settings.theme, settings.ui_scale));
        }
    }

    /// Whether the keymap changed since the last call, so it can be saved.
    pub fn take_keymap_change(&mut self) -> bool {
        std::mem::take(&mut self.unsaved.keymap)
    }

    /// The windows over the screen: environments, shortcuts and the
    /// palette. Returns the commands they ask for.
    fn windows(
        &mut self,
        ui: &egui::Ui,
        snapshot: &Snapshot,
        hints: &VariableHints,
        strings: &Strings,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let ctx = ui.ctx().clone();
        let context = snapshot.active_tab().map(|tab| tab.context.as_ref());
        let files = &snapshot.environment_files;
        if let Some(event) = self.environments.show(&ctx, files, context, hints, strings) {
            commands.push(environment_command(event));
        }
        if self.shortcuts.show(&ctx, &mut self.keymap, strings) {
            self.unsaved.keymap = true;
        }
        if self.settings_window.show(&ctx, &mut self.settings, strings) {
            self.unsaved.settings = true;
        }
        if self.palette.open.is_some() {
            let items = palette_items(&ctx, snapshot, &self.keymap, strings);
            match self.palette.show(&ctx, &items, strings) {
                Some(PaletteItem::Action(action, ..)) => {
                    commands.extend(self.run(action, snapshot, strings));
                }
                Some(PaletteItem::Request(path, ..)) => commands.push(Command::OpenRequest(path)),
                Some(PaletteItem::Environment(name, _)) => {
                    commands.push(Command::SelectEnvironment(name));
                }
                None => {}
            }
        }
        commands
    }

    /// What `action` does now.
    fn run(&mut self, action: Action, snapshot: &Snapshot, strings: &Strings) -> Vec<Command> {
        let active = snapshot.active_tab();
        let sending = active.is_some_and(|tab| tab.response == ResponseState::Sending);
        let position = (snapshot.active.as_ref())
            .and_then(|path| snapshot.tabs.iter().position(|tab| tab.path == *path));
        let tab_at = |index: usize| {
            (snapshot.tabs.get(index)).map(|tab| Command::ActivateTab(tab.path.clone()))
        };
        let count = snapshot.tabs.len();
        let one = |command: Option<Command>| command.into_iter().collect();
        match action {
            Action::Send => one((active.is_some() && !sending).then_some(Command::Send)),
            Action::Cancel => one(sending.then_some(Command::Cancel)),
            Action::Save => one(active.map(|tab| Command::Save(tab.path.clone()))),
            Action::SaveAll => vec![Command::SaveAll],
            Action::Palette => {
                self.palette.open(PaletteMode::All);
                Vec::new()
            }
            Action::QuickOpen => {
                self.palette.open(PaletteMode::Requests);
                Vec::new()
            }
            Action::NewRequest => vec![Command::CreateRequest {
                folder: String::new(),
                name: strings.new_request_name.to_owned(),
            }],
            Action::CloseTab => match active.and_then(|tab| self.tabs.request_close(tab)) {
                Some(event) => self.tab_commands(event),
                None => Vec::new(),
            },
            Action::ReopenTab => {
                let open = |path: &RequestPath| snapshot.tabs.iter().any(|t| t.path == *path);
                while let Some(path) = self.closed.pop() {
                    if !open(&path) {
                        return vec![Command::OpenRequest(path)];
                    }
                }
                Vec::new()
            }
            Action::NextTab => one(position.and_then(|i| tab_at((i + 1) % count))),
            Action::PreviousTab => one(position.and_then(|i| tab_at((i + count - 1) % count))),
            Action::GoToTab(n) => one(tab_at(usize::from(n).saturating_sub(1))),
            Action::FocusUrl => {
                self.focus_url = true;
                Vec::new()
            }
            Action::Environments => {
                self.environments.open = !self.environments.open;
                Vec::new()
            }
            Action::ToggleSidebar => {
                self.sidebar_hidden = !self.sidebar_hidden;
                Vec::new()
            }
            Action::ShowHistory => {
                self.sidebar_hidden = false;
                self.sidebar = Sidebar::History;
                Vec::new()
            }
            Action::FormatBody => {
                self.request.tab = crate::organisms::RequestTab::Body;
                self.request.format_now = true;
                Vec::new()
            }
            Action::Shortcuts => {
                self.shortcuts.open = true;
                Vec::new()
            }
            Action::Settings => {
                self.settings_window.open = true;
                Vec::new()
            }
            Action::ThemeSystem | Action::ThemeLight | Action::ThemeDark => {
                self.settings.theme = match action {
                    Action::ThemeLight => ThemeChoice::Light,
                    Action::ThemeDark => ThemeChoice::Dark,
                    _ => ThemeChoice::System,
                };
                self.unsaved.settings = true;
                Vec::new()
            }
        }
    }

    /// The commands of a tab event; a closed tab is remembered to open
    /// again.
    fn tab_commands(&mut self, event: TabEvent) -> Vec<Command> {
        let mut closed = |path: &RequestPath| {
            self.closed.retain(|p| p != path);
            self.closed.push(path.clone());
            if self.closed.len() > CLOSED_TABS {
                self.closed.remove(0);
            }
        };
        match event {
            TabEvent::Activate(path) => vec![Command::ActivateTab(path)],
            TabEvent::Close(path) => {
                closed(&path);
                vec![Command::CloseTab(path)]
            }
            TabEvent::SaveAndClose(path) => {
                closed(&path);
                vec![Command::Save(path.clone()), Command::CloseTab(path)]
            }
        }
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
}

/// The URL bar, the request panel and the response, one under the other.
/// Returns the URL bar's event and whether the request was edited, with the
/// URL and the parameter list kept in step.
fn editor_area(
    ui: &mut egui::Ui,
    tab: &Tab,
    request: &mut HttpRequest,
    ((panel, response), hints, sending, focus_url): (
        (&mut RequestPanel, &mut ResponseView),
        &VariableHints,
        bool,
        bool,
    ),
    strings: &Strings,
) -> (Option<UrlBarEvent>, bool) {
    let event = UrlBar::new(
        &mut request.method,
        &mut request.url,
        hints,
        sending,
        strings,
    )
    .focus(focus_url)
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

/// What the palette offers: every action but the tab numbers, then every
/// request, then every environment.
fn palette_items(
    ctx: &egui::Context,
    snapshot: &Snapshot,
    keymap: &Keymap,
    strings: &Strings,
) -> Vec<PaletteItem> {
    let mut items: Vec<PaletteItem> = (ALL.iter().copied())
        .filter(|action| !matches!(action, Action::GoToTab(_) | Action::Palette))
        .map(|action| {
            let shortcut = keymap.first(action).map(|s| ctx.format_shortcut(&s));
            PaletteItem::Action(action, action.label(strings), shortcut)
        })
        .collect();
    if let Some(collection) = &snapshot.collection {
        requests(&collection.tree.items, &mut items);
    }
    items.push(PaletteItem::Environment(
        None,
        strings.no_environment.to_owned(),
    ));
    for name in &snapshot.environments {
        items.push(PaletteItem::Environment(Some(name.clone()), name.clone()));
    }
    items
}

fn requests(tree: &[TreeItem], out: &mut Vec<PaletteItem>) {
    for item in tree {
        match item {
            TreeItem::Folder { children, .. } => requests(children, out),
            TreeItem::Request {
                path, name, method, ..
            } => out.push(PaletteItem::Request(
                path.clone(),
                name.clone(),
                method.clone(),
            )),
            _ => {}
        }
    }
}

/// Shown over a request whose file changed outside apifast while it had
/// changes not saved. Returns whether to keep them, once the user chooses.
fn conflict_banner(ui: &mut egui::Ui, strings: &Strings) -> Option<bool> {
    let warning = theme::palette(ui).warning;
    let mut choice = None;
    egui::Frame::new()
        .fill(warning.gamma_multiply(theme::BADGE_TINT))
        .stroke(egui::Stroke::new(1.0, warning))
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(theme::space::M)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(strings.changed_on_disk).color(warning));
                if ui.button(strings.keep_mine).clicked() {
                    choice = Some(true);
                }
                if ui.button(strings.load_from_disk).clicked() {
                    choice = Some(false);
                }
            });
        });
    ui.add_space(theme::space::S);
    choice
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
