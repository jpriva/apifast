//! The main screen: collection tree, environment, tabs, URL bar, request
//! panel and response.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::TreeItem;
use apifast_domain::{HttpRequest, Secret, sync_params_from_url, sync_url_from_params};
use eframe::egui::{self, RichText};

use crate::actions::{ALL, Action, Keymap};

use crate::atoms::{ColorSwatch, HintsCache, VariableHint, VariableHints};
use crate::core::{Command, ResponseState, Snapshot, Tab, TabContext, TabKey, VaultCommand};
use crate::layout;
use crate::molecules::EnvironmentSelector;
use crate::organisms::{
    CollectionDialogs, CollectionRequest, CommandPalette, EnvironmentEditor, EnvironmentEvent,
    HistoryView, PaletteItem, PaletteMode, RequestChange, RequestPanel, ResponseView, RunnerEvent,
    RunnerWindow, SettingsWindow, ShortcutEditor, TabBar, TabEvent, TreeEvent, TreeView, UrlBar,
    UrlBarEvent, VaultWindow, paste_key,
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
    closed: Vec<TabKey>,
    /// Put the cursor in the URL on the next frame.
    focus_url: bool,
    /// The application's settings.
    pub settings: Settings,
    /// The settings window.
    pub settings_window: SettingsWindow,
    /// The theme and scale last applied, to apply a new choice once.
    applied: Option<(ThemeChoice, f32)>,
    /// The collection key window.
    pub vault_window: VaultWindow,
    /// The collection runner.
    pub runner: RunnerWindow,
    /// A collection's settings.
    pub collection_settings: crate::organisms::CollectionSettingsWindow,
    /// The collections and their proxies at the last frame, for the
    /// settings window to start from.
    last_collections: Vec<CollectionProxyEntry>,
    /// The collection key being pasted in the banner.
    key_paste: String,
    /// Creating and opening collections.
    collections: CollectionDialogs,
}

/// A collection's name and proxy, as the tree last showed them.
#[derive(Debug, Clone)]
struct CollectionProxyEntry {
    root: PathBuf,
    name: String,
    settings: crate::organisms::CollectionSettings,
}

/// The settings of each loaded collection, for the settings window.
fn collection_settings_of(snapshot: &Snapshot) -> Vec<CollectionProxyEntry> {
    (snapshot.collections.iter())
        .filter_map(|c| {
            let collection = &c.tree.as_ref()?.collection;
            Some(CollectionProxyEntry {
                root: c.root.clone(),
                name: c.name.clone(),
                settings: crate::organisms::CollectionSettings {
                    proxy: collection.proxy.clone(),
                    client_certificates: collection.client_certificates.clone(),
                },
            })
        })
        .collect()
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
    key: TabKey,
    revision: u64,
    request: HttpRequest,
}

