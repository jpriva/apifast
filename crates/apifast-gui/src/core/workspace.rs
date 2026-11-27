//! The active workspace (ADR 0009): restoring it, switching between
//! workspaces, its list of collections in `workspace.yml`, and what this
//! machine keeps of it (expanded collections, open tabs).

use crate::messages::Say as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::{EnvironmentRepository as _, Workspace, WorkspaceRepository};
use apifast_domain::{Environment, Variable};
use apifast_storage_fs::{
    FsKnownWorkspaces, FsWorkspace, FsWorkspaceState, KnownWorkspaces, WORKSPACE_FILE,
    WorkspaceStateData,
};
use apifast_watch::FsWatcher;

use super::collections::{Listed, canonical};
use super::{Command, Message, State, TabKey, WorkspaceView};

/// The name of the workspace made on the first start.
const DEFAULT_NAME: &str = "My workspace";

/// The active workspace.
pub(super) struct ActiveWorkspace {
    repo: FsWorkspace,
    state: FsWorkspaceState,
    /// Numbers this opening, so a late change of another is ignored.
    opened: u64,
    /// Watches its folder, not its subfolders, for `workspace.yml`.
    _watcher: Option<FsWatcher>,
    /// Watches its `environments/`, once it exists.
    globals_watcher: Option<FsWatcher>,
    /// Its global environments, as in their files.
    pub(super) global_files: Arc<Vec<Environment>>,
    /// The active global environment.
    pub(super) global_environment: Option<String>,
}

impl State {
    fn known(&self) -> FsKnownWorkspaces {
        FsKnownWorkspaces::new(apifast_history::dirs::known_workspaces_file_in(
            &self.data_dir,
        ))
    }

    /// Opens the workspace last used (unless the settings say not to), or
    /// the default one (made the first time), with its collections and
    /// tabs as they were left.
    pub(super) fn restore(&mut self) {
        let known = self.known().load().unwrap_or_default();
        let last = known
            .active
            .filter(|_| self.reopen_last)
            .filter(|folder| folder.join(WORKSPACE_FILE).is_file());
        let folder = match last {
            Some(folder) => folder,
            None => match self.default_workspace() {
                Ok(folder) => folder,
                Err(message) => {
                    self.snapshot.error = Some(message);
                    return;
                }
            },
        };
        if let Err(message) = self.switch_to(&folder) {
            self.snapshot.error = Some(message);
            // Never without a workspace: fall back to the default one.
            if let Ok(default) = self.default_workspace()
                && default != folder
            {
                let _ = self.switch_to(&default);
            }
        }
    }

    /// The default workspace's folder, made if it does not exist.
    fn default_workspace(&self) -> Result<PathBuf, String> {
        let folder = apifast_history::dirs::default_workspace_in(&self.data_dir);
        if !folder.join(WORKSPACE_FILE).is_file() {
            FsWorkspace::create(&folder, DEFAULT_NAME).map_err(|e| e.say(self.strings))?;
        }
        Ok(canonical(&folder))
    }

    pub(super) fn handle_workspace(&mut self, command: Command) {
        let result = match command {
            Command::CreateWorkspace { folder, name } => FsWorkspace::create(&folder, &name)
                .map_err(|e| e.say(self.strings))
                .and_then(|_| self.switch_to(&canonical(&folder))),
            Command::OpenWorkspace(folder) => self.switch_to(&canonical(&folder)),
            Command::RenameWorkspace(name) => self.rename_workspace(&name),
            Command::ForgetWorkspace(folder) => self.forget_workspace(&canonical(&folder)),
            _ => Ok(()),
        };
        match result {
            Ok(()) => self.snapshot.error = None,
            Err(message) => self.snapshot.error = Some(message),
        }
    }

