//! The bridge between the interface and the use cases.
//!
//! [`Core`] runs on its own thread with a tokio runtime. The interface sends
//! it [`Command`]s and reads the latest [`Snapshot`], an immutable value the
//! core replaces whenever something changes. The interface never waits on
//! I/O: it only reads snapshots and sends commands.
//!
//! The open tabs and their unsaved drafts are kept in a session outside the
//! collection, written shortly after each change and when the core stops,
//! and restored when the collection is opened again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use apifast_app::{
    AppError, CancelToken, Clock, CollectionRepository, CollectionTree, EnvironmentRepository,
    HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, ItemPath, NoPrompter, NoSecrets,
    Placement, RepositoryError, RequestPath, ResponseHead, ResponseId, ResponseStore, SecretStore,
    SendOptions, SendPorts, Session, SessionStore, SessionTab, SharedSecretFiles, TransportError,
    create_environment, create_folder, create_request, delete_environment, delete_item,
    load_request, move_item, open_collection, rename_environment, rename_item,
    resolve_request_variables, save_environment, save_request, send_request,
};
use apifast_domain::{
    AuthScheme, Chain, Collection, Environment, Header, HttpRequest, Origin, Resolved,
    ResolvedSettings, ResolvedVariables, Setting, Settings, resolve,
};
use apifast_history::{FsHistoryStore, FsResponseStore, Limits};
use apifast_http::ReqwestTransport;
use apifast_storage_fs::{FsCollection, FsSessionStore};
use apifast_vault::VaultSecrets;
use apifast_watch::{Changes, FsWatcher};
use tokio::sync::mpsc;

use crate::body::{BodyHandle, BodySource};
use crate::system::{SystemClock, SystemRandom};

/// How much history is kept by default: entries beyond these are removed
/// when a collection opens (see [`Command::Configure`]).
pub const HISTORY_LIMITS: HistoryLimits = HistoryLimits {
    max_entries: Some(1000),
    max_age: Some(Duration::from_hours(30 * 24)),
};

/// How many response bodies are kept, removed when a collection opens.
pub const RESPONSE_LIMITS: Limits = Limits {
    max_count: Some(1000),
    max_total_bytes: Some(512 * 1024 * 1024),
    max_age: Some(Duration::from_hours(30 * 24)),
};

/// A body kept in the response store, read by windows from disk.
struct StoredBody {
    store: Arc<FsResponseStore>,
    id: ResponseId,
    size: u64,
}

/// The stores of the collection at `root`, all under `data_dir` and keyed
/// by its id (or a hash of its path): responses, session, history and
/// secrets.
fn stores(
    data_dir: &Path,
    root: &Path,
    id: Option<&str>,
    shared: Arc<dyn SharedSecretFiles>,
) -> Result<Stores, String> {
    let responses =
        FsResponseStore::new(apifast_history::dirs::responses_dir_in(data_dir, root, id));
    let sessions = FsSessionStore::new(apifast_history::dirs::sessions_dir_in(data_dir, root, id));
    let history = FsHistoryStore::new(apifast_history::dirs::history_file_in(data_dir, root, id));
    // Secrets need the collection's id: without one, none was ever stored.
    let secrets = id
        .map(|id| VaultSecrets::for_collection(data_dir, id, shared).map(Arc::new))
        .transpose()
        .map_err(|e| e.to_string())?;
    Ok(Stores {
        responses,
        sessions,
        history,
        vault: secrets,
    })
}

/// What [`stores`] makes.
struct Stores {
    responses: FsResponseStore,
    sessions: FsSessionStore,
    history: FsHistoryStore,
    vault: Option<Arc<VaultSecrets>>,
}

/// A response whose body is in `store`.
fn received(
    store: &Arc<FsResponseStore>,
    head: ResponseHead,
    size: u64,
    id: ResponseId,
) -> Received {
    let stored = StoredBody {
        store: store.clone(),
        id: id.clone(),
        size,
    };
    Received {
        head,
        size,
        body: BodyHandle::new(id.0, Arc::new(stored)),
    }
}

/// What a tab shows for a history entry. With `kept_only`, a response whose
/// body was removed gives nothing.
fn response_of(
    store: &Arc<FsResponseStore>,
    entry: &HistoryEntry,
    kept_only: bool,
) -> Option<ResponseState> {
    match &entry.outcome {
        HistoryOutcome::Received { head, size, body } => {
            if kept_only && store.size(body).is_err() {
                return None;
            }
            Some(ResponseState::Received(received(
                store,
                head.clone(),
                *size,
                body.clone(),
            )))
        }
        HistoryOutcome::Failed(message) => Some(ResponseState::Failed(message.clone())),
    }
}

impl BodySource for StoredBody {
    fn len(&self) -> u64 {
        self.size
    }

    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        self.store
            .read(&self.id, offset, len)
            .map_err(|error| error.to_string())
    }
}

/// How long the core waits after a change before writing the session, so
/// that typing does not write a file per key.
const SESSION_DELAY: Duration = Duration::from_millis(300);