impl Editor {
    fn follows(&self, tab: &Tab) -> bool {
        tab.is(&self.key) && self.revision == tab.revision
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
        let collection = snapshot.collection.as_ref();
        self.last_collections = collection_settings_of(snapshot);
        let active = snapshot.active_tab();
        let hints = self.follow(active, strings);
        let sending = active.is_some_and(|tab| tab.response == ResponseState::Sending);
        let selected = snapshot.active.as_ref();
        self.request.format_shortcuts = Some(self.keymap.shortcuts(Action::FormatBody));
        self.request.token_busy =
            snapshot.getting_token.is_some() && snapshot.getting_token == snapshot.active;
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
        let key_paste = &mut self.key_paste;

        let mut resolve = None;
        let mut pasted_key = None;
        let mut save_captures = None;
        let mut create_here = None;
        let (
            (environment, edit_environments),
            (tree_event, picked, asked),
            (tab_event, (url_event, edited)),
        ) = layout::workbench(
            ui,
            |ui| {
                top_bar(
                    ui,
                    collection.map(|c| c.tree.collection.name.as_str()),
                    snapshot,
                    strings,
                )
            },
            (!self.sidebar_hidden).then_some(|ui: &mut egui::Ui| {
                sidebar_area(ui, (sidebar, tree, history), snapshot, strings)
            }),
            |ui| {
                tabs.names = (snapshot.collections.iter())
                    .map(|c| (c.root.clone(), c.name.clone()))
                    .collect();
                let tab_event = tabs.show(ui, &snapshot.tabs, selected, strings);
                ui.separator();
                let palette = theme::palette(ui);
                create_here = messages(ui, snapshot, notice, strings);
                (pasted_key, save_captures) = banners(ui, snapshot, key_paste, strings);
                let (Some(tab), Some(editor)) = (active, editor.as_mut()) else {
                    let hint = if collection.is_some() {
                        strings.no_request
                    } else {
                        strings.no_collection
                    };
                    ui.label(RichText::new(hint).color(palette.text_muted));
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

        let token = self.request.token_action.take();
        let mut commands = banner_commands(
            (pasted_key, token, save_captures, resolve),
            collection,
            active,
        );
        if edit_environments {
            self.environments.open = !self.environments.open;
        }
        commands.extend(self.collection_commands(ui, (create_here, asked), snapshot, strings));
        commands.extend(self.windows(ui, snapshot, &hints, strings));
        commands.extend(environment);
        if let Some((root, event)) = tree_event {
            commands.extend(self.tree_commands(root, event));
        }
        if let Some(index) = picked {
            commands.push(Command::OpenHistory(index));
        }
        commands.extend(self.editor_commands(edited, url_event));
        if let Some(event) = tab_event {
            commands.extend(self.tab_commands(event));
        }
        commands.extend(self.keys(ui, snapshot, sending, strings));
        commands
    }

    /// The commands of the editor: the edits made, and sending or
    /// cancelling from the URL bar.
    fn editor_commands(&self, edited: bool, url_event: Option<UrlBarEvent>) -> Vec<Command> {
        let mut commands = Vec::new();
        if let (true, Some(editor)) = (edited, &self.editor) {
            commands.push(Command::Edit {
                tab: editor.key.clone(),
                request: Box::new(editor.request.clone()),
            });
        }
        match url_event {
            Some(UrlBarEvent::Send) => commands.push(Command::Send),
            Some(UrlBarEvent::Cancel) => commands.push(Command::Cancel),
            Some(UrlBarEvent::Edited) | None => {}
        }
        commands
    }

    /// The commands for collections: a folder to create one in, an action
    /// from the sidebar, and what the dialogs ask for.
    fn collection_commands(
        &mut self,
        ui: &egui::Ui,
        (create_here, asked): (Option<PathBuf>, Option<SidebarAsk>),
        snapshot: &Snapshot,
        strings: &Strings,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        if let Some(folder) = create_here {
            commands.push(collection_command(CollectionRequest::Create {
                name: folder.file_name().map_or_else(
                    || strings.new_collection_title.to_owned(),
                    |n| n.to_string_lossy().into_owned(),
                ),
                folder,
            }));
        }
        match asked {
            Some(SidebarAsk::Action(action)) => {
                commands.extend(self.run(action, snapshot, strings));
            }
            Some(SidebarAsk::Workspace(folder)) => commands.push(Command::OpenWorkspace(folder)),
            Some(SidebarAsk::NewWorkspace) => {
                let workspace = snapshot.workspace.as_ref();
                let parent = (workspace.and_then(|w| w.folder.parent()))
                    .map_or_else(|| snapshot.default_parent.clone(), Path::to_path_buf);
                self.collections.new_workspace(&parent);
            }
            Some(SidebarAsk::OpenWorkspace) => self.collections.open_workspace(),
            Some(SidebarAsk::RenameWorkspace) => {
                let name = snapshot.workspace.as_ref().map_or("", |w| w.name.as_str());
                self.collections.rename_workspace(name);
            }
            None => {}
        }
        if let Some(request) = self.collections.show(ui.ctx(), strings) {
            commands.push(collection_command(request));
        }
        commands
    }

    /// The commands of what the tree reported about the collection at
    /// `root`; some open a window about it.
    fn tree_commands(&mut self, root: PathBuf, event: TreeEvent) -> Vec<Command> {
        let focus = Command::FocusCollection(root.clone());
        match event {
            TreeEvent::Expand(expanded) => vec![Command::ExpandCollection { root, expanded }],
            TreeEvent::Environments => {
                self.environments.open = true;
                vec![focus]
            }
            TreeEvent::CollectionKey => {
                self.vault_window.open = true;
                vec![focus]
            }
            TreeEvent::Close => vec![Command::CloseCollection(root)],
            TreeEvent::Remove => vec![Command::RemoveCollection(root)],
            TreeEvent::Settings => {
                if let Some(entry) = self.last_collections.iter().find(|c| c.root == root) {
                    self.collection_settings.open_for(
                        root.clone(),
                        entry.name.clone(),
                        entry.settings.clone(),
                    );
                }
                vec![focus]
            }
            TreeEvent::Run { folder, label } => {
                self.runner.open_for(root.clone(), folder, label);
                vec![focus]
            }
            TreeEvent::RequestSelected(path) => vec![Command::OpenRequest(TabKey::new(root, path))],
            TreeEvent::CreateRequest { folder, name } => {
                vec![Command::CreateRequest { root, folder, name }]
            }
            TreeEvent::CreateFolder { parent, name } => {
                vec![Command::CreateFolder { root, parent, name }]
            }
            TreeEvent::Rename { item, name } => vec![Command::Rename { root, item, name }],
            TreeEvent::Delete(item) => vec![Command::Delete { root, item }],
            TreeEvent::Move { item, placement } => vec![Command::Move {
                root,
                item,
                placement,
            }],
        }
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
        let files = (snapshot.environment_files.as_slice(), &snapshot.vault);
        let events = self.environments.show(&ctx, files, context, hints, strings);
        commands.extend(events.into_iter().map(environment_command));
        let vault = self.vault_window.show(&ctx, &snapshot.vault, strings);
        commands.extend(vault.into_iter().map(Command::Vault));
        if let Some((root, settings)) = self.collection_settings.show(&ctx, strings) {
            commands.push(Command::SaveCollectionSettings {
                root,
                proxy: settings.proxy,
                client_certificates: settings.client_certificates,
            });
        }
        commands.extend(
            self.runner
                .show(&ctx, snapshot.run.as_ref(), strings)
                .map(|event| match event {
                    RunnerEvent::Run { root, settings } => Command::Run { root, settings },
                    RunnerEvent::Stop => Command::StopRun,
                    RunnerEvent::Close => Command::CloseRun,
                    RunnerEvent::OpenStep(index) => Command::OpenRunStep(index),
                }),
        );
        if self.shortcuts.show(&ctx, &mut self.keymap, strings) {
            self.unsaved.keymap = true;
        }
        self.settings_window.password_saved = snapshot.proxy_password_saved;
        if self.settings_window.show(&ctx, &mut self.settings, strings) {
            self.unsaved.settings = true;
        }
        if let Some(password) = self.settings_window.password_change.take() {
            commands.push(Command::SetProxyPassword(password));
        }
        if self.palette.open.is_some() {
            let items = palette_items(&ctx, snapshot, &self.keymap, strings);
            match self.palette.show(&ctx, &items, strings) {
                Some(PaletteItem::Action(action, ..)) => {
                    commands.extend(self.run(action, snapshot, strings));
                }
                Some(PaletteItem::Request(key, ..)) => commands.push(Command::OpenRequest(key)),
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
            .and_then(|key| snapshot.tabs.iter().position(|tab| tab.is(key)));
        let tab_at =
            |index: usize| (snapshot.tabs.get(index)).map(|tab| Command::ActivateTab(tab.key()));
        let count = snapshot.tabs.len();
        let one = |command: Option<Command>| command.into_iter().collect();
        match action {
            Action::Send => one((active.is_some() && !sending).then_some(Command::Send)),
            Action::Cancel => one(sending.then_some(Command::Cancel)),
            Action::Save => one(active.map(|tab| Command::Save(tab.key()))),
            Action::SaveAll => vec![Command::SaveAll],
            Action::Palette => {
                self.palette.open(PaletteMode::All);
                Vec::new()
            }
            Action::QuickOpen => {
                self.palette.open(PaletteMode::Requests);
                Vec::new()
            }
            Action::NewRequest => {
                one(snapshot
                    .collection
                    .as_ref()
                    .map(|collection| Command::CreateRequest {
                        root: collection.root.clone(),
                        folder: String::new(),
                        name: strings.new_request_name.to_owned(),
                    }))
            }
            Action::CloseTab => match active.and_then(|tab| self.tabs.request_close(tab)) {
                Some(event) => self.tab_commands(event),
                None => Vec::new(),
            },
            Action::ReopenTab => one(self.reopen(snapshot)),
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
            Action::CollectionKey => {
                self.vault_window.open = true;
                Vec::new()
            }
            Action::Settings => {
                self.settings_window.open = true;
                Vec::new()
            }
            Action::OpenCollection => {
                self.collections.open_collection();
                Vec::new()
            }
            Action::NewCollection => {
                // In the active workspace's folder unless another is chosen.
                self.collections.new_collection(&snapshot.default_parent);
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

    /// The last closed tab that is not open again.
    fn reopen(&mut self, snapshot: &Snapshot) -> Option<Command> {
        while let Some(key) = self.closed.pop() {
            if !snapshot.tabs.iter().any(|t| t.is(&key)) {
                return Some(Command::OpenRequest(key));
            }
        }
        None
    }

    /// The commands of a tab event; a closed tab is remembered to open
    /// again.
    fn tab_commands(&mut self, event: TabEvent) -> Vec<Command> {
        let mut closed = |key: &TabKey| {
            self.closed.retain(|k| k != key);
            self.closed.push(key.clone());
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
                    key: tab.key(),
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
    // Every loaded collection's requests; with several, named after it.
    let loaded: Vec<_> = (snapshot.collections.iter())
        .filter_map(|c| Some((c, c.tree.as_ref()?)))
        .collect();
    for (collection, tree) in &loaded {
        let prefix = (loaded.len() > 1).then_some(collection.name.as_str());
        requests(&tree.items, (&collection.root, prefix), &mut items);
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

/// The requests of `tree`, of the collection at `root`; their names start
/// with `prefix` when given.
fn requests(
    tree: &[TreeItem],
    (root, prefix): (&PathBuf, Option<&str>),
    out: &mut Vec<PaletteItem>,
) {
    for item in tree {
        match item {
            TreeItem::Folder { children, .. } => requests(children, (root, prefix), out),
            TreeItem::Request {
                path, name, method, ..
            } => out.push(PaletteItem::Request(
                TabKey::new(root.clone(), path.clone()),
                prefix.map_or_else(|| name.clone(), |p| format!("{p} / {name}")),
                method.clone(),
            )),
            _ => {}
        }
    }
}

/// Shown while the collection shares secrets this machine cannot open:
/// asks for the collection's key. Returns it once given.
fn key_banner(ui: &mut egui::Ui, text: &mut String, strings: &Strings) -> Option<Secret> {
    let warning = theme::palette(ui).warning;
    let mut key = None;
    egui::Frame::new()
        .fill(warning.gamma_multiply(theme::BADGE_TINT))
        .stroke(egui::Stroke::new(1.0, warning))
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(theme::space::M)
        .show(ui, |ui| {
            ui.label(RichText::new(strings.vault_banner).color(warning));
            key = paste_key(ui, text, strings);
        });
    ui.add_space(theme::space::S);
    key
}

/// What the banners and the token buttons asked for: a pasted key, the
/// token of the active tab, the captures to save, a conflict resolved.
fn banner_commands(
    (pasted_key, token, save_captures, resolve): (
        Option<Secret>,
        Option<crate::organisms::TokenAction>,
        Option<bool>,
        Option<bool>,
    ),
    collection: Option<&crate::core::CollectionView>,
    active: Option<&Tab>,
) -> Vec<Command> {
    let mut commands: Vec<Command> = pasted_key
        .map(|key| Command::Vault(VaultCommand::ImportKey(key)))
        .into_iter()
        .collect();
    if let (Some(action), Some(tab)) = (token, active) {
        commands.push(match action {
            crate::organisms::TokenAction::Get => Command::GetToken(tab.key()),
            crate::organisms::TokenAction::Forget => Command::ForgetToken(tab.key()),
        });
    }
    if let (Some(save), Some(collection)) = (save_captures, collection) {
        commands.push(Command::SaveCaptures {
            root: collection.root.clone(),
            save,
        });
    }
    if let (Some(keep_draft), Some(tab)) = (resolve, active) {
        commands.push(Command::ResolveConflict {
            tab: tab.key(),
            keep_draft,
        });
    }
    commands
}

/// The banners over the editor about the focused collection: its key is
/// missing, values were captured for its environment.
fn banners(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    key_paste: &mut String,
    strings: &Strings,
) -> (Option<Secret>, Option<bool>) {
    let key = (snapshot.vault.shares && !snapshot.vault.has_key)
        .then(|| key_banner(ui, key_paste, strings))
        .flatten();
    let captures = (snapshot.captured.as_ref()).and_then(|c| captures_banner(ui, c, strings));
    (key, captures)
}

/// Offers to write the values captured for the active environment there.
/// Returns whether to, once the user chooses.
fn captures_banner(
    ui: &mut egui::Ui,
    captured: &crate::core::PendingCaptures,
    strings: &Strings,
) -> Option<bool> {
    let accent = theme::palette(ui).accent;
    let mut choice = None;
    egui::Frame::new()
        .fill(accent.gamma_multiply(theme::BADGE_TINT))
        .stroke(egui::Stroke::new(1.0, accent))
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(theme::space::M)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let text = strings
                    .captured_for_environment
                    .replace("{environment}", &captured.environment)
                    .replace("{names}", &captured.names.join(", "));
                ui.label(text);
                if ui.button(strings.save_to_environment).clicked() {
                    choice = Some(true);
                }
                if ui.button(strings.discard_captures).clicked() {
                    choice = Some(false);
                }
            });
        });
    ui.add_space(theme::space::S);
    choice
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
    name: Option<&str>,
    snapshot: &Snapshot,
    strings: &Strings,
) -> (Option<Command>, bool) {
    ui.horizontal(|ui| {
        let Some(name) = name else {
            ui.label(RichText::new("apifast").strong());
            return (None, false);
        };
        ui.label(RichText::new(name).strong());
        if snapshot.vault.master_in_file {
            ui.label(RichText::new(strings.master_in_file).color(theme::palette(ui).warning))
                .on_hover_text(strings.master_in_file_hint);
        }
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
        EnvironmentEvent::Secret {
            environment,
            name,
            value,
            shared,
        } => Command::Vault(VaultCommand::Set {
            environment,
            name,
            value,
            shared,
        }),
        EnvironmentEvent::Reveal { environment, name } => {
            Command::Vault(VaultCommand::Reveal { environment, name })
        }
        EnvironmentEvent::Hide => Command::Vault(VaultCommand::Hide),
        EnvironmentEvent::Forget(orphan) => Command::Vault(VaultCommand::Forget(orphan)),
    }
}

/// What the sidebar asks for besides the tree and the history.
enum SidebarAsk {
    /// An action, such as creating or opening a collection.
    Action(Action),
    /// Switch to this workspace.
    Workspace(PathBuf),
    /// Create a workspace.
    NewWorkspace,
    /// Open a workspace folder.
    OpenWorkspace,
    /// Rename the active workspace.
    RenameWorkspace,
}

/// The sidebar: the workspace, then a switch between its collections and
/// the focused collection's history. Returns what the tree reported, with
/// its collection, the history entry picked and what else was asked for.
fn sidebar_area(
    ui: &mut egui::Ui,
    (sidebar, tree, history): (&mut Sidebar, &mut TreeView, &mut HistoryView),
    snapshot: &Snapshot,
    strings: &Strings,
) -> (
    Option<(PathBuf, TreeEvent)>,
    Option<usize>,
    Option<SidebarAsk>,
) {
    let ask = workspace_header(ui, snapshot, strings);
    ui.horizontal(|ui| {
        ui.selectable_value(sidebar, Sidebar::Collection, strings.sidebar_collection);
        ui.selectable_value(sidebar, Sidebar::History, strings.sidebar_history);
    });
    ui.separator();
    match *sidebar {
        Sidebar::History => {
            if let Some(collection) = &snapshot.collection {
                let name = &collection.tree.collection.name;
                let muted = theme::palette(ui).text_muted;
                ui.label(RichText::new(format!("{} {name}", strings.history_of)).color(muted));
            }
            (
                None,
                history.show(ui, &snapshot.history, now_ms(), strings),
                ask,
            )
        }
        Sidebar::Collection if snapshot.collections.is_empty() => {
            let muted = theme::palette(ui).text_muted;
            ui.label(RichText::new(strings.no_collection).color(muted));
            (None, None, ask)
        }
        Sidebar::Collection => {
            let focused = snapshot.collection.as_ref().map(|c| c.root.as_path());
            let selected = snapshot.active.as_ref();
            let event = tree.show(
                ui,
                &snapshot.collections,
                snapshot.version,
                (selected, focused),
                strings,
            );
            (event, None, ask)
        }
    }
}

/// The workspace selector and the buttons that create or open a
/// collection. Returns what was asked for.
fn workspace_header(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    strings: &Strings,
) -> Option<SidebarAsk> {
    let mut ask = None;
    let active = snapshot.workspace.as_ref();
    let current = active.map_or("", |w| w.name.as_str());
    let response = egui::ComboBox::from_id_salt("workspace")
        .selected_text(RichText::new(current).strong())
        .width(ui.available_width() - theme::space::S)
        .show_ui(ui, |ui| {
            for workspace in &snapshot.workspaces {
                let is_active = active.is_some_and(|a| a.folder == workspace.folder);
                let item = ui
                    .selectable_label(is_active, workspace.name.as_str())
                    .on_hover_text(workspace.folder.display().to_string());
                if item.clicked() && !is_active {
                    ask = Some(SidebarAsk::Workspace(workspace.folder.clone()));
                }
            }
            ui.separator();
            if ui.button(strings.new_workspace).clicked() {
                ask = Some(SidebarAsk::NewWorkspace);
            }
            if ui.button(strings.open_workspace).clicked() {
                ask = Some(SidebarAsk::OpenWorkspace);
            }
            if ui.button(strings.rename_workspace).clicked() {
                ask = Some(SidebarAsk::RenameWorkspace);
            }
        });
    response.response.accessible_name(strings.workspace);
    ui.horizontal(|ui| {
        if ui.small_button(strings.new_collection).clicked() {
            ask = Some(SidebarAsk::Action(Action::NewCollection));
        }
        if ui.small_button(strings.open_collection).clicked() {
            ask = Some(SidebarAsk::Action(Action::OpenCollection));
        }
    });
    ask
}

/// The error and the notice, if any. A folder without a collection gets a
/// button to create one there: returns that folder when pressed.
fn messages(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    notice: Option<&str>,
    strings: &Strings,
) -> Option<PathBuf> {
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
    let folder = snapshot.missing_collection.as_ref()?;
    (snapshot.error.is_some() && ui.button(strings.create_here).clicked()).then(|| folder.clone())
}

/// The command of what the collection dialogs asked for.
fn collection_command(request: CollectionRequest) -> Command {
    match request {
        CollectionRequest::Open(folder) => Command::OpenCollection(folder),
        CollectionRequest::Create { folder, name } => Command::CreateCollection { folder, name },
        CollectionRequest::OpenWorkspace(folder) => Command::OpenWorkspace(folder),
        CollectionRequest::CreateWorkspace { folder, name } => {
            Command::CreateWorkspace { folder, name }
        }
        CollectionRequest::RenameWorkspace(name) => Command::RenameWorkspace(name),
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