    /// Leaves the active workspace as it was and opens the one in `folder`.
    fn switch_to(&mut self, folder: &Path) -> Result<(), String> {
        let repo = FsWorkspace::open(folder).map_err(|e| e.say(self.strings))?;
        let workspace = repo.workspace().map_err(|e| e.say(self.strings))?;
        // Leave the current one as it was.
        if self.workspace.is_some() {
            self.write_session();
            self.session_pending = false;
            let roots: Vec<PathBuf> = self.open.keys().cloned().collect();
            for root in roots {
                self.unload(&root);
            }
            self.focus = None;
            self.snapshot.active = None;
        }
        let state = FsWorkspaceState::new(apifast_history::dirs::workspace_state_file_in(
            &self.data_dir,
            folder,
        ));
        let saved = state.load().unwrap_or_default();
        self.listed = listed(&repo, &workspace, &saved);

        let opened = self.next_revision();
        let messages = self.messages.clone();
        let watcher = FsWatcher::start_shallow(folder, move |changes| {
            let Some(messages) = messages.upgrade() else {
                return;
            };
            if changes.rescan || changes.paths.contains(WORKSPACE_FILE) {
                let _ = messages.send(Message::WorkspaceChanged(opened));
            }
            // Its environments/ made or removed.
            if changes.rescan || changes.paths.contains(GLOBALS_DIR) {
                let _ = messages.send(Message::GlobalEnvironmentsChanged(opened));
            }
        })
        .ok();
        let globals_watcher = watch_globals(folder, opened, self.messages.clone());
        let global_files = repo.environments().unwrap_or_else(|e| {
            self.snapshot.error = Some(e.say(self.strings));
            Vec::new()
        });
        // One the files no longer have is not active.
        let global_environment = (saved.global_environment.clone())
            .filter(|name| global_files.iter().any(|e| &e.name == name));
        self.snapshot.global_environment_files = Arc::new(global_files.clone());
        self.snapshot
            .global_environment
            .clone_from(&global_environment);
        self.workspace = Some(ActiveWorkspace {
            repo,
            state,
            opened,
            _watcher: watcher,
            globals_watcher,
            global_files: Arc::new(global_files),
            global_environment,
        });
        self.snapshot.workspace = Some(WorkspaceView {
            folder: folder.to_path_buf(),
            name: workspace.name,
        });
        self.snapshot.default_parent = folder.to_path_buf();
        self.remember_workspace(folder);

        // Expanded collections load; tabs come back in their order.
        let expanded: Vec<PathBuf> = (self.listed.iter())
            .filter(|l| l.expanded)
            .map(|l| l.root.clone())
            .collect();
        for root in expanded {
            let _ = self.load(&root);
        }
        for (root, path) in saved.tabs {
            self.restore_tab(&TabKey::new(root, path));
        }
        let active = (saved.active)
            .map(|(root, path)| TabKey::new(root, path))
            .filter(|key| self.tab(key).is_some())
            .or_else(|| self.snapshot.tabs.first().map(super::Tab::key));
        self.focus = (active.as_ref().map(|key| key.root.clone()))
            .or_else(|| self.open.keys().next().cloned());
        self.snapshot.active = active;
        Ok(())
    }

    /// Adds the workspace in `folder` to the known ones and makes it the
    /// last used.
    fn remember_workspace(&mut self, folder: &Path) {
        let store = self.known();
        let mut known = store.load().unwrap_or_default();
        if !known.known.iter().any(|k| k == folder) {
            known.known.push(folder.to_path_buf());
        }
        known.active = Some(folder.to_path_buf());
        if let Err(e) = store.save(&known) {
            self.snapshot.error = Some(e.say(self.strings));
        }
        self.refresh_known(&known);
    }

    /// The known workspaces as the snapshot shows them, with their names.
    fn refresh_known(&mut self, known: &KnownWorkspaces) {
        self.snapshot.workspaces = (known.known.iter())
            .map(|folder| {
                let name = (FsWorkspace::open(folder).ok())
                    .and_then(|repo| repo.workspace().ok())
                    .map_or_else(|| folder.display().to_string(), |workspace| workspace.name);
                WorkspaceView {
                    folder: folder.clone(),
                    name,
                }
            })
            .collect();
    }

    fn rename_workspace(&mut self, name: &str) -> Result<(), String> {
        let active = self.workspace.as_ref().ok_or("no workspace is open")?;
        let mut workspace = active.repo.workspace().map_err(|e| e.say(self.strings))?;
        name.trim().clone_into(&mut workspace.name);
        active
            .repo
            .save_workspace(&workspace)
            .map_err(|e| e.say(self.strings))?;
        if let Some(view) = &mut self.snapshot.workspace {
            view.name.clone_from(&workspace.name);
        }
        let known = self.known().load().unwrap_or_default();
        self.refresh_known(&known);
        Ok(())
    }

