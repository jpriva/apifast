//! The collections of the workspace: loading and unloading them, their
//! trees and environments, changes made outside apifast, and what the
//! snapshot shows of them.

use crate::messages::Say as _;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::{
    AppError, CollectionTree, EnvironmentRepository, HistoryEntry, ItemPath, RepositoryError,
    RequestPath, Session, SessionStore, SharedSecretFiles, create_environment, delete_environment,
    load_request, open_collection, rename_environment, save_environment,
};
use apifast_domain::{Collection, Environment, HttpRequest, Variable};
use apifast_history::{FsHistoryStore, FsResponseStore};
use apifast_http::ReqwestTransport;
use apifast_storage_fs::{FsCollection, FsSessionStore};
use apifast_vault::VaultSecrets;
use apifast_watch::{Changes, FsWatcher};

use super::{
    COLLECTION_FILE, CollectionEntry, CollectionView, Command, Message, State, Stores, Tab, TabKey,
    VaultView, stores,
};

/// A loaded collection: its ports and what the interface shows of it.
pub(super) struct Open {
    pub(super) repo: Arc<FsCollection>,
    pub(super) transport: Arc<ReqwestTransport>,
    pub(super) responses: Arc<FsResponseStore>,
    pub(super) history: Arc<FsHistoryStore>,
    pub(super) sessions: FsSessionStore,
    /// Numbers this loading, so late results for another are ignored.
    pub(super) opened: u64,
    /// The collection's secrets. `None` without a collection id: no secret
    /// was ever stored.
    pub(super) vault: Option<Arc<VaultSecrets>>,
    /// Values captured from responses, for the next sends. They last while
    /// the collection is loaded.
    pub(super) runtime: Vec<Variable>,
    /// What scripts changed in the environment, the collection and the
    /// globals, which the next scripts read; for as long as the runtime.
    pub(super) script_changes: Vec<apifast_app::VariableChange>,
    /// The names in `runtime` a script set last, not a capture.
    pub(super) scripted: std::collections::BTreeSet<String>,
    /// OAuth 2.0 tokens, kept while the collection is loaded.
    pub(super) tokens: Arc<dyn apifast_app::TokenStore>,
    /// Its cookie jar, shared with its transport.
    pub(super) cookies: Arc<apifast_app::Cookies>,
    /// Where its jar is kept.
    pub(super) cookie_store: Arc<dyn apifast_app::CookieStore>,
    /// What the interface sees of its jar, as of the last change.
    pub(super) cookie_view: Arc<Vec<apifast_domain::Cookie>>,
    /// Values captured for the active environment, waiting for the user.
    pub(super) captured: Option<super::captures::Captured>,
    /// Its folders and requests.
    pub(super) tree: Arc<CollectionTree>,
    /// Its environments, as in their files.
    pub(super) environment_files: Arc<Vec<Environment>>,
    /// Its active environment.
    pub(super) environment: Option<String>,
    /// Its sends, oldest first.
    pub(super) entries: Arc<Vec<HistoryEntry>>,
    /// What the interface sees of its vault.
    pub(super) vault_view: VaultView,
    /// Unsaved changes of requests without a tab, kept until the request
    /// is opened again.
    pub(super) parked: HashMap<RequestPath, HttpRequest>,
    /// Where its GraphQL schemas are kept between sessions.
    pub(super) schema_cache: Arc<apifast_history::FsSchemaCache>,
    /// Its GraphQL schemas, by [`apifast_app::schema_key`], as read or
    /// asked for in this session.
    pub(super) schemas: HashMap<String, super::schema::SchemaView>,
    /// The methods of its gRPC requests, by [`super::methods::key_of`].
    pub(super) methods: HashMap<String, super::methods::MethodsView>,
    /// Its git repository, if it is in one.
    pub(super) git: Option<Arc<dyn apifast_app::GitRepository>>,
    /// What the interface sees of its repository.
    pub(super) git_view: Option<super::GitView>,
    /// Watches its folder. `None` if the system refused: changes made
    /// outside then show after loading it again.
    pub(super) _watcher: Option<FsWatcher>,
}