/// What the interface asks the core to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Opens the collection in this folder, with the tabs it had open.
    OpenCollection(PathBuf),
    /// Picks the active environment, or none.
    SelectEnvironment(Option<String>),
    /// Opens a request in a tab, or shows its tab if it is open.
    OpenRequest(RequestPath),
    /// Shows an open tab.
    ActivateTab(RequestPath),
    /// Closes a tab, discarding its unsaved changes.
    CloseTab(RequestPath),
    /// Replaces the contents of an open tab with the user's edits. They
    /// stay a draft until saved.
    Edit {
        /// The tab.
        path: RequestPath,
        /// Its new contents. Boxed: an edit is much larger than any other
        /// command.
        request: Box<HttpRequest>,
    },
    /// Saves the draft of a tab to its file.
    Save(RequestPath),
    /// Saves every draft.
    SaveAll,
    /// Sends the active tab, as it is on screen (with its draft).
    Send,
    /// Cancels the send of the active tab.
    Cancel,
    /// Creates a request called `name` in `folder` (`""` for the root) and
    /// opens it.
    CreateRequest {
        /// Where.
        folder: String,
        /// Its name.
        name: String,
    },
    /// Creates a folder called `name` in `parent` (`""` for the root).
    CreateFolder {
        /// Where.
        parent: String,
        /// Its name.
        name: String,
    },
    /// Renames a request or folder.
    Rename {
        /// What.
        item: ItemPath,
        /// The new name.
        name: String,
    },
    /// Deletes a request or folder, with everything inside it.
    Delete(ItemPath),
    /// Moves a request or folder.
    Move {
        /// What.
        item: ItemPath,
        /// Where to.
        placement: Placement,
    },
    /// Shows entry `index` of [`Snapshot::history`] in its request's tab.
    OpenHistory(usize),
    /// How much history and how many response bodies to keep, from the
    /// settings. Applied when a collection opens.
    Configure {
        /// Of the history.
        history: HistoryLimits,
        /// Of the response bodies.
        responses: Limits,
    },
    /// Creates an empty environment.
    CreateEnvironment(String),
    /// Renames an environment, and the `extends` that name it.
    RenameEnvironment {
        /// Its name.
        from: String,
        /// The new name.
        to: String,
    },
    /// Saves an environment, found by its name.
    SaveEnvironment(Box<Environment>),
    /// Deletes an environment.
    DeleteEnvironment(String),
    /// Settles a tab whose file changed outside apifast while it had a
    /// draft: keep the draft (saving then replaces the file) or take the
    /// file and drop the draft.
    ResolveConflict {
        /// The tab.
        path: RequestPath,
        /// Whether the draft is kept.
        keep_draft: bool,
    },
}

/// Everything the interface shows, as of one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Increases with every change.
    pub version: u64,
    /// The open collection.
    pub collection: Option<CollectionView>,
    /// Names of the collection's environments.
    pub environments: Vec<String>,
    /// The collection's environments, as in their files, in the order of
    /// [`Self::environments`].
    pub environment_files: Arc<Vec<Environment>>,
    /// The active environment.
    pub environment: Option<String>,
    /// Open tabs, in order.
    pub tabs: Vec<Tab>,
    /// The request of the active tab.
    pub active: Option<RequestPath>,
    /// The last error that is not about a send, such as a collection that
    /// cannot be opened.
    pub error: Option<String>,
    /// The sends of the collection, oldest first. Loaded shortly after the
    /// collection opens.
    pub history: Arc<Vec<HistoryEntry>>,
}

impl Snapshot {
    /// The active tab.
    pub fn active_tab(&self) -> Option<&Tab> {
        let active = self.active.as_ref()?;
        self.tabs.iter().find(|tab| tab.path == *active)
    }
}

/// An open collection.
#[derive(Debug, Clone)]
pub struct CollectionView {
    /// Its folder.
    pub root: PathBuf,
    /// Its folders and requests.
    pub tree: CollectionTree,
}

/// An open request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// Its path in the collection.
    pub path: RequestPath,
    /// Its contents as saved.
    pub saved: HttpRequest,
    /// Its unsaved changes, if any.
    pub draft: Option<HttpRequest>,
    /// Its last send.
    pub response: ResponseState,
    /// Changes whenever the contents change for any reason other than an
    /// [`Command::Edit`] (loaded, saved, renamed), so the interface knows
    /// to drop its own copy of them.
    pub revision: u64,
    /// What its request sees from its folders, collection and environment.
    pub context: Arc<TabContext>,
    /// Its file changed outside apifast while it had a draft: the interface
    /// asks which to keep (see [`Command::ResolveConflict`]).
    pub conflict: bool,
}

/// What a tab's request sees from outside itself: the variables of the
/// active environment and every scope, and the headers, auth and settings
/// it inherits, each with the level it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabContext {
    /// Variables by name. Empty when they cannot be worked out, such as
    /// with a broken environment.
    pub variables: ResolvedVariables,
    /// Names of the request's folders, outermost first, for
    /// [`Origin::Folder`].
    pub folders: Vec<String>,
    /// Headers from the collection and folders.
    pub inherited_headers: Vec<Resolved<Header>>,
    /// The auth the request gets when it inherits.
    pub inherited_auth: Resolved<AuthScheme>,
    /// The settings the request gets for each one it inherits.
    pub inherited_settings: ResolvedSettings,
    /// Changes whenever it is worked out again.
    pub revision: u64,
}