    fn forget_workspace(&mut self, folder: &Path) -> Result<(), String> {
        if self
            .snapshot
            .workspace
            .as_ref()
            .is_some_and(|w| w.folder == folder)
        {
            return Err(self.strings.errors.workspace_active.to_owned());
        }
        let store = self.known();
        let mut known = store.load().unwrap_or_default();
        known.known.retain(|k| k != folder);
        store.save(&known).map_err(|e| e.say(self.strings))?;
        self.refresh_known(&known);
        Ok(())
    }

    /// Adds the collection at `root` to the active workspace's file, if it
    /// is not there.
    pub(super) fn add_to_workspace(&mut self, root: &Path) {
        if self.listed.iter().any(|l| l.root == root) {
            return;
        }
        let name = (self.open.get(root))
            .map(|open| open.tree.collection.name.clone())
            .unwrap_or_default();
        self.listed.push(Listed {
            root: root.to_path_buf(),
            name: name.clone(),
            expanded: true,
            error: None,
        });
        let Some(active) = &self.workspace else {
            return;
        };
        let result = active.repo.workspace().and_then(|mut workspace| {
            workspace
                .collections
                .push(active.repo.entry_for(root, &name));
            active.repo.save_workspace(&workspace)
        });
        if let Err(e) = result {
            self.snapshot.error = Some(e.say(self.strings));
        }
    }

    /// Removes the collection at `root` from the active workspace's file.
    pub(super) fn remove_from_workspace(&mut self, root: &Path) {
        self.listed.retain(|l| l.root != root);
        let Some(active) = &self.workspace else {
            return;
        };
        let result = active.repo.workspace().and_then(|mut workspace| {
            (workspace.collections).retain(|entry| canonical(&active.repo.resolve(entry)) != root);
            active.repo.save_workspace(&workspace)
        });
        if let Err(e) = result {
            self.snapshot.error = Some(e.say(self.strings));
        }
    }

    /// `workspace.yml` changed outside apifast: collections added there
    /// appear, and those removed close.
    pub(super) fn workspace_changed(&mut self, opened: u64) {
        let Some(active) = self.workspace.as_ref().filter(|w| w.opened == opened) else {
            return;
        };
        let Ok(workspace) = active.repo.workspace() else {
            // Half written: the next change will tell.
            return;
        };
        let state = WorkspaceStateData {
            expanded: (self.listed.iter())
                .filter(|l| l.expanded)
                .map(|l| l.root.clone())
                .collect(),
            ..WorkspaceStateData::default()
        };
        let listed = listed(&active.repo, &workspace, &state);
        let gone: Vec<PathBuf> = (self.open.keys())
            .filter(|root| !listed.iter().any(|l| l.root == **root))
            .cloned()
            .collect();
        for root in gone {
            self.unload(&root);
        }
        if let Some(view) = &mut self.snapshot.workspace {
            view.name = workspace.name;
        }
        self.listed = listed;
    }

    /// The variables of the active global environment, for the sends. A
    /// broken `extends` says why and gives none.
    pub(super) fn global_variables(&mut self) -> Arc<Vec<Variable>> {
        let Some(active) = &self.workspace else {
            return Arc::default();
        };
        let name = active.global_environment.as_deref();
        match apifast_app::global_variables(&active.global_files, name) {
            Ok(variables) => Arc::new(variables),
            Err(e) => {
                self.snapshot.error = Some(e.say(self.strings));
                Arc::default()
            }
        }
    }

    /// Makes `name` the active global environment, or none, and every tab
    /// sees it.
    pub(super) fn select_global_environment(&mut self, name: Option<String>) {
        let Some(active) = &mut self.workspace else {
            return;
        };
        active.global_environment =
            name.filter(|n| active.global_files.iter().any(|e| &e.name == n));
        self.snapshot
            .global_environment
            .clone_from(&active.global_environment);
        self.write_workspace_state();
        self.spread_globals();
    }

    /// Gives every loaded collection the active global environment's
    /// variables, and works out again what its tabs see.
    pub(super) fn spread_globals(&mut self) {
        let globals = self.global_variables();
        for open in self.open.values_mut() {
            open.globals = globals.clone();
        }
        self.refresh_variables(None);
    }