/// A collection of the workspace, loaded or not.
pub(super) struct Listed {
    /// Its folder.
    pub(super) root: PathBuf,
    /// Its name in `workspace.yml`.
    pub(super) name: String,
    /// Whether it is expanded in the tree.
    pub(super) expanded: bool,
    /// Why it could not be loaded.
    pub(super) error: Option<String>,
}

/// `path` without `.` and `..` and symbolic links, when it exists.
pub(super) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

impl State {
    /// The focused collection.
    pub(super) fn focused(&self) -> Option<&Open> {
        self.open.get(self.focus.as_ref()?)
    }

    /// The focused collection, to change it.
    pub(super) fn focused_mut(&mut self) -> Option<&mut Open> {
        self.open.get_mut(self.focus.as_ref()?)
    }

    /// The folder of the collection loaded as number `opened`.
    pub(super) fn root_of(&self, opened: u64) -> Option<PathBuf> {
        (self.open.iter())
            .find(|(_, open)| open.opened == opened)
            .map(|(root, _)| root.clone())
    }

    /// Fills what the snapshot shows of the workspace and of the focused
    /// collection from where they are kept.
    pub(super) fn mirror(&mut self) {
        self.snapshot.collections = (self.listed.iter())
            .map(|listed| {
                let open = self.open.get(&listed.root);
                CollectionEntry {
                    root: listed.root.clone(),
                    name: open.map_or_else(
                        || listed.name.clone(),
                        |open| open.tree.collection.name.clone(),
                    ),
                    expanded: listed.expanded,
                    tree: open.map(|open| open.tree.clone()),
                    error: listed.error.clone(),
                    needs_key: open
                        .is_some_and(|open| open.vault_view.shares && !open.vault_view.has_key),
                }
            })
            .collect();
        let focused = (self.focus.as_ref()).and_then(|root| Some((root, self.open.get(root)?)));
        let snapshot = &mut self.snapshot;
        if let Some((root, open)) = focused {
            snapshot.collection = Some(CollectionView {
                root: root.clone(),
                tree: open.tree.clone(),
            });
            snapshot.environments = (open.environment_files.iter())
                .map(|e| e.name.clone())
                .collect();
            snapshot.environment_files = open.environment_files.clone();
            snapshot.environment.clone_from(&open.environment);
            snapshot.history = open.entries.clone();
            snapshot.vault = open.vault_view.clone();
            snapshot.cookies = open.cookie_view.clone();
            snapshot.git.clone_from(&open.git_view);
            snapshot.captured = open.captured.as_ref().map(super::captures::Captured::view);
        } else {
            snapshot.captured = None;
            snapshot.collection = None;
            snapshot.environments = Vec::new();
            snapshot.environment_files = Arc::default();
            snapshot.environment = None;
            snapshot.history = Arc::default();
            snapshot.cookies = Arc::default();
            snapshot.git = None;
            snapshot.vault = VaultView {
                master_in_file: snapshot.vault.master_in_file,
                ..VaultView::default()
            };
        }
    }

    /// Opens the collection in `root`, adding it to the workspace, or tells
    /// why not; a folder without a collection is remembered, so the
    /// interface can offer to create one.
    pub(super) fn open_or_report(&mut self, root: &Path) {
        let root = canonical(root);
        let missing = root.is_dir() && !root.join(COLLECTION_FILE).exists();
        match self.load(&root) {
            Ok(()) => {
                self.add_to_workspace(&root);
                self.set_expanded(&root, true);
                self.focus = Some(root);
                self.snapshot.error = None;
                self.snapshot.missing_collection = None;
            }
            Err(message) => {
                self.snapshot.error = Some(message);
                self.snapshot.missing_collection = missing.then_some(root);
            }
        }
    }