impl Default for TabContext {
    fn default() -> Self {
        let nothing = resolve(Chain::new(
            &Collection::new(""),
            &[],
            &HttpRequest::new("", apifast_domain::Method::Get, ""),
        ));
        Self {
            variables: ResolvedVariables::default(),
            folders: Vec::new(),
            inherited_headers: Vec::new(),
            inherited_auth: Resolved::new(AuthScheme::None, Origin::Default),
            inherited_settings: nothing.settings().clone(),
            revision: 0,
        }
    }
}

impl Tab {
    /// The contents on screen: the draft, or the saved request.
    pub fn request(&self) -> &HttpRequest {
        self.draft.as_ref().unwrap_or(&self.saved)
    }

    /// Whether it has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.draft.is_some()
    }
}

/// The state of the last send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ResponseState {
    /// Nothing sent yet.
    #[default]
    Idle,
    /// Waiting for the response.
    Sending,
    /// A response arrived.
    Received(Received),
    /// The send failed; the message is for the user.
    Failed(String),
    /// The user cancelled the send.
    Cancelled,
}

/// A response that arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// Status, headers and timing.
    pub head: ResponseHead,
    /// Size of the whole body.
    pub size: u64,
    /// The body, read by windows where it is kept.
    pub body: BodyHandle,
}

/// Messages the core thread handles: commands from the interface and
/// results of its own tasks.
enum Message {
    Command(Command),
    /// A send ended; the number identifies it in [`State::sending`]. The
    /// entry, already in the history file, is there unless it was cancelled.
    SendFinished(Box<Finished>),
    /// The history of collection number `.0` was pruned and read.
    HistoryLoaded(u64, Vec<HistoryEntry>),
    /// Files of collection number `.0` changed outside apifast.
    External(u64, Changes),
    WriteSession,
}

/// How a send ended: its number, its result and its history entry.
struct Finished {
    id: u64,
    result: Result<Received, AppError>,
    entry: Option<HistoryEntry>,
}

/// Handle to the core thread. Dropping it stops the thread, after writing
/// the session.
pub struct Core {
    messages: mpsc::UnboundedSender<Message>,
    snapshot: Arc<Mutex<Arc<Snapshot>>>,
    thread: Option<JoinHandle<()>>,
}

impl Core {
    /// Starts the core. Responses and sessions are stored under `data_dir`
    /// (normally [`apifast_history::dirs::data_dir`]). `notify` is called
    /// after every new snapshot, from the core thread; the interface uses it
    /// to request a repaint.
    ///
    /// # Panics
    ///
    /// If the thread or its runtime cannot be created (out of resources).
    pub fn start(data_dir: PathBuf, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let snapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let shared = snapshot.clone();
        // Weak, so that only the handle keeps the channel open: dropping the
        // handle must end the loop.
        let own_sender = sender.downgrade();
        let thread = std::thread::Builder::new()
            .name("apifast-core".into())
            .spawn(move || {
                #[expect(clippy::expect_used, reason = "without a runtime nothing can work")]
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("cannot start the async runtime");
                let state = State {
                    data_dir,
                    snapshot: Snapshot::default(),
                    shared,
                    notify: Box::new(notify),
                    open: None,
                    sending: HashMap::new(),
                    revision: 0,
                    session_pending: false,
                    messages: own_sender,
                    limits: (HISTORY_LIMITS, RESPONSE_LIMITS),
                };
                runtime.block_on(state.run(receiver));
            });
        #[expect(
            clippy::expect_used,
            reason = "without the core thread nothing can work"
        )]
        let thread = thread.expect("cannot start the core thread");
        Self {
            messages: sender,
            snapshot,
            thread: Some(thread),
        }
    }

    /// Asks the core to do something. Never blocks.
    pub fn send(&self, command: Command) {
        // Fails only once the core has stopped, when nothing is listening.
        let _ = self.messages.send(Message::Command(command));
    }

    /// The latest snapshot. Cheap: it clones an `Arc`.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // Closing the channel ends the core loop, which writes the session;
        // the runtime then drops any send in progress.
        let (closed, _) = mpsc::unbounded_channel();
        drop(std::mem::replace(&mut self.messages, closed));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The ports of an open collection.
struct Open {
    repo: Arc<FsCollection>,
    transport: Arc<ReqwestTransport>,
    responses: Arc<FsResponseStore>,
    history: Arc<FsHistoryStore>,
    sessions: FsSessionStore,
    /// Numbers this opening, so a late history load for another is ignored.
    opened: u64,
    /// The collection's secrets. `None` without a collection id: no secret
    /// was ever stored.
    vault: Option<Arc<VaultSecrets>>,
    /// Watches the folder while the collection is open. `None` if the
    /// system refused: changes made outside then show after reopening.
    _watcher: Option<FsWatcher>,
}