    /// Changes the workspace's global environments, reads them again and
    /// gives every tab what they now say. The active one follows a rename
    /// and is none once deleted.
    pub(super) fn global_environment(&mut self, command: &super::GlobalCommand) {
        use super::GlobalCommand;
        let Some(active) = &self.workspace else {
            return;
        };
        let repo = active.repo.clone();
        let result = match command {
            GlobalCommand::Create(name) => apifast_app::create_environment(&repo, name).map(|_| ()),
            GlobalCommand::Rename { from, to } => {
                apifast_app::rename_environment(&repo, from, to).map(|_| ())
            }
            GlobalCommand::Save(environment) => apifast_app::save_environment(&repo, environment),
            GlobalCommand::Delete(name) => apifast_app::delete_environment(&repo, name),
        };
        if let Err(e) = result {
            self.snapshot.error = Some(e.say(self.strings));
        }
        if let (GlobalCommand::Rename { from, to }, Some(active)) = (command, &mut self.workspace)
            && active.global_environment.as_deref() == Some(from.as_str())
        {
            active.global_environment = Some(to.clone());
        }
        self.reload_globals();
    }

    /// The workspace's `environments/`, or the folder itself, changed
    /// outside apifast (or by it): reads them again, and watches the
    /// folder once it exists.
    pub(super) fn global_environments_changed(&mut self, opened: u64) {
        let Some(active) = self.workspace.as_mut().filter(|w| w.opened == opened) else {
            return;
        };
        if active.globals_watcher.is_none() {
            active.globals_watcher =
                watch_globals(active.repo.folder(), opened, self.messages.clone());
        }
        self.reload_globals();
    }

    /// Reads the global environments again and gives every tab what they
    /// say. An active one that is gone is none.
    fn reload_globals(&mut self) {
        let Some(active) = &self.workspace else {
            return;
        };
        let files = active.repo.environments().unwrap_or_else(|e| {
            self.snapshot.error = Some(e.say(self.strings));
            Vec::new()
        });
        let Some(active) = &mut self.workspace else {
            return;
        };
        active.global_environment =
            (active.global_environment.take()).filter(|name| files.iter().any(|e| &e.name == name));
        active.global_files = Arc::new(files);
        self.snapshot.global_environment_files = active.global_files.clone();
        self.snapshot
            .global_environment
            .clone_from(&active.global_environment);
        self.write_workspace_state();
        self.spread_globals();
    }

    /// Writes what this machine keeps of the active workspace.
    pub(super) fn write_workspace_state(&mut self) {
        let Some(active) = &self.workspace else {
            return;
        };
        let state = WorkspaceStateData {
            expanded: (self.listed.iter())
                .filter(|l| l.expanded)
                .map(|l| l.root.clone())
                .collect(),
            tabs: (self.snapshot.tabs.iter())
                .map(|tab| (tab.root.clone(), tab.path.clone()))
                .collect(),
            active: (self.snapshot.active.as_ref()).map(|key| (key.root.clone(), key.path.clone())),
            global_environment: active.global_environment.clone(),
        };
        if let Err(e) = active.state.save(&state) {
            self.snapshot.error = Some(e.say(self.strings));
        }
    }
}

/// The folder of a workspace's global environments.
const GLOBALS_DIR: &str = "environments";

/// Watches the global environments of the workspace in `folder`, opening
/// number `opened`; `None` while it has no `environments/`.
fn watch_globals(
    folder: &Path,
    opened: u64,
    messages: tokio::sync::mpsc::WeakUnboundedSender<Message>,
) -> Option<FsWatcher> {
    let dir = folder.join(GLOBALS_DIR);
    if !dir.is_dir() {
        return None;
    }
    FsWatcher::start_shallow(&dir, move |_| {
        if let Some(messages) = messages.upgrade() {
            let _ = messages.send(Message::GlobalEnvironmentsChanged(opened));
        }
    })
    .ok()
}

/// The collections of `workspace`, expanded as `state` says.
fn listed(repo: &FsWorkspace, workspace: &Workspace, state: &WorkspaceStateData) -> Vec<Listed> {
    let mut out: Vec<Listed> = Vec::new();
    for entry in &workspace.collections {
        let root = canonical(&repo.resolve(entry));
        if out.iter().any(|l| l.root == root) {
            continue;
        }
        out.push(Listed {
            expanded: state.expanded.contains(&root),
            root,
            name: entry.name.clone(),
            error: None,
        });
    }
    out
}