    /// Creates a collection named `name` in `folder` and opens it.
    pub(super) fn create_collection(&mut self, folder: &Path, name: String) {
        match FsCollection::create(folder, &Collection::new(name)) {
            Ok(_) => self.open_or_report(folder),
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
    }

    /// Makes the collection at `root` the focused one, loading it.
    pub(super) fn focus_on(&mut self, root: &Path) {
        let root = canonical(root);
        match self.load(&root) {
            Ok(()) => self.focus = Some(root),
            Err(message) => self.snapshot.error = Some(message),
        }
    }

    /// Expands or collapses the collection at `root`; expanding loads it.
    pub(super) fn expand(&mut self, root: &Path, expanded: bool) {
        let root = canonical(root);
        self.set_expanded(&root, expanded);
        if expanded {
            match self.load(&root) {
                Ok(()) => self.focus = Some(root),
                Err(message) => self.snapshot.error = Some(message),
            }
        }
    }

    pub(super) fn set_expanded(&mut self, root: &Path, expanded: bool) {
        if let Some(listed) = self.listed.iter_mut().find(|l| l.root == root) {
            listed.expanded = expanded;
        }
    }

    /// Unloads the collection at `root` and closes its tabs; their unsaved
    /// changes stay in its session.
    pub(super) fn close_collection(&mut self, root: &Path) {
        let root = canonical(root);
        self.unload(&root);
        self.set_expanded(&root, false);
    }

    /// Closes the collection at `root` and removes it from the workspace.
    pub(super) fn remove_collection(&mut self, root: &Path) {
        let root = canonical(root);
        self.unload(&root);
        self.remove_from_workspace(&root);
    }

    /// Unloads the collection at `root`: its tabs close, their drafts are
    /// parked in its session, its sends are cancelled.
    pub(super) fn unload(&mut self, root: &Path) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        for tab in self.snapshot.tabs.iter().filter(|tab| tab.root == root) {
            if let Some(draft) = &tab.draft {
                open.parked.insert(tab.path.clone(), draft.clone());
            }
        }
        self.write_collection_session(root);
        let keys: Vec<TabKey> = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root)
            .map(Tab::key)
            .collect();
        for key in keys {
            self.close_tab(&key);
        }
        self.open.remove(root);
        if self.focus.as_deref() == Some(root) {
            self.focus = (self.snapshot.active.as_ref())
                .map(|key| key.root.clone())
                .or_else(|| self.open.keys().next().cloned());
        }
    }

    /// Loads the collection at `root` if it is not loaded: its tree,
    /// environments, stores, session and watcher.
    pub(super) fn load(&mut self, root: &Path) -> Result<(), String> {
        if self.open.contains_key(root) {
            return Ok(());
        }
        let result = self.try_load(root);
        match &result {
            Ok(()) => tracing::info!(collection = %root.display(), "collection opened"),
            Err(error) => tracing::warn!(collection = %root.display(), "cannot open: {error}"),
        }
        if let Some(listed) = self.listed.iter_mut().find(|l| l.root == root) {
            listed.error = result.as_ref().err().cloned();
        }
        result
    }

    fn try_load(&mut self, root: &Path) -> Result<(), String> {
        let repo = Arc::new(FsCollection::open(root).map_err(|e| e.say(self.strings))?);
        let tree = open_collection(&*repo).map_err(|e| e.say(self.strings))?;
        let environment_files = repo.environments().map_err(|e| e.say(self.strings))?;
        let id = tree
            .collection
            .id
            .as_ref()
            .map(apifast_domain::CollectionId::as_str);
        let shared: Arc<dyn SharedSecretFiles> = repo.clone();
        let Stores {
            responses,
            sessions,
            history,
            vault,
            tokens,
            cookies: cookie_store,
            schemas: schema_cache,
        } = stores(&self.data_dir, root, id, (shared, self.keyring))?;
        let (cookies, cookie_error) = super::cookies::load_jar(&*cookie_store);
        cookies.set_policy(self.cookie_policy.0, self.cookie_policy.1);
        let cookies = Arc::new(cookies);
        let transport = self.transport_for(root, &tree.collection, cookies.clone())?;

        let (session, error) = match sessions.load() {
            Ok(session) => (session, cookie_error),
            Err(e) => (Session::default(), Some(e.say(self.strings))),
        };
        let parked: HashMap<RequestPath, HttpRequest> = (session.tabs.into_iter())
            .filter_map(|tab| Some((tab.path, tab.draft?)))
            .collect();
        let environment =
            (session.environment).filter(|name| environment_files.iter().any(|e| e.name == *name));

        let opened = self.next_revision();
        let (responses, history) = (Arc::new(responses), Arc::new(history));
        self.load_history(opened, responses.clone(), history.clone());
        let messages = self.messages.clone();
        let watcher = FsWatcher::start(root, move |changes| {
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::External(opened, changes));
            }
        });
        let (watcher, error) = match watcher {
            Ok(watcher) => (Some(watcher), error),
            Err(e) => (None, error.or_else(|| Some(e.to_string()))),
        };
        self.open.insert(
            root.to_path_buf(),
            Open {
                repo,
                transport: Arc::new(transport),
                responses,
                history,
                sessions,
                opened,
                vault,
                runtime: Vec::new(),
                script_changes: Vec::new(),
                scripted: std::collections::BTreeSet::new(),
                tokens,
                cookie_view: Arc::new(cookies.jar().cookies().to_vec()),
                cookies,
                cookie_store,
                captured: None,
                tree: Arc::new(tree),
                environment_files: Arc::new(environment_files),
                environment,
                entries: Arc::default(),
                vault_view: VaultView::default(),
                parked,
                schema_cache: Arc::new(schema_cache),
                schemas: HashMap::new(),
                methods: HashMap::new(),
                git: super::git::open_git(root),
                git_view: None,
                _watcher: watcher,
            },
        );
        if error.is_some() {
            self.snapshot.error = error;
        }
        self.refresh_vault_of(root);
        self.refresh_git(root);
        Ok(())
    }

    /// Reopens the tab `key` from a saved state, with its parked draft.
    /// Does nothing if its collection cannot be loaded or the request is
    /// gone without a draft.
    pub(super) fn restore_tab(&mut self, key: &TabKey) {
        if self.tab(key).is_some() || self.load(&key.root).is_err() {
            return;
        }
        let Some(open) = self.open.get_mut(&key.root) else {
            return;
        };
        let draft = open.parked.remove(&key.path);
        let (saved, draft) = match load_request(&*open.repo, &key.path) {
            Ok(loaded) => (loaded.request, draft),
            // The file is gone but its changes are not: saving writes it
            // again.
            Err(_) => match draft {
                Some(draft) => (draft.clone(), Some(draft)),
                None => return,
            },
        };
        let mut tab = self.new_tab(key, saved.clone(), None);
        tab.draft = draft.filter(|d| *d != saved);
        self.snapshot.tabs.push(tab);
        self.refresh_variables(Some(key));
    }

    /// Commands about the focused collection's environments.
    pub(super) fn handle_environment(&mut self, command: Command) {
        let Some(root) = self.focus.clone() else {
            return;
        };
        match command {
            Command::CreateEnvironment(name) => {
                self.environments(&root, |repo| create_environment(repo, &name).map(|_| ()));
            }
            Command::RenameEnvironment { from, to } => {
                let was_active = (self.open.get(&root))
                    .is_some_and(|open| open.environment.as_deref() == Some(from.as_str()));
                let mut renamed = None;
                self.environments(&root, |repo| {
                    renamed = Some(rename_environment(repo, &from, &to)?);
                    Ok(())
                });
                if let (Some(to), Some(open)) = (&renamed, self.open.get_mut(&root)) {
                    // The secrets' associated data hold the environment's
                    // name: seal them again for the new one.
                    if let Some(Err(e)) =
                        (open.vault.clone()).map(|vault| vault.rename_environment(&from, to))
                    {
                        self.snapshot.error = Some(e.say(self.strings));
                    }
                    if was_active {
                        open.environment = Some(to.clone());
                    }
                }
                self.refresh_collection(&root);
            }
            Command::SaveEnvironment(environment) => {
                self.environments(&root, |repo| save_environment(repo, &environment));
                self.refresh_collection(&root);
            }
            Command::DeleteEnvironment(name) => {
                self.environments(&root, |repo| delete_environment(repo, &name));
                self.refresh_collection(&root);
            }
            _ => {}
        }
    }

    /// Runs `change` on the environments of the collection at `root`,
    /// reports its error, and reads them again.
    fn environments(
        &mut self,
        root: &Path,
        change: impl FnOnce(&FsCollection) -> Result<(), AppError>,
    ) {
        let Some(repo) = self.open.get(root).map(|open| open.repo.clone()) else {
            return;
        };
        match change(&repo) {
            Ok(()) => self.snapshot.error = None,
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
        self.reload_environments(root);
    }

    /// Reads the environments of the collection at `root` again.
    pub(super) fn reload_environments(&mut self, root: &Path) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        match open.repo.environments() {
            Ok(environments) => {
                // An active environment that is gone is no longer active.
                if let Some(active) = &open.environment
                    && !environments.iter().any(|e| e.name == *active)
                {
                    open.environment = None;
                }
                open.environment_files = Arc::new(environments);
            }
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
        self.refresh_vault_of(root);
    }

    /// Saves `scripts` as those of `folder`, or of the collection at
    /// `root`, keeping the rest of its file.
    pub(super) fn save_scripts(
        &mut self,
        root: &Path,
        folder: Option<&str>,
        scripts: Vec<apifast_domain::ScriptBlock>,
    ) {
        self.save_defaults(root, folder, |defaults| defaults.scripts = scripts);
    }

    /// Changes with `change` what `folder`, or the collection at `root`,
    /// gives every request inside, keeping the rest of its file.
    pub(super) fn save_defaults(
        &mut self,
        root: &Path,
        folder: Option<&str>,
        change: impl FnOnce(&mut apifast_domain::RequestDefaults),
    ) {
        let found = (self.open.get(root)).and_then(|open| match folder {
            None => Some(None),
            Some(path) => find_folder(&open.tree.items, path).map(Some),
        });
        let Some(found) = found else {
            return;
        };
        self.organize(root, |repo| {
            use apifast_app::CollectionRepository as _;
            if let (Some(path), Some(mut saved)) = (folder, found) {
                change(&mut saved.defaults);
                repo.save_folder(path, &saved)?;
            } else {
                let mut collection = repo.collection()?;
                change(&mut collection.defaults);
                repo.save_collection(&collection)?;
            }
            Ok(())
        });
    }

    pub(super) fn reload_tree(&mut self, root: &Path) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        match open_collection(&*open.repo) {
            Ok(tree) => open.tree = Arc::new(tree),
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
    }

    /// Runs a use case that changes the files of the collection at `root`,
    /// then reloads its tree. Returns the use case's result, or `None`
    /// after reporting its error.
    pub(super) fn organize<T>(
        &mut self,
        root: &Path,
        change: impl FnOnce(&FsCollection) -> Result<T, AppError>,
    ) -> Option<T> {
        let repo = self.open.get(root)?.repo.clone();
        let result = change(&repo);
        // Reload even after an error: part of the change may have happened.
        self.reload_tree(root);
        match result {
            Ok(value) => {
                self.snapshot.error = None;
                Some(value)
            }
            Err(e) => {
                self.snapshot.error = Some(e.say(self.strings));
                None
            }
        }
    }

    /// After `old` moved to `new` in the collection at `root`, the tabs of
    /// `old` and of everything inside it follow, with their drafts and
    /// sends in progress. Their saved contents are read again: the name may
    /// have changed.
    pub(super) fn follow_move(&mut self, root: &Path, old: &ItemPath, new: &ItemPath) {
        let Some(repo) = self.open.get(root).map(|open| open.repo.clone()) else {
            return;
        };
        let moved = |path: &RequestPath| {
            ItemPath::Request(path.clone()).is_within(old).then(|| {
                RequestPath::new(format!(
                    "{}{}",
                    new.as_str(),
                    &path.as_str()[old.as_str().len()..]
                ))
            })
        };
        let mut follows = Vec::new();
        for (index, tab) in self.snapshot.tabs.iter().enumerate() {
            if tab.root == root
                && let Some(to) = moved(&tab.path)
            {
                follows.push((index, to));
            }
        }
        for (index, to) in follows {
            let revision = self.next_revision();
            let from = self.snapshot.tabs[index].key();
            let to_key = TabKey::new(root, to.clone());
            if let Some(send) = self.sending.remove(&from) {
                self.sending.insert(to_key.clone(), send);
            }
            if self.snapshot.active.as_ref() == Some(&from) {
                self.snapshot.active = Some(to_key.clone());
            }
            let tab = &mut self.snapshot.tabs[index];
            match load_request(&*repo, &to) {
                Ok(loaded) => {
                    // A rename changes the name on disk; the draft keeps it,
                    // or saving it would undo the rename.
                    if let Some(draft) = &mut tab.draft
                        && tab.saved.name != loaded.request.name
                    {
                        draft.name.clone_from(&loaded.request.name);
                    }
                    tab.saved = loaded.request;
                }
                Err(e) => self.snapshot.error = Some(e.say(self.strings)),
            }
            tab.path = to;
            tab.revision = revision;
            self.refresh_variables(Some(&to_key));
        }
    }

    /// Reconciles files of the collection at `root` changed outside
    /// apifast with what is open. A file that reads as what the tab already
    /// holds was written by apifast itself, and changes nothing.
    pub(super) fn apply_external(&mut self, root: &Path, changes: &Changes) {
        let Some(repo) = self.open.get(root).map(|open| open.repo.clone()) else {
            return;
        };
        // What changed in the files may be a change for git too.
        self.refresh_git(root);
        // A changed path is a file, or a folder renamed or removed whole.
        let touched = |path: &str| {
            changes.rescan
                || (changes.paths.iter())
                    .any(|changed| path == changed || path.starts_with(&format!("{changed}/")))
        };
        let shared = changes.rescan
            || changes.paths.iter().any(|path| {
                path == COLLECTION_FILE
                    || path == ".env"
                    || path == "environments"
                    || path.starts_with("environments/")
                    || path.ends_with("folder.yml")
            });
        self.reload_tree(root);
        if shared {
            self.reload_environments(root);
        }
        let tabs: Vec<TabKey> = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root && touched(tab.path.as_str()))
            .map(Tab::key)
            .collect();
        for key in tabs {
            match load_request(&*repo, &key.path) {
                Ok(loaded) => self.take_external(&key, loaded.request),
                Err(AppError::Repository(RepositoryError::NotFound(_))) => {
                    // Gone. Changes not saved stay: saving writes the file
                    // again. Without them, there is nothing left to show.
                    if self.tab(&key).is_some_and(|tab| tab.draft.is_none()) {
                        self.close_tab(&key);
                    }
                }
                // Half written, or not valid yet: the next change will tell.
                Err(_) => {}
            }
        }
        if shared {
            self.refresh_collection(root);
        }
        // Secrets shared by a teammate, or a new challenge, came with git.
        if changes.rescan || changes.paths.iter().any(|p| p.starts_with(".apifast")) {
            self.refresh_vault_of(root);
        }
    }

    /// The file of tab `key` now holds `request`.
    fn take_external(&mut self, key: &TabKey, request: HttpRequest) {
        let revision = self.next_revision();
        let Some(tab) = self.tab_mut(key) else {
            return;
        };
        if request == tab.saved {
            return;
        }
        match &tab.draft {
            None => tab.revision = revision,
            // The file now says what the draft says: nothing to settle.
            Some(draft) if *draft == request => {
                tab.draft = None;
                tab.conflict = false;
                tab.revision = revision;
            }
            // Both changed: keep the draft on screen and ask.
            Some(_) => tab.conflict = true,
        }
        tab.saved = request;
        self.refresh_variables(Some(key));
    }
}

/// The folder at `path` in `items`, as the tree read it.
fn find_folder(items: &[apifast_app::TreeItem], path: &str) -> Option<apifast_domain::Folder> {
    items.iter().find_map(|item| match item {
        apifast_app::TreeItem::Folder {
            path: p, folder, ..
        } if p == path => Some(folder.clone()),
        apifast_app::TreeItem::Folder { children, .. } => find_folder(children, path),
        _ => None,
    })
}