struct State {
    data_dir: PathBuf,
    snapshot: Snapshot,
    shared: Arc<Mutex<Arc<Snapshot>>>,
    notify: Box<dyn Fn() + Send + Sync>,
    open: Option<Open>,
    /// Sends in progress, by tab: a number for the send, and its token.
    /// The entry moves with its tab.
    sending: HashMap<RequestPath, (u64, CancelToken)>,
    /// Source of [`Tab::revision`] and of send numbers.
    revision: u64,
    /// Whether the session changed and a write is scheduled.
    session_pending: bool,
    /// How much history and how many response bodies are kept; applied
    /// when a collection opens.
    limits: (HistoryLimits, Limits),
    /// To post the results of spawned tasks back to the loop. Weak: a strong
    /// sender here would keep the loop alive after the handle is dropped.
    messages: mpsc::WeakUnboundedSender<Message>,
}

impl State {
    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = receiver.recv().await {
            match message {
                Message::Command(command) => {
                    let touches_session = !matches!(command, Command::Send | Command::Cancel);
                    self.handle(command);
                    if touches_session {
                        self.schedule_session_write();
                    }
                }
                Message::SendFinished(finished) => {
                    let Finished { id, result, entry } = *finished;
                    self.finish_send(id, result, entry);
                }
                Message::HistoryLoaded(opened, entries) => self.history_loaded(opened, entries),
                Message::External(opened, changes) => {
                    if self.open.as_ref().is_some_and(|open| open.opened == opened) {
                        self.apply_external(&changes);
                    }
                }
                Message::WriteSession => {
                    self.session_pending = false;
                    self.write_session();
                }
            }
            self.publish();
        }
        // Stopping: nothing typed is lost.
        if self.session_pending {
            self.write_session();
        }
    }

    fn publish(&mut self) {
        self.snapshot.version += 1;
        *self.shared.lock().unwrap_or_else(PoisonError::into_inner) =
            Arc::new(self.snapshot.clone());
        (self.notify)();
    }

    fn next_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::OpenCollection(root) => {
                if let Err(message) = self.open_collection(root) {
                    self.snapshot.error = Some(message);
                }
            }
            Command::SelectEnvironment(name) => {
                self.snapshot.environment = name;
                self.refresh_variables(None);
            }
            Command::OpenRequest(path) => self.open_request(path),
            Command::ActivateTab(path) => {
                if self.tab(&path).is_some() {
                    self.snapshot.active = Some(path);
                }
            }
            Command::CloseTab(path) => self.close_tab(&path),
            Command::Edit { path, request } => {
                let Some(tab) = self.tab_mut(&path) else {
                    return;
                };
                // Only the request's own variables change what it sees; the
                // URL or a header do not, so typing reads no files.
                let variables_changed = request.variables != tab.request().variables;
                tab.draft = (*request != tab.saved).then_some(*request);
                if variables_changed {
                    self.refresh_variables(Some(&path));
                }
            }
            Command::Save(path) => self.save(&path),
            Command::SaveAll => {
                let dirty: Vec<RequestPath> = (self.snapshot.tabs.iter())
                    .filter(|tab| tab.is_dirty())
                    .map(|tab| tab.path.clone())
                    .collect();
                for path in dirty {
                    self.save(&path);
                }
            }
            Command::Send => self.start_send(),
            Command::Cancel => {
                if let Some((_, token)) =
                    (self.snapshot.active.as_ref()).and_then(|p| self.sending.get(p))
                {
                    token.cancel();
                }
            }
            Command::CreateRequest { folder, name } => {
                if let Some(path) = self.organize(|repo| create_request(repo, &folder, &name)) {
                    self.open_request(path);
                }
            }
            Command::CreateFolder { parent, name } => {
                self.organize(|repo| create_folder(repo, &parent, &name));
            }
            Command::Rename { item, name } => {
                if let Some(new) = self.organize(|repo| rename_item(repo, &item, &name)) {
                    self.follow_move(&item, &new);
                }
            }
            Command::Delete(item) => {
                if self.organize(|repo| delete_item(repo, &item)).is_some() {
                    let inside: Vec<RequestPath> = (self.snapshot.tabs.iter())
                        .map(|tab| tab.path.clone())
                        .filter(|path| ItemPath::Request(path.clone()).is_within(&item))
                        .collect();
                    for path in inside {
                        self.close_tab(&path);
                    }
                }
            }
            Command::Move { item, placement } => {
                if let Some(new) = self.organize(|repo| move_item(repo, &item, &placement)) {
                    self.follow_move(&item, &new);
                }
            }
            Command::OpenHistory(index) => self.open_history(index),
            Command::Configure { history, responses } => self.limits = (history, responses),
            Command::ResolveConflict { path, keep_draft } => {
                let revision = self.next_revision();
                if let Some(tab) = self.tab_mut(&path) {
                    tab.conflict = false;
                    if !keep_draft {
                        tab.draft = None;
                        tab.revision = revision;
                    }
                }
                self.refresh_variables(Some(&path));
            }
            Command::CreateEnvironment(_)
            | Command::RenameEnvironment { .. }
            | Command::SaveEnvironment(_)
            | Command::DeleteEnvironment(_) => self.handle_environment(command),
        }
    }

    fn handle_environment(&mut self, command: Command) {
        match command {
            Command::CreateEnvironment(name) => {
                self.environments(|repo| create_environment(repo, &name).map(|_| ()));
            }
            Command::RenameEnvironment { from, to } => {
                let was_active = self.snapshot.environment.as_deref() == Some(from.as_str());
                let mut renamed = None;
                self.environments(|repo| {
                    renamed = Some(rename_environment(repo, &from, &to)?);
                    Ok(())
                });
                if let Some(to) = &renamed {
                    // The secrets' associated data hold the environment's
                    // name: seal them again for the new one.
                    let vault = self.open.as_ref().and_then(|open| open.vault.clone());
                    if let Some(Err(e)) = vault.map(|vault| vault.rename_environment(&from, to)) {
                        self.snapshot.error = Some(e.to_string());
                    }
                }
                if let (true, Some(to)) = (was_active, renamed) {
                    self.snapshot.environment = Some(to);
                }
                self.refresh_variables(None);
            }
            Command::SaveEnvironment(environment) => {
                self.environments(|repo| save_environment(repo, &environment));
                self.refresh_variables(None);
            }
            Command::DeleteEnvironment(name) => {
                self.environments(|repo| delete_environment(repo, &name));
                self.refresh_variables(None);
            }
            _ => {}
        }
    }

    /// Runs `change` on the environments, reports its error, and reads them
    /// again.
    fn environments(&mut self, change: impl FnOnce(&FsCollection) -> Result<(), AppError>) {
        let Some(open) = &self.open else { return };
        let repo = open.repo.clone();
        match change(&repo) {
            Ok(()) => self.snapshot.error = None,
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
        self.reload_environments();
    }

    /// Reads the environments again.
    fn reload_environments(&mut self) {
        let Some(open) = &self.open else { return };
        match open.repo.environments() {
            Ok(environments) => {
                self.snapshot.environments = environments.iter().map(|e| e.name.clone()).collect();
                self.snapshot.environment_files = Arc::new(environments);
                // An active environment that is gone is no longer active.
                if let Some(active) = &self.snapshot.environment
                    && !self.snapshot.environments.contains(active)
                {
                    self.snapshot.environment = None;
                }
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    fn tab(&self, path: &RequestPath) -> Option<&Tab> {
        self.snapshot.tabs.iter().find(|tab| tab.path == *path)
    }

    fn tab_mut(&mut self, path: &RequestPath) -> Option<&mut Tab> {
        self.snapshot.tabs.iter_mut().find(|tab| tab.path == *path)
    }

    fn open_collection(&mut self, root: PathBuf) -> Result<(), String> {
        let repo = Arc::new(FsCollection::open(&root).map_err(|e| e.to_string())?);
        let tree = open_collection(&*repo).map_err(|e| e.to_string())?;
        let environment_files = repo.environments().map_err(|e| e.to_string())?;
        let environments: Vec<String> = environment_files.iter().map(|e| e.name.clone()).collect();
        let transport = ReqwestTransport::new(&root).map_err(|e| e.to_string())?;
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
        } = stores(&self.data_dir, &root, id, shared)?;

        // Leave the previous collection as it was.
        if self.session_pending {
            self.write_session();
            self.session_pending = false;
        }
        for (_, token) in self.sending.values() {
            token.cancel();
        }
        self.sending.clear();

        let (session, error) = match sessions.load() {
            Ok(session) => (session, None),
            Err(e) => (Session::default(), Some(e.to_string())),
        };
        let mut tabs = Vec::with_capacity(session.tabs.len());
        for SessionTab { path, draft } in session.tabs {
            let (saved, draft) = match load_request(&*repo, &path) {
                Ok(loaded) => {
                    let saved = loaded.request;
                    let draft = draft.filter(|draft| *draft != saved);
                    (saved, draft)
                }
                // The file is gone but its changes are not: saving writes
                // it again.
                Err(_) => match draft {
                    Some(draft) => (draft.clone(), Some(draft)),
                    None => continue,
                },
            };
            tabs.push(Tab {
                path,
                saved,
                draft,
                response: ResponseState::Idle,
                revision: self.next_revision(),
                context: Arc::default(),
                conflict: false,
            });
        }
        let active = (session.active)
            .filter(|active| tabs.iter().any(|tab| tab.path == *active))
            .or_else(|| tabs.first().map(|tab| tab.path.clone()));
        let environment = (session.environment).filter(|name| environments.contains(name));

        let opened = self.next_revision();
        let (responses, history) = (Arc::new(responses), Arc::new(history));
        self.load_history(opened, responses.clone(), history.clone());
        let messages = self.messages.clone();
        let watcher = FsWatcher::start(&root, move |changes| {
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::External(opened, changes));
            }
        });
        let (watcher, error) = match watcher {
            Ok(watcher) => (Some(watcher), error),
            Err(e) => (None, error.or_else(|| Some(e.to_string()))),
        };
        self.open = Some(Open {
            repo,
            transport: Arc::new(transport),
            responses,
            history,
            sessions,
            opened,
            vault,
            _watcher: watcher,
        });
        self.snapshot = Snapshot {
            version: self.snapshot.version,
            collection: Some(CollectionView { root, tree }),
            environments,
            environment_files: Arc::new(environment_files),
            environment,
            tabs,
            active,
            error,
            history: Arc::default(),
        };
        self.refresh_variables(None);
        Ok(())
    }

    fn open_request(&mut self, path: RequestPath) {
        if self.tab(&path).is_some() {
            self.snapshot.active = Some(path);
            return;
        }
        let Some(open) = &self.open else { return };
        match load_request(&*open.repo, &path) {
            Ok(loaded) => {
                let tab = Tab {
                    path: path.clone(),
                    saved: loaded.request,
                    draft: None,
                    response: ResponseState::Idle,
                    revision: self.next_revision(),
                    context: Arc::default(),
                    conflict: false,
                };
                // New tabs go right after the active one.
                let at = (self.snapshot.active.as_ref())
                    .and_then(|active| self.snapshot.tabs.iter().position(|t| t.path == *active))
                    .map_or(self.snapshot.tabs.len(), |i| i + 1);
                self.snapshot.tabs.insert(at, tab);
                self.refresh_variables(Some(&path));
                self.snapshot.active = Some(path);
                self.snapshot.error = None;
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    fn close_tab(&mut self, path: &RequestPath) {
        let Some(index) = self.snapshot.tabs.iter().position(|tab| tab.path == *path) else {
            return;
        };
        if let Some((_, token)) = self.sending.remove(path) {
            token.cancel();
        }
        self.snapshot.tabs.remove(index);
        if self.snapshot.active.as_ref() == Some(path) {
            // The tab that took its place, or the one before it.
            let next = self
                .snapshot
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.snapshot.tabs.get(i)));
            self.snapshot.active = next.map(|tab| tab.path.clone());
        }
    }

    fn save(&mut self, path: &RequestPath) {
        let Some(open) = &self.open else { return };
        let Some(draft) = self.tab(path).and_then(|tab| tab.draft.clone()) else {
            return;
        };
        if let Err(e) = save_request(&*open.repo, path, &draft) {
            self.snapshot.error = Some(e.to_string());
            return;
        }
        let revision = self.next_revision();
        if let Some(tab) = self.tab_mut(path) {
            tab.saved = draft;
            tab.draft = None;
            tab.revision = revision;
            // Saving replaced the file: nothing is left to settle.
            tab.conflict = false;
        }
        self.snapshot.error = None;
        self.refresh_variables(Some(path));
        // The name or method shown in the tree may have changed.
        self.reload_tree();
    }

    /// Works out again the context of the tab at `only`, or of every tab.
    fn refresh_variables(&mut self, only: Option<&RequestPath>) {
        for index in 0..self.snapshot.tabs.len() {
            let tab = &self.snapshot.tabs[index];
            if only.is_some_and(|path| *path != tab.path) {
                continue;
            }
            let mut context = self.context(&tab.path, tab.request());
            context.revision = self.next_revision();
            self.snapshot.tabs[index].context = Arc::new(context);
        }
    }

    /// What `request` sees at `path` from outside itself.
    fn context(&self, path: &RequestPath, request: &HttpRequest) -> TabContext {
        let Some(open) = &self.open else {
            return TabContext::default();
        };
        // A request whose file is gone has no folders to read: it shows
        // nothing inherited until it is saved again.
        let (Ok(mut loaded), Ok(collection)) = (
            load_request(&*open.repo, path),
            CollectionRepository::collection(&*open.repo),
        ) else {
            return TabContext::default();
        };
        loaded.request = request.clone();
        let variables = resolve_request_variables(
            &*open.repo,
            &*open.repo,
            &loaded,
            self.snapshot.environment.as_deref(),
            &[],
        )
        .unwrap_or_default();
        // The request with nothing of its own shows what it inherits.
        let mut bare = request.clone();
        bare.headers.clear();
        bare.auth = Setting::Inherit;
        bare.settings = Settings::default();
        let inherited = resolve(Chain::new(&collection, &loaded.folders, &bare));
        TabContext {
            variables,
            folders: loaded.folders.iter().map(|f| f.name.clone()).collect(),
            inherited_headers: inherited.headers().to_vec(),
            inherited_auth: inherited.auth().clone(),
            inherited_settings: inherited.settings().clone(),
            revision: 0,
        }
    }

    /// Reconciles files changed outside apifast with what is open. A file
    /// that reads as what the tab already holds was written by apifast
    /// itself, and changes nothing.
    fn apply_external(&mut self, changes: &Changes) {
        let Some(open) = &self.open else { return };
        let repo = open.repo.clone();
        // A changed path is a file, or a folder renamed or removed whole.
        let touched = |path: &str| {
            changes.rescan
                || (changes.paths.iter())
                    .any(|changed| path == changed || path.starts_with(&format!("{changed}/")))
        };
        let shared = changes.rescan
            || changes.paths.iter().any(|path| {
                path == "opencollection.yml"
                    || path == ".env"
                    || path == "environments"
                    || path.starts_with("environments/")
                    || path.ends_with("folder.yml")
            });
        self.reload_tree();
        if shared {
            self.reload_environments();
        }
        let tabs: Vec<RequestPath> = (self.snapshot.tabs.iter())
            .map(|tab| tab.path.clone())
            .filter(|path| touched(path.as_str()))
            .collect();
        for path in tabs {
            match load_request(&*repo, &path) {
                Ok(loaded) => self.take_external(&path, loaded.request),
                Err(AppError::Repository(RepositoryError::NotFound(_))) => {
                    // Gone. Changes not saved stay: saving writes the file
                    // again. Without them, there is nothing left to show.
                    if self.tab(&path).is_some_and(|tab| tab.draft.is_none()) {
                        self.close_tab(&path);
                    }
                }
                // Half written, or not valid yet: the next change will tell.
                Err(_) => {}
            }
        }
        if shared {
            self.refresh_variables(None);
        }
    }

    /// The file of tab `path` now holds `request`.
    fn take_external(&mut self, path: &RequestPath, request: HttpRequest) {
        let revision = self.next_revision();
        let Some(tab) = self.tab_mut(path) else {
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
        self.refresh_variables(Some(path));
    }

    fn reload_tree(&mut self) {
        let Some(open) = &self.open else { return };
        match open_collection(&*open.repo) {
            Ok(tree) => {
                if let Some(collection) = &mut self.snapshot.collection {
                    collection.tree = tree;
                }
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    /// Runs a use case that changes the collection's files, then reloads the
    /// tree. Returns the use case's result, or `None` after reporting its
    /// error.
    fn organize<T>(
        &mut self,
        change: impl FnOnce(&FsCollection) -> Result<T, AppError>,
    ) -> Option<T> {
        let result = change(&self.open.as_ref()?.repo);
        // Reload even after an error: part of the change may have happened.
        self.reload_tree();
        match result {
            Ok(value) => {
                self.snapshot.error = None;
                Some(value)
            }
            Err(e) => {
                self.snapshot.error = Some(e.to_string());
                None
            }
        }
    }

    /// After `old` moved to `new`, the tabs of `old` and of everything
    /// inside it follow, with their drafts and sends in progress. Their
    /// saved contents are read again: the name may have changed.
    fn follow_move(&mut self, old: &ItemPath, new: &ItemPath) {
        let Some(open) = &self.open else { return };
        let repo = open.repo.clone();
        let moved = |path: &RequestPath| {
            ItemPath::Request(path.clone()).is_within(old).then(|| {
                RequestPath::new(format!(
                    "{}{}",
                    new.as_str(),
                    &path.as_str()[old.as_str().len()..]
                ))
            })
        };
        let mut revisions = Vec::new();
        for index in 0..self.snapshot.tabs.len() {
            let Some(to) = moved(&self.snapshot.tabs[index].path) else {
                continue;
            };
            revisions.push((index, to));
        }
        for (index, to) in revisions {
            let revision = self.next_revision();
            let from = self.snapshot.tabs[index].path.clone();
            if let Some(send) = self.sending.remove(&from) {
                self.sending.insert(to.clone(), send);
            }
            if self.snapshot.active.as_ref() == Some(&from) {
                self.snapshot.active = Some(to.clone());
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
                Err(e) => self.snapshot.error = Some(e.to_string()),
            }
            tab.path = to.clone();
            tab.revision = revision;
            self.refresh_variables(Some(&to));
        }
    }

    fn start_send(&mut self) {
        let Some(path) = self.snapshot.active.clone() else {
            return;
        };
        if self.sending.contains_key(&path) {
            return;
        }
        let Some(open) = &self.open else { return };
        let (repo, transport, responses, history) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
            open.history.clone(),
        );
        let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
            Some(vault) => vault.clone(),
            None => Arc::new(NoSecrets),
        };
        let Some(request) = self.tab(&path).map(Tab::request) else {
            return;
        };
        let written = (request.method.as_str().to_owned(), request.url.clone());
        let options = SendOptions {
            environment: self.snapshot.environment.clone(),
            draft: self.tab(&path).and_then(|tab| tab.draft.clone()),
            ..SendOptions::default()
        };
        let Some(tab) = self.tab_mut(&path) else {
            return;
        };
        tab.response = ResponseState::Sending;
        let cancel = CancelToken::new();
        let id = self.next_revision();
        self.sending.insert(path.clone(), (id, cancel.clone()));

        let messages = self.messages.clone();
        tokio::spawn(async move {
            let (clock, random) = (SystemClock, SystemRandom::default());
            let ports = SendPorts {
                collections: &*repo,
                environments: &*repo,
                transport: &*transport,
                responses: &*responses,
                clock: &clock,
                random: &random,
                prompter: &NoPrompter,
                secrets: &*secrets,
                report: &|_| {},
            };
            let at_ms = clock.unix_millis();
            let result = send_request(ports, &path, &options, &cancel).await;
            let entry = match &result {
                Ok(outcome) => Some(HistoryEntry::received(
                    at_ms,
                    path.clone(),
                    written,
                    outcome.head.clone(),
                    (outcome.body.size, outcome.body.id.clone()),
                    outcome.contains_secret,
                )),
                Err(AppError::Transport(TransportError::Cancelled)) => None,
                Err(e) => Some(HistoryEntry::failed(
                    at_ms,
                    path.clone(),
                    written,
                    e.to_string(),
                )),
            };
            // A history that cannot be written does not hide the response.
            let entry = entry.filter(|entry| history.append(entry).is_ok());
            let result = result.map(|outcome| {
                received(&responses, outcome.head, outcome.body.size, outcome.body.id)
            });
            // Gone only when the core is stopping, and then nobody waits.
            if let Some(messages) = messages.upgrade() {
                let finished = Finished { id, result, entry };
                let _ = messages.send(Message::SendFinished(Box::new(finished)));
            }
        });
    }

    /// Records the result of send number `id` in its tab, wherever the tab
    /// is now. A tab closed meanwhile has no entry, and the result is
    /// dropped.
    fn finish_send(
        &mut self,
        id: u64,
        result: Result<Received, AppError>,
        entry: Option<HistoryEntry>,
    ) {
        if let Some(entry) = entry {
            Arc::make_mut(&mut self.snapshot.history).push(entry);
        }
        let Some(path) = (self.sending.iter())
            .find(|(_, (send, _))| *send == id)
            .map(|(path, _)| path.clone())
        else {
            return;
        };
        self.sending.remove(&path);
        let response = match result {
            Ok(received) => ResponseState::Received(received),
            Err(AppError::Transport(TransportError::Cancelled)) => ResponseState::Cancelled,
            Err(e) => ResponseState::Failed(e.to_string()),
        };
        if let Some(tab) = self.tab_mut(&path) {
            tab.response = response;
        }
    }

    /// Prunes the history and the responses of the collection just opened,
    /// then reads its history, away from the loop.
    fn load_history(
        &self,
        opened: u64,
        responses: Arc<FsResponseStore>,
        history: Arc<FsHistoryStore>,
    ) {
        let messages = self.messages.clone();
        let (history_limits, response_limits) = self.limits;
        tokio::spawn(async move {
            let entries = tokio::task::spawn_blocking(move || {
                let now = SystemTime::now();
                let now_ms = SystemClock.unix_millis();
                // Limits that cannot be applied now are applied next time.
                let _ = history.prune(history_limits, now_ms);
                let _ = responses.prune(response_limits, now);
                history.entries().unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::HistoryLoaded(opened, entries));
            }
        });
    }

    /// Takes the history read for collection number `opened`, and shows in
    /// each tab still empty its last response that is still kept.
    fn history_loaded(&mut self, opened: u64, entries: Vec<HistoryEntry>) {
        let Some(open) = self.open.as_ref().filter(|open| open.opened == opened) else {
            return;
        };
        let responses = open.responses.clone();
        for tab in &mut self.snapshot.tabs {
            if !matches!(tab.response, ResponseState::Idle) {
                continue;
            }
            let last = entries.iter().rev().find(|entry| entry.request == tab.path);
            if let Some(response) = last.and_then(|entry| response_of(&responses, entry, true)) {
                tab.response = response;
            }
        }
        self.snapshot.history = Arc::new(entries);
    }

    /// Shows history entry `index` in its request's tab.
    fn open_history(&mut self, index: usize) {
        let Some(entry) = self.snapshot.history.get(index).cloned() else {
            return;
        };
        let Some(open) = &self.open else { return };
        let responses = open.responses.clone();
        self.open_request(entry.request.clone());
        let sending = self.sending.contains_key(&entry.request);
        match self.tab_mut(&entry.request) {
            Some(tab) if !sending => {
                if let Some(response) = response_of(&responses, &entry, false) {
                    tab.response = response;
                }
            }
            _ => {}
        }
    }

    fn schedule_session_write(&mut self) {
        if self.open.is_none() || self.session_pending {
            return;
        }
        self.session_pending = true;
        let messages = self.messages.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SESSION_DELAY).await;
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::WriteSession);
            }
        });
    }

    fn write_session(&mut self) {
        let Some(open) = &self.open else { return };
        let session = Session {
            tabs: (self.snapshot.tabs.iter())
                .map(|tab| SessionTab {
                    path: tab.path.clone(),
                    draft: tab.draft.clone(),
                })
                .collect(),
            active: self.snapshot.active.clone(),
            environment: self.snapshot.environment.clone(),
        };
        if let Err(e) = open.sessions.save(&session) {
            self.snapshot.error = Some(e.to_string());
        }
    }
}
