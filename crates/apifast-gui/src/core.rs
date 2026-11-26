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

mod captures;
mod collections;
mod cookies;
mod copy;
mod export;
mod git;
mod import;
mod methods;
mod proxy;
mod reimport;
mod runner;
mod schema;
mod socket;
mod stream;
mod tokens;
mod vault;
mod workspace;

use crate::messages::Say as _;
use collections::{Listed, Open};
use workspace::ActiveWorkspace;

pub use cookies::{CookieCommand, CookieSlot};
pub use copy::Copied;
pub use export::Exported;
pub use git::{GitCommand, GitNotice, GitProblem, GitView};
pub use import::ImportView;
pub use methods::MethodsView;
pub use reimport::ReimportView;
pub use schema::SchemaView;
pub use stream::{Live, LiveStream, MAX_LIVE_EVENTS};
pub use vault::{Revealed, SHOWN_FOR, SecretState, SecretStatus, VaultCommand, VaultView};

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use apifast_app::{
    AppError, AssertionOutcome, CancelToken, CaptureOutcome, Clock, CollectionRepository,
    CollectionTree, HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, ItemPath,
    NoPrompter, NoSecrets, Placement, RequestPath, ResponseHead, ResponseId, ResponseStore,
    SecretStore, SendOptions, SendPorts, SentRequest, Session, SessionStore, SessionTab,
    SharedSecretFiles, TokenStore, TransportError, apply_captures, apply_script_variables,
    create_folder, create_request, delete_item, keep_script_changes, load_request, move_item,
    rename_in, rename_item, resolve_request_variables, save_request, send_request,
};
use apifast_domain::{
    AuthScheme, Chain, Collection, Environment, Header, HttpRequest, Origin, Resolved,
    ResolvedSettings, ResolvedVariables, Setting, Settings, resolve,
};
use apifast_history::{FsHistoryStore, FsResponseStore, Limits};
use apifast_storage_fs::FsSessionStore;
use apifast_vault::{SystemKeyring, VaultSecrets};
use apifast_watch::Changes;
use tokio::sync::mpsc;

use crate::body::{BodyHandle, BodySource};
use crate::system::{SystemClock, SystemRandom};
pub use runner::{RunSettings, RunView};
use tokens::token_state;

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
    (shared, keyring): (Arc<dyn SharedSecretFiles>, SystemKeyring),
) -> Result<Stores, String> {
    let responses =
        FsResponseStore::new(apifast_history::dirs::responses_dir_in(data_dir, root, id));
    let sessions = FsSessionStore::new(apifast_history::dirs::sessions_dir_in(data_dir, root, id));
    let history = FsHistoryStore::new(apifast_history::dirs::history_file_in(data_dir, root, id));
    // Secrets need the collection's id: without one, none was ever stored.
    let secrets = id
        .map(|id| VaultSecrets::for_collection(data_dir, id, shared, keyring).map(Arc::new))
        .transpose()
        .map_err(|e| e.to_string())?;
    // OAuth 2.0 tokens, sealed with the master key, for any collection.
    let tokens = apifast_vault::FsTokenStore::new(
        apifast_history::dirs::tokens_file_in(data_dir, root, id),
        data_dir,
        keyring,
    );
    // The cookie jar, sealed the same way.
    let cookies = apifast_vault::FsCookieStore::new(
        apifast_history::dirs::cookies_file_in(data_dir, root, id),
        data_dir,
        keyring,
    );
    let schemas = apifast_history::FsSchemaCache::new(apifast_history::dirs::schemas_dir_in(
        data_dir, root, id,
    ));
    Ok(Stores {
        responses,
        sessions,
        history,
        vault: secrets,
        tokens: Arc::new(tokens),
        cookies: Arc::new(cookies),
        schemas,
    })
}

/// What [`stores`] makes.
struct Stores {
    responses: FsResponseStore,
    sessions: FsSessionStore,
    history: FsHistoryStore,
    vault: Option<Arc<VaultSecrets>>,
    tokens: Arc<dyn TokenStore>,
    cookies: Arc<dyn apifast_app::CookieStore>,
    schemas: apifast_history::FsSchemaCache,
}

/// A response whose body is in `store`.
fn received(
    store: &Arc<FsResponseStore>,
    head: ResponseHead,
    (size, id): (u64, ResponseId),
    sent: Option<SentRequest>,
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
        checks: Checks::default(),
        sent: sent.map(Arc::new),
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
                (*size, body.clone()),
                entry.sent.clone(),
            )))
        }
        HistoryOutcome::Failed(message) => Some(ResponseState::Failed(message.clone())),
    }
}

/// What the history keeps of a send at `at_ms` of the request at `path`,
/// `written` as it was: nothing when it was cancelled.
fn history_entry(
    result: &Result<apifast_app::SendOutcome, AppError>,
    at_ms: i64,
    path: RequestPath,
    written: (String, String),
) -> Option<HistoryEntry> {
    match result {
        Ok(outcome) => Some(
            HistoryEntry::received(
                at_ms,
                path,
                written,
                outcome.head.clone(),
                (outcome.body.size, outcome.body.id.clone()),
                &outcome.secrets,
            )
            .with_sent(Some(outcome.sent.clone())),
        ),
        Err(AppError::Transport(TransportError::Cancelled)) => None,
        Err(e) => Some(HistoryEntry::failed(at_ms, path, written, e.to_string())),
    }
}

/// How send number `id` of the tab `key` ended, `written` as it was and
/// sent at `.1` (ms): its result, kept in `responses`, and its entry,
/// appended to `history`.
fn finished(
    id: u64,
    key: TabKey,
    (written, at_ms): ((String, String), i64),
    result: Result<apifast_app::SendOutcome, AppError>,
    (responses, history): (&Arc<FsResponseStore>, &FsHistoryStore),
) -> Finished {
    let entry = history_entry(&result, at_ms, key.path.clone(), written);
    // A history that cannot be written does not hide the response.
    let entry = entry.filter(|entry| history.append(entry).is_ok());
    let dependencies = result
        .as_ref()
        .map(|outcome| outcome.dependencies.clone())
        .unwrap_or_default();
    let result = result.map(|outcome| Received {
        checks: Checks {
            captures: outcome.captures,
            assertions: outcome.assertions,
            scripts: Box::new(outcome.scripts),
        },
        ..received(
            responses,
            outcome.head,
            (outcome.body.size, outcome.body.id),
            Some(outcome.sent),
        )
    });
    Finished {
        id,
        root: key.root,
        result,
        entry,
        dependencies,
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

/// The folder of the default workspace, in the data directory.
const DEFAULT_WORKSPACE: &str = "default-workspace";

/// The file that makes a folder a collection.
const COLLECTION_FILE: &str = "opencollection.yml";

/// A tab: the collection it belongs to and its request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabKey {
    /// The collection's folder.
    pub root: PathBuf,
    /// The request, relative to it.
    pub path: RequestPath,
}

impl TabKey {
    /// The tab of `path` in the collection at `root`.
    pub fn new(root: impl Into<PathBuf>, path: RequestPath) -> Self {
        Self {
            root: root.into(),
            path,
        }
    }
}

/// A change to the focused collection's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryCommand {
    /// Names entry `id`; an empty name takes its name away. A named entry
    /// is kept whatever the limits.
    Rename {
        /// The entry's [`HistoryEntry::id`].
        id: String,
        /// Its new name.
        name: String,
    },
    /// Removes entry `id`, with its body.
    Remove(String),
    /// Removes every entry, with their bodies.
    Clear,
}

/// What the interface asks the core to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Opens the active workspace as it was left: its collections, the ones
    /// expanded and the tabs. Done once; the first command that needs the
    /// workspace does it too.
    Restore,
    /// Opens the collection in this folder, adding it to the workspace, and
    /// shows it.
    OpenCollection(PathBuf),
    /// Creates a collection named `name` in `folder` (created if needed),
    /// then opens it. Refused if the folder already holds one.
    CreateCollection {
        /// Its folder.
        folder: PathBuf,
        /// Its name.
        name: String,
    },
    /// Makes this collection the one the environment selector, the
    /// environment editor, the vault and the history are about.
    FocusCollection(PathBuf),
    /// Expands or collapses a collection in the tree; expanding loads it.
    ExpandCollection {
        /// The collection's folder.
        root: PathBuf,
        /// Whether it is expanded.
        expanded: bool,
    },
    /// Unloads a collection and closes its tabs. Unsaved changes stay in
    /// its session, and come back when the request is opened again.
    CloseCollection(PathBuf),
    /// Closes a collection and removes it from the workspace. Nothing is
    /// deleted.
    RemoveCollection(PathBuf),
    /// Creates a workspace named `name` in `folder` and switches to it.
    CreateWorkspace {
        /// Its folder.
        folder: PathBuf,
        /// Its name.
        name: String,
    },
    /// Switches to the workspace in this folder, remembering it.
    OpenWorkspace(PathBuf),
    /// Renames the active workspace.
    RenameWorkspace(String),
    /// Forgets a workspace on this machine. Nothing is deleted. The active
    /// one cannot be forgotten.
    ForgetWorkspace(PathBuf),
    /// Picks the active environment of the focused collection, or none.
    SelectEnvironment(Option<String>),
    /// Opens a request in a tab, or shows its tab if it is open.
    OpenRequest(TabKey),
    /// Shows an open tab.
    ActivateTab(TabKey),
    /// Closes a tab, discarding its unsaved changes.
    CloseTab(TabKey),
    /// Replaces the contents of an open tab with the user's edits. They
    /// stay a draft until saved.
    Edit {
        /// The tab.
        tab: TabKey,
        /// Its new contents. Boxed: an edit is much larger than any other
        /// command.
        request: Box<HttpRequest>,
    },
    /// Saves the draft of a tab to its file.
    Save(TabKey),
    /// Saves every draft.
    SaveAll,
    /// Sends the active tab, as it is on screen (with its draft).
    Send,
    /// Cancels the send of the active tab; a WebSocket connection closes.
    Cancel,
    /// Sends message number `.0` of the active tab's WebSocket request, as
    /// it is on screen, over its open connection.
    SendMessage(usize),
    /// Creates a request called `name` in `folder` (`""` for the root) of
    /// the collection at `root`, and opens it.
    CreateRequest {
        /// The collection's folder.
        root: PathBuf,
        /// Where.
        folder: String,
        /// Its name.
        name: String,
    },
    /// Creates a folder called `name` in `parent` (`""` for the root).
    CreateFolder {
        /// The collection's folder.
        root: PathBuf,
        /// Where.
        parent: String,
        /// Its name.
        name: String,
    },
    /// Renames a request or folder.
    Rename {
        /// The collection's folder.
        root: PathBuf,
        /// What.
        item: ItemPath,
        /// The new name.
        name: String,
    },
    /// Deletes a request or folder, with everything inside it.
    Delete {
        /// The collection's folder.
        root: PathBuf,
        /// What.
        item: ItemPath,
    },
    /// Moves a request or folder, inside its collection.
    Move {
        /// The collection's folder.
        root: PathBuf,
        /// What.
        item: ItemPath,
        /// Where to.
        placement: Placement,
    },
    /// Shows entry `index` of [`Snapshot::history`] in its request's tab.
    OpenHistory(usize),
    /// A change to the focused collection's history.
    History(HistoryCommand),
    /// How much history and how many response bodies to keep, from the
    /// settings. Applied when a collection opens.
    Configure {
        /// Of the history.
        history: HistoryLimits,
        /// Of the response bodies.
        responses: Limits,
        /// The application's proxy, without its password.
        proxy: apifast_domain::ProxyChoice,
        /// How servers are trusted.
        trust: apifast_domain::TlsTrust,
        /// Whether cookies from the jars are sent, and whether those that
        /// responses set are stored.
        cookies: (bool, bool),
        /// The timeout of requests whose levels set none, in milliseconds
        /// (0: none).
        timeout_ms: u64,
        /// Whether the workspace used last opens at the start, instead of
        /// the default one.
        reopen_last: bool,
    },
    /// The interface's language, by its code: what the core says (errors,
    /// notices) is said in it. English until told.
    Language(&'static str),
    /// Keeps the application proxy's password, sealed; `None` forgets it.
    SetProxyPassword(Option<apifast_domain::Secret>),
    /// Creates an empty environment in the focused collection.
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
    /// Writes the values captured for the active environment of the
    /// collection at `root` (`save`), or forgets them.
    SaveCaptures {
        /// The collection.
        root: PathBuf,
        /// Write them, or forget them.
        save: bool,
    },
    /// Runs the requests of the collection at `root` (docs/08), in the
    /// background. Ignored while a run is going.
    Run {
        /// The collection.
        root: PathBuf,
        /// What to run and how.
        settings: RunSettings,
    },
    /// Gets a new OAuth 2.0 token for the request of the tab, in the
    /// background.
    GetToken(TabKey),
    /// Forgets the OAuth 2.0 token stored for the request of the tab.
    ForgetToken(TabKey),
    /// Saves the scripts of a folder, or of the collection at `root`.
    SaveScripts {
        /// The collection.
        root: PathBuf,
        /// The folder, relative to it; `None` for the collection.
        folder: Option<String>,
        /// Its scripts.
        scripts: Vec<apifast_domain::ScriptBlock>,
    },
    /// Saves the proxy, the client certificates and the DNS overrides of
    /// the collection at `root` into its files, and sends with them from
    /// then on.
    SaveCollectionSettings {
        /// The collection.
        root: PathBuf,
        /// Its proxy.
        proxy: apifast_domain::CollectionProxy,
        /// Its client certificates.
        client_certificates: Vec<apifast_domain::ClientCertificate>,
        /// Its DNS overrides.
        dns_overrides: Vec<apifast_domain::DnsOverride>,
    },
    /// Stops the run that is going.
    StopRun,
    /// Forgets a run that ended.
    CloseRun,
    /// Opens the request of a step of the run, with its response.
    OpenRunStep(usize),
    /// Settles a tab whose file changed outside apifast while it had a
    /// draft: keep the draft (saving then replaces the file) or take the
    /// file and drop the draft.
    ResolveConflict {
        /// The tab.
        tab: TabKey,
        /// Whether the draft is kept.
        keep_draft: bool,
    },
    /// Something of the focused collection's vault: secrets and its key.
    Vault(VaultCommand),
    /// A change to the focused collection's cookie jar.
    Cookies(CookieCommand),
    /// Something asked of the focused collection's git repository.
    Git(GitCommand),
    /// Renders the request as cURL, for the clipboard (see
    /// [`Snapshot::copied`]).
    CopyAs(TabKey, apifast_import::CodeLanguage),
    /// Reads a Postman collection and its environments and makes the plan
    /// of importing them (see [`Snapshot::import`]).
    PlanImport {
        /// The collection's export.
        collection: PathBuf,
        /// Its environments' exports.
        environments: Vec<PathBuf>,
    },
    /// Writes the reviewed import into this new folder, and opens it.
    ApplyImport(PathBuf),
    /// Forgets the import being reviewed.
    CloseImport,
    /// Starts importing again into the collection at this folder (see
    /// [`Snapshot::reimport`]).
    OpenReimport(PathBuf),
    /// Reads this file and plans importing it again.
    PlanReimport(PathBuf),
    /// Applies the reviewed reimport, with the requests of these keys
    /// chosen.
    ApplyReimport(Vec<String>),
    /// Forgets the reimport being reviewed.
    CloseReimport,
    /// Asks the server of the tab's GraphQL request for its schema (see
    /// [`TabContext::schema`]).
    FetchSchema(TabKey),
    /// Loads the methods of the tab's gRPC request (see
    /// [`TabContext::methods`]).
    LoadMethods(TabKey),
    /// Ends the client's stream of the active tab's gRPC call: no more
    /// messages go.
    EndStream,
    /// Writes the collection at `root` to `file` as Postman, with its
    /// environments next to it (see [`Snapshot::exported`]).
    ExportPostman {
        /// The collection's folder.
        root: PathBuf,
        /// The collection's file.
        file: PathBuf,
    },
}

impl Command {
    /// Whether it works in a workspace: all but the settings and the
    /// language, which come before one is restored.
    fn needs_workspace(&self) -> bool {
        !matches!(self, Self::Configure { .. } | Self::Language(_))
    }
}

/// Everything the interface shows, as of one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Increases with every change.
    pub version: u64,
    /// The active workspace, once restored.
    pub workspace: Option<WorkspaceView>,
    /// The workspaces this machine knows, the active one among them.
    pub workspaces: Vec<WorkspaceView>,
    /// The collections of the active workspace, in its order.
    pub collections: Vec<CollectionEntry>,
    /// The focused collection: the one of the active tab, or the last one
    /// picked. The fields below up to [`Self::environment`], and
    /// [`Self::history`] and [`Self::vault`], are about it.
    pub collection: Option<CollectionView>,
    /// Names of its environments.
    pub environments: Vec<String>,
    /// Its environments, as in their files, in the order of
    /// [`Self::environments`].
    pub environment_files: Arc<Vec<Environment>>,
    /// Its active environment.
    pub environment: Option<String>,
    /// Open tabs, of any collection, in order.
    pub tabs: Vec<Tab>,
    /// The active tab.
    pub active: Option<TabKey>,
    /// The last error that is not about a send, such as a collection that
    /// cannot be opened.
    pub error: Option<String>,
    /// A folder the user tried to open as a collection, which has none: the
    /// interface offers to create one there.
    pub missing_collection: Option<PathBuf>,
    /// Where a new collection goes unless the user picks another folder:
    /// the active workspace's folder.
    pub default_parent: PathBuf,
    /// The sends of the focused collection, oldest first. Loaded shortly
    /// after it opens.
    pub history: Arc<Vec<HistoryEntry>>,
    /// What the interface sees of the focused collection's vault.
    pub vault: VaultView,
    /// The cookies of the focused collection's jar, in its order.
    pub cookies: Arc<Vec<apifast_domain::Cookie>>,
    /// The focused collection's git repository, if it is in one.
    pub git: Option<GitView>,
    /// The last text for the clipboard, such as a request as cURL.
    pub copied: Option<Copied>,
    /// The import being reviewed, if any.
    pub import: Option<ImportView>,
    /// The reimport being reviewed, if any.
    pub reimport: Option<ReimportView>,
    /// What the last export did.
    pub exported: Option<Exported>,
    /// Values captured for its active environment, waiting for the user to
    /// write them there.
    pub captured: Option<PendingCaptures>,
    /// The last run of the collection runner, of any collection.
    pub run: Option<RunView>,
    /// The tab whose OAuth 2.0 token is being obtained.
    pub getting_token: Option<TabKey>,
    /// Whether a password is kept for the application's proxy.
    pub proxy_password_saved: bool,
}

impl Snapshot {
    /// The active tab.
    pub fn active_tab(&self) -> Option<&Tab> {
        let active = self.active.as_ref()?;
        self.tabs.iter().find(|tab| tab.key() == *active)
    }
}

/// Values captured for an environment, waiting to be written there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingCaptures {
    /// The environment.
    pub environment: String,
    /// The variables, in capture order. Their values stay in the core.
    pub names: Vec<String>,
}

/// A workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
    /// Its folder, which holds `workspace.yml`.
    pub folder: PathBuf,
    /// Its name.
    pub name: String,
}

/// A collection of the workspace, loaded or not.
#[derive(Debug, Clone)]
pub struct CollectionEntry {
    /// Its folder.
    pub root: PathBuf,
    /// Its name: the collection's own once loaded, the workspace's before.
    pub name: String,
    /// Whether it is expanded in the tree.
    pub expanded: bool,
    /// Its folders and requests, once loaded.
    pub tree: Option<Arc<CollectionTree>>,
    /// Why it cannot be loaded, if it cannot.
    pub error: Option<String>,
    /// It shares secrets and this machine lacks its key.
    pub needs_key: bool,
}

impl CollectionEntry {
    /// A loaded and expanded collection at `root`, named after its tree.
    pub fn loaded(root: impl Into<PathBuf>, tree: CollectionTree) -> Self {
        Self {
            root: root.into(),
            name: tree.collection.name.clone(),
            expanded: true,
            tree: Some(Arc::new(tree)),
            error: None,
            needs_key: false,
        }
    }
}

/// A loaded collection.
#[derive(Debug, Clone)]
pub struct CollectionView {
    /// Its folder.
    pub root: PathBuf,
    /// Its folders and requests.
    pub tree: Arc<CollectionTree>,
}

/// An open request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// The folder of its collection.
    pub root: PathBuf,
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
    /// The OAuth 2.0 token stored for the request, when it uses OAuth 2.0.
    pub token: TokenState,
    /// Its GraphQL schema, when its body is GraphQL.
    pub schema: Option<SchemaView>,
    /// The methods it can call, when it is a gRPC request.
    pub methods: Option<MethodsView>,
    /// Changes whenever it is worked out again.
    pub revision: u64,
}

/// What the interface knows of a request's OAuth 2.0 token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TokenState {
    /// The request does not use OAuth 2.0.
    #[default]
    NotUsed,
    /// It does, and no token is stored.
    Missing,
    /// A token is stored.
    Stored(apifast_app::TokenInfo),
    /// The stored tokens cannot be read.
    Unreadable(String),
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
            token: TokenState::NotUsed,
            schema: None,
            methods: None,
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

    /// Which tab it is.
    pub fn key(&self) -> TabKey {
        TabKey::new(self.root.clone(), self.path.clone())
    }

    /// Whether it is the tab `key`.
    pub fn is(&self, key: &TabKey) -> bool {
        self.root == key.root && self.path == key.path
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
    /// An event stream is arriving; it ends when the server ends it or
    /// the user stops it.
    Streaming(LiveStream),
    /// A response arrived.
    Received(Received),
    /// The send failed; the message is for the user.
    Failed(String),
    /// The user cancelled the send.
    Cancelled,
}

impl ResponseState {
    /// Whether a send is under way: waiting, or receiving a stream.
    pub fn is_sending(&self) -> bool {
        matches!(self, Self::Sending | Self::Streaming(_))
    }
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
    /// What ran after it: captures and assertions. Empty for a response
    /// from the history, which does not keep them.
    pub checks: Checks,
    /// The request as it went out, as the history keeps it; none for an
    /// entry from before apifast kept it.
    pub sent: Option<Arc<SentRequest>>,
}

/// The captures and assertions that ran after a response, masked, and
/// what the scripts did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checks {
    /// Captures, in order.
    pub captures: Vec<CaptureOutcome>,
    /// Assertions, in order.
    pub assertions: Vec<AssertionOutcome>,
    /// The scripts of the request, its folders and the collection; boxed,
    /// so a response state stays small.
    pub scripts: Box<apifast_app::ScriptReport>,
}

/// Messages the core thread handles: commands from the interface and
/// results of its own tasks.
enum Message {
    Command(Command),
    /// A send ended; the number identifies it in [`State::sending`]. The
    /// entry, already in the history file, is there unless it was cancelled.
    SendFinished(Box<Finished>),
    /// The history of collection opening number `.0` was pruned and read.
    HistoryLoaded(u64, Vec<HistoryEntry>),
    /// Files of collection opening number `.0` changed outside apifast.
    External(u64, Changes),
    /// The `workspace.yml` of workspace opening number `.0` changed.
    WorkspaceChanged(u64),
    /// Hide what was shown of the vault, if it is still show number `.0`.
    HideShown(u64),
    WriteSession,
    /// News of run number `.0`.
    Run(u64, runner::RunMessage),
    /// The token for the tab was obtained, or why not.
    TokenObtained(TabKey, Result<(), String>),
    /// Send number `.0` has news of its event stream.
    Streamed(u64),
    /// The methods of collection `.0`'s gRPC request, kept by `.1`, came,
    /// or why not.
    MethodsLoaded(PathBuf, String, Result<apifast_domain::GrpcCatalog, String>),
    /// The schema of collection `.0`, kept by `.1`, came, or why not.
    SchemaFetched(
        PathBuf,
        String,
        Result<apifast_domain::GraphQlSchema, String>,
    ),
}

/// A send or a connection in progress.
struct InFlight {
    /// Its number.
    id: u64,
    /// Stops it; a connection closes normally.
    cancel: CancelToken,
    /// What arrived of an event stream or a connection.
    stream: LiveStream,
    /// Where the messages to send over a connection go; `None` for a send.
    outbox: Option<mpsc::UnboundedSender<apifast_domain::WsMessage>>,
}

/// How a send ended: its number, its collection, its result and its
/// history entry.
struct Finished {
    id: u64,
    root: PathBuf,
    result: Result<Received, AppError>,
    entry: Option<HistoryEntry>,
    /// Entries for the requests its `response(...)` sent first.
    dependencies: Vec<HistoryEntry>,
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
        Self::start_with(data_dir, SystemKeyring::default(), notify)
    }

    /// As [`Core::start`], with the master key in `keyring`. Tests pass
    /// [`SystemKeyring::off`], so the user's keyring is never touched.
    ///
    /// # Panics
    ///
    /// If the thread or its runtime cannot be created (out of resources).
    pub fn start_with(
        data_dir: PathBuf,
        keyring: SystemKeyring,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let first = Snapshot {
            default_parent: data_dir.join(DEFAULT_WORKSPACE),
            ..Snapshot::default()
        };
        let snapshot = Arc::new(Mutex::new(Arc::new(first.clone())));
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
                let sealed = apifast_vault::SealedValues::new(
                    data_dir.join("settings.sealed"),
                    "settings",
                    &data_dir,
                    keyring,
                );
                let mut first = first;
                first.proxy_password_saved = matches!(sealed.get("proxy_password"), Ok(Some(_)));
                let state = State {
                    data_dir,
                    snapshot: first,
                    shared,
                    notify: Box::new(notify),
                    workspace: None,
                    listed: Vec::new(),
                    open: BTreeMap::new(),
                    focus: None,
                    sending: HashMap::new(),
                    run: None,
                    revision: 0,
                    session_pending: false,
                    messages: own_sender,
                    limits: (HISTORY_LIMITS, RESPONSE_LIMITS),
                    proxy: apifast_domain::ProxyChoice::System,
                    trust: apifast_domain::TlsTrust::default(),
                    cookie_policy: (true, true),
                    default_timeout_ms: apifast_domain::Settings::DEFAULT_TIMEOUT_MS,
                    reopen_last: true,
                    importing: None,
                    reimporting: None,
                    sealed,
                    shown: 0,
                    keyring,
                    strings: &crate::strings::EN,
                    logged_error: None,
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

struct State {
    data_dir: PathBuf,
    snapshot: Snapshot,
    shared: Arc<Mutex<Arc<Snapshot>>>,
    notify: Box<dyn Fn() + Send + Sync>,
    /// The active workspace, once restored.
    workspace: Option<ActiveWorkspace>,
    /// Its collections, in its order.
    listed: Vec<Listed>,
    /// The loaded collections, by folder.
    open: BTreeMap<PathBuf, Open>,
    /// The focused collection (see [`Snapshot::collection`]).
    focus: Option<PathBuf>,
    /// Sends and connections in progress, by tab. The entry moves with its
    /// tab.
    sending: HashMap<TabKey, InFlight>,
    /// The run that is going: its number and how to stop it.
    run: Option<(u64, CancelToken)>,
    /// Source of [`Tab::revision`], of send numbers and of openings.
    revision: u64,
    /// Whether the sessions changed and a write is scheduled.
    session_pending: bool,
    /// How much history and how many response bodies are kept; applied
    /// when a collection opens.
    limits: (HistoryLimits, Limits),
    /// The application's proxy, as the settings say; its password is kept
    /// apart, in `sealed`.
    proxy: apifast_domain::ProxyChoice,
    /// How servers are trusted, as the settings say.
    trust: apifast_domain::TlsTrust,
    /// Whether cookies are sent and stored, as the settings say.
    cookie_policy: (bool, bool),
    /// The timeout of requests whose levels set none, as the settings say.
    default_timeout_ms: u64,
    /// Whether the workspace used last opens at the start.
    reopen_last: bool,
    /// The collection read for the import being reviewed.
    importing: Option<apifast_app::ImportedCollection>,
    /// The source read for the reimport being reviewed.
    reimporting: reimport::Reimporting,
    /// Values of the settings kept sealed with the master key, such as the
    /// proxy's password.
    sealed: apifast_vault::SealedValues,
    /// Numbers what the vault shows, so hiding it later does not hide
    /// something shown since.
    shown: u64,
    /// The interface's language, for what the core says.
    strings: &'static crate::strings::Strings,
    /// The last error written to the log, so each is written once.
    logged_error: Option<String>,
    /// The keyring the vault keeps the master key in.
    keyring: SystemKeyring,
    /// To post the results of spawned tasks back to the loop. Weak: a strong
    /// sender here would keep the loop alive after the handle is dropped.
    messages: mpsc::WeakUnboundedSender<Message>,
}

impl State {
    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = receiver.recv().await {
            match message {
                Message::Command(command) => {
                    let touches_session = !matches!(
                        command,
                        Command::Send
                            | Command::Cancel
                            | Command::SendMessage(_)
                            | Command::EndStream
                            | Command::LoadMethods(_)
                    );
                    self.handle(command);
                    if touches_session {
                        self.schedule_session_write();
                    }
                }
                Message::SendFinished(finished) => {
                    self.finish_send(*finished);
                    self.sync_cookies();
                }
                Message::Run(id, message) => {
                    self.run_message(id, message);
                    self.sync_cookies();
                }
                Message::TokenObtained(key, result) => {
                    self.token_obtained(&key, result);
                    self.sync_cookies();
                }
                Message::MethodsLoaded(root, key, result) => {
                    self.methods_loaded(&root, key, result);
                    self.sync_cookies();
                }
                Message::SchemaFetched(root, key, result) => {
                    self.schema_fetched(&root, key, result);
                    self.sync_cookies();
                }
                Message::Streamed(id) => self.streamed(id),
                Message::HistoryLoaded(opened, entries) => self.history_loaded(opened, entries),
                Message::HideShown(shown) => self.hide_shown(shown),
                Message::External(opened, changes) => {
                    if let Some(root) = self.root_of(opened) {
                        self.apply_external(&root, &changes);
                    }
                }
                Message::WorkspaceChanged(opened) => self.workspace_changed(opened),
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
        self.mirror();
        // Each error the interface shows, once, in the log too.
        if self.snapshot.error != self.logged_error {
            if let Some(error) = &self.snapshot.error {
                tracing::warn!("{error}");
            }
            self.logged_error.clone_from(&self.snapshot.error);
        }
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
        // Every command but the settings works in a workspace.
        if self.workspace.is_none() && command.needs_workspace() {
            self.restore();
        }
        match command {
            Command::Restore => {}
            Command::Language(code) => self.strings = crate::strings::english_or(code).strings,
            Command::OpenCollection(root) => self.open_or_report(&root),
            Command::CreateCollection { folder, name } => self.create_collection(&folder, name),
            Command::FocusCollection(root) => self.focus_on(&root),
            Command::ExpandCollection { root, expanded } => self.expand(&root, expanded),
            Command::CloseCollection(root) => self.close_collection(&root),
            Command::RemoveCollection(root) => self.remove_collection(&root),
            Command::CreateWorkspace { .. }
            | Command::OpenWorkspace(_)
            | Command::RenameWorkspace(_)
            | Command::ForgetWorkspace(_) => self.handle_workspace(command),
            Command::SelectEnvironment(name) => {
                if let Some(open) = self.focused_mut() {
                    open.environment = name;
                }
                self.refresh_variables(None);
            }
            Command::OpenRequest(key) => self.open_request(&key),
            Command::ActivateTab(key) => {
                if self.tab(&key).is_some() {
                    self.focus = Some(key.root.clone());
                    self.snapshot.active = Some(key);
                }
            }
            Command::CloseTab(key) => self.close_tab(&key),
            Command::Edit { tab: key, request } => {
                let Some(tab) = self.tab_mut(&key) else {
                    return;
                };
                // Only the request's own variables change what it sees; the
                // URL or a header do not, so typing reads no files.
                let variables_changed = request.variables != tab.request().variables;
                tab.draft = (*request != tab.saved).then_some(*request);
                if variables_changed {
                    self.refresh_variables(Some(&key));
                }
            }
            Command::Save(key) => self.save(&key),
            Command::SaveAll => {
                let dirty: Vec<TabKey> = (self.snapshot.tabs.iter())
                    .filter(|tab| tab.is_dirty())
                    .map(Tab::key)
                    .collect();
                for key in dirty {
                    self.save(&key);
                }
            }
            Command::Send => self.start_send(),
            Command::SendMessage(index) => self.send_message(index),
            Command::Cancel => {
                if let Some(sending) =
                    (self.snapshot.active.as_ref()).and_then(|key| self.sending.get(key))
                {
                    sending.cancel.cancel();
                }
            }
            Command::CreateRequest { .. }
            | Command::CreateFolder { .. }
            | Command::Rename { .. }
            | Command::Delete { .. }
            | Command::Move { .. }
            | Command::SaveScripts { .. } => self.handle_tree(command),
            Command::OpenHistory(_) | Command::History(_) => self.handle_history(command),
            Command::Configure { .. }
            | Command::SetProxyPassword(_)
            | Command::SaveCollectionSettings { .. } => self.handle_settings(command),
            Command::Vault(command) => self.vault_command(command),
            Command::Cookies(command) => self.cookie_command(command),
            Command::Git(command) => self.git_command(command),
            Command::CopyAs(key, language) => self.copy_as(&key, language),
            Command::FetchSchema(key) => self.fetch_schema(key),
            Command::LoadMethods(key) => self.load_methods(key),
            Command::EndStream => self.end_stream(),
            Command::ExportPostman { root, file } => self.export_postman(&root, &file),
            Command::PlanImport { .. }
            | Command::ApplyImport(_)
            | Command::CloseImport
            | Command::OpenReimport(_)
            | Command::PlanReimport(_)
            | Command::ApplyReimport(_)
            | Command::CloseReimport => self.handle_import(command),
            Command::ResolveConflict {
                tab: key,
                keep_draft,
            } => self.resolve_conflict(&key, keep_draft),
            Command::CreateEnvironment(_)
            | Command::RenameEnvironment { .. }
            | Command::SaveEnvironment(_)
            | Command::DeleteEnvironment(_) => self.handle_environment(command),
            Command::SaveCaptures { root, save } => self.save_captures(&root, save),
            Command::Run { root, settings } => self.start_run(&root, settings),
            Command::StopRun => self.stop_run(),
            Command::GetToken(key) => self.get_token(key),
            Command::ForgetToken(key) => self.forget_token(&key),
            Command::CloseRun => self.close_run(),
            Command::OpenRunStep(index) => self.open_run_step(index),
        }
    }

    /// The file of the tab at `key` changed while it had a draft: keeps
    /// the draft, or takes the file.
    fn resolve_conflict(&mut self, key: &TabKey, keep_draft: bool) {
        let revision = self.next_revision();
        if let Some(tab) = self.tab_mut(key) {
            tab.conflict = false;
            if !keep_draft {
                tab.draft = None;
                tab.revision = revision;
            }
        }
        self.refresh_variables(Some(key));
    }

    /// Commands about settings: limits and proxies.
    fn handle_settings(&mut self, command: Command) {
        match command {
            Command::Configure {
                history,
                responses,
                proxy,
                trust,
                cookies: (send, store),
                timeout_ms,
                reopen_last,
            } => {
                self.limits = (history, responses);
                self.default_timeout_ms = timeout_ms;
                self.reopen_last = reopen_last;
                self.set_cookie_policy(send, store);
                if proxy != self.proxy || trust != self.trust {
                    self.proxy = proxy;
                    self.trust = trust;
                    self.rebuild_transports();
                }
            }
            Command::SetProxyPassword(password) => self.set_proxy_password(password.as_ref()),
            Command::SaveCollectionSettings {
                root,
                proxy,
                client_certificates,
                dns_overrides,
            } => {
                let settings = (proxy, client_certificates, dns_overrides);
                self.save_collection_settings(&root, settings);
            }
            _ => {}
        }
    }

    /// Commands that change a collection's folders and requests.
    fn handle_tree(&mut self, command: Command) {
        match command {
            Command::CreateRequest { root, folder, name } => {
                if let Some(path) =
                    self.organize(&root, |repo| create_request(repo, &folder, &name))
                {
                    self.open_request(&TabKey::new(root, path));
                }
            }
            Command::CreateFolder { root, parent, name } => {
                self.organize(&root, |repo| create_folder(repo, &parent, &name));
            }
            Command::SaveScripts {
                root,
                folder,
                scripts,
            } => self.save_scripts(&root, folder.as_deref(), scripts),
            Command::Rename { root, item, name } => {
                if let Some(new) = self.organize(&root, |repo| rename_item(repo, &item, &name)) {
                    self.follow_move(&root, &item, &new);
                }
            }
            Command::Delete { root, item } => {
                if self
                    .organize(&root, |repo| delete_item(repo, &item))
                    .is_some()
                {
                    let inside: Vec<TabKey> = (self.snapshot.tabs.iter())
                        .filter(|tab| {
                            tab.root == root && ItemPath::Request(tab.path.clone()).is_within(&item)
                        })
                        .map(Tab::key)
                        .collect();
                    for key in inside {
                        self.close_tab(&key);
                    }
                }
            }
            Command::Move {
                root,
                item,
                placement,
            } => {
                if let Some(new) = self.organize(&root, |repo| move_item(repo, &item, &placement)) {
                    self.follow_move(&root, &item, &new);
                }
            }
            _ => {}
        }
    }

    fn tab(&self, key: &TabKey) -> Option<&Tab> {
        self.snapshot.tabs.iter().find(|tab| tab.is(key))
    }

    fn tab_mut(&mut self, key: &TabKey) -> Option<&mut Tab> {
        self.snapshot.tabs.iter_mut().find(|tab| tab.is(key))
    }

    /// The tab at `key` with `saved` and `draft`, ready to insert.
    fn new_tab(&mut self, key: &TabKey, saved: HttpRequest, draft: Option<HttpRequest>) -> Tab {
        Tab {
            root: key.root.clone(),
            path: key.path.clone(),
            draft: draft.filter(|draft| *draft != saved),
            saved,
            response: ResponseState::Idle,
            revision: self.next_revision(),
            context: Arc::default(),
            conflict: false,
        }
    }

    fn open_request(&mut self, key: &TabKey) {
        if self.tab(key).is_some() {
            self.focus = Some(key.root.clone());
            self.snapshot.active = Some(key.clone());
            return;
        }
        if let Err(message) = self.load(&key.root) {
            self.snapshot.error = Some(message);
            return;
        }
        let Some(open) = self.open.get_mut(&key.root) else {
            return;
        };
        match load_request(&*open.repo, &key.path) {
            Ok(loaded) => {
                // Unsaved changes kept from when the tab was closed with
                // its collection come back.
                let draft = open.parked.remove(&key.path);
                let tab = self.new_tab(key, loaded.request, draft);
                // New tabs go right after the active one.
                let at = (self.snapshot.active.as_ref())
                    .and_then(|active| self.snapshot.tabs.iter().position(|t| t.is(active)))
                    .map_or(self.snapshot.tabs.len(), |i| i + 1);
                self.snapshot.tabs.insert(at, tab);
                self.refresh_variables(Some(key));
                self.focus = Some(key.root.clone());
                self.snapshot.active = Some(key.clone());
                self.snapshot.error = None;
            }
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
    }

    fn close_tab(&mut self, key: &TabKey) {
        let Some(index) = self.snapshot.tabs.iter().position(|tab| tab.is(key)) else {
            return;
        };
        if let Some(sending) = self.sending.remove(key) {
            sending.cancel.cancel();
        }
        self.snapshot.tabs.remove(index);
        if self.snapshot.active.as_ref() == Some(key) {
            // The tab that took its place, or the one before it.
            let next = self
                .snapshot
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.snapshot.tabs.get(i)));
            self.snapshot.active = next.map(Tab::key);
            if let Some(next) = &self.snapshot.active {
                self.focus = Some(next.root.clone());
            }
        }
    }

    fn save(&mut self, key: &TabKey) {
        let Some(repo) = self.open.get(&key.root).map(|open| open.repo.clone()) else {
            return;
        };
        let Some(draft) = self.tab(key).and_then(|tab| tab.draft.clone()) else {
            return;
        };
        if let Err(e) = save_request(&*repo, &key.path, &draft) {
            self.snapshot.error = Some(e.say(self.strings));
            return;
        }
        let revision = self.next_revision();
        if let Some(tab) = self.tab_mut(key) {
            tab.saved = draft;
            tab.draft = None;
            tab.revision = revision;
            // Saving replaced the file: nothing is left to settle.
            tab.conflict = false;
        }
        self.snapshot.error = None;
        self.refresh_variables(Some(key));
        // The name or method shown in the tree may have changed.
        self.reload_tree(&key.root);
    }

    /// Works out again the context of the tab at `only`, or of every tab.
    fn refresh_variables(&mut self, only: Option<&TabKey>) {
        let keys: Vec<TabKey> = (self.snapshot.tabs.iter())
            .filter(|tab| only.is_none_or(|key| tab.is(key)))
            .map(Tab::key)
            .collect();
        for key in &keys {
            self.load_schema(key);
        }
        for index in 0..self.snapshot.tabs.len() {
            let tab = &self.snapshot.tabs[index];
            if only.is_some_and(|key| !tab.is(key)) {
                continue;
            }
            let mut context = self.context(&tab.key(), tab.request());
            context.revision = self.next_revision();
            self.snapshot.tabs[index].context = Arc::new(context);
        }
    }

    /// Works out again the context of every tab of the collection at `root`.
    fn refresh_collection(&mut self, root: &Path) {
        let keys: Vec<TabKey> = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root)
            .map(Tab::key)
            .collect();
        for key in keys {
            self.refresh_variables(Some(&key));
        }
    }

    /// What `request` sees at `key` from outside itself.
    fn context(&self, key: &TabKey, request: &HttpRequest) -> TabContext {
        let Some(open) = self.open.get(&key.root) else {
            return TabContext::default();
        };
        // A request whose file is gone has no folders to read: it shows
        // nothing inherited until it is saved again.
        let (Ok(mut loaded), Ok(collection)) = (
            load_request(&*open.repo, &key.path),
            CollectionRepository::collection(&*open.repo),
        ) else {
            return TabContext::default();
        };
        loaded.request = request.clone();
        let variables = resolve_request_variables(
            &*open.repo,
            &*open.repo,
            &loaded,
            open.environment.as_deref(),
            &[],
        )
        .unwrap_or_default();
        // The request with nothing of its own shows what it inherits.
        let mut bare = request.clone();
        bare.headers.clear();
        bare.auth = Setting::Inherit;
        bare.settings = Settings::default();
        let mut inherited = resolve(Chain::new(&collection, &loaded.folders, &bare));
        inherited.use_default_timeout_ms(self.default_timeout_ms);
        let uses_oauth2 = matches!(
            (&request.auth, &inherited.auth().value),
            (Setting::Set(AuthScheme::OAuth2(_)), _) | (Setting::Inherit, AuthScheme::OAuth2(_))
        );
        let token = if uses_oauth2 {
            token_state(open, key, request, self.strings)
        } else {
            TokenState::NotUsed
        };
        let schema = schema::key_of(open, request)
            .map(|key| open.schemas.get(&key).cloned().unwrap_or_default());
        let methods = methods::key_of(open, request)
            .map(|key| open.methods.get(&key).cloned().unwrap_or_default());
        TabContext {
            token,
            schema,
            methods,
            variables,
            folders: loaded.folders.iter().map(|f| f.name.clone()).collect(),
            inherited_headers: inherited.headers().to_vec(),
            inherited_auth: inherited.auth().clone(),
            inherited_settings: inherited.settings().clone(),
            revision: 0,
        }
    }

    fn start_send(&mut self) {
        let Some(key) = self.snapshot.active.clone() else {
            return;
        };
        if self.sending.contains_key(&key) {
            return;
        }
        let Some(open) = self.open.get(&key.root) else {
            return;
        };
        let (repo, transport, responses, history) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
            open.history.clone(),
        );
        let tokens = open.tokens.clone();
        let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
            Some(vault) => vault.clone(),
            None => Arc::new(NoSecrets),
        };
        let Some(request) = self.tab(&key).map(Tab::request) else {
            return;
        };
        if request.body.is_websocket() || request.body.is_grpc() {
            let grpc = request.body.is_grpc();
            self.start_connect(key, grpc);
            return;
        }
        let written = (request.method.as_str().to_owned(), request.url.clone());
        let options = SendOptions {
            environment: open.environment.clone(),
            draft: self.tab(&key).and_then(|tab| tab.draft.clone()),
            runtime: open.runtime.clone(),
            script_changes: open.script_changes.clone(),
            default_timeout_ms: Some(self.default_timeout_ms),
            ..SendOptions::default()
        };
        let Some(tab) = self.tab_mut(&key) else {
            return;
        };
        tab.response = ResponseState::Sending;
        let cancel = CancelToken::new();
        let id = self.next_revision();
        let stream = LiveStream::default();
        self.sending.insert(
            key.clone(),
            InFlight {
                id,
                cancel: cancel.clone(),
                stream: stream.clone(),
                outbox: None,
            },
        );

        let messages = self.messages.clone();
        let live = stream::LiveResponses {
            store: responses.clone(),
            stream,
            id,
            messages: messages.clone(),
        };
        tokio::spawn(async move {
            let authorizer = apifast_http::LoopbackAuthorizer::system();
            let engine = apifast_script_js::QuickJs::default();
            let (clock, random) = (SystemClock, SystemRandom::default());
            let ports = SendPorts {
                collections: &*repo,
                environments: &*repo,
                transport: &*transport,
                responses: &live,
                clock: &clock,
                random: &random,
                prompter: &NoPrompter,
                secrets: &*secrets,
                report: &|_| {},
                history: Some(&*history),
                tokens: Some(tokens.as_ref()),
                authorizer: Some(&authorizer),
                scripts: Some(&engine),
            };
            let at_ms = clock.unix_millis();
            let result = send_request(ports, &key.path, &options, &cancel).await;
            let finished = finished(id, key, (written, at_ms), result, (&responses, &*history));
            // Gone only when the core is stopping, and then nobody waits.
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::SendFinished(Box::new(finished)));
            }
        });
    }

    /// Shows the event stream of send number `id` in its tab, from its
    /// first news on.
    fn streamed(&mut self, id: u64) {
        let Some((key, stream)) = (self.sending.iter())
            .find(|(_, sending)| sending.id == id)
            .map(|(key, sending)| (key.clone(), sending.stream.clone()))
        else {
            return;
        };
        stream.handled();
        if let Some(tab) = self.tab_mut(&key)
            && tab.response == ResponseState::Sending
        {
            tab.response = ResponseState::Streaming(stream);
        }
    }

    /// Records the result of a send in its collection's history and in its
    /// tab, wherever the tab is now. A tab closed meanwhile has no entry,
    /// and the result is dropped.
    fn finish_send(&mut self, finished: Finished) {
        let Finished {
            id,
            root,
            result,
            entry,
            dependencies,
        } = finished;
        if let Some(open) = self.open.get_mut(&root) {
            let entries = Arc::make_mut(&mut open.entries);
            entries.extend(dependencies);
            entries.extend(entry);
        }
        let Some(key) = (self.sending.iter())
            .find(|(_, sending)| sending.id == id)
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        self.sending.remove(&key);
        // The path, never the URL: its query may carry secrets.
        let path = key.path.as_str();
        match &result {
            Ok(received) => tracing::info!(
                path,
                status = received.head.status,
                ms = received.head.elapsed_ms,
                bytes = received.size,
                "sent"
            ),
            Err(AppError::Transport(TransportError::Cancelled)) => {
                tracing::debug!(path, "send cancelled");
            }
            Err(error) => tracing::warn!(path, %error, "send failed"),
        }
        let response = match result {
            Ok(received) => {
                let checks = &received.checks;
                if let Some(open) = self.open.get_mut(&key.root) {
                    apply_captures(&mut open.runtime, &checks.captures);
                    apply_script_variables(&mut open.runtime, &checks.scripts.variables);
                    keep_script_changes(&mut open.script_changes, &checks.scripts.variables);
                }
                self.note_environment_captures(&key.root, &checks.captures);
                let persisted = captures::persisted(&checks.scripts.variables);
                self.note_environment_captures(&key.root, &persisted);
                ResponseState::Received(received)
            }
            Err(AppError::Transport(TransportError::Cancelled)) => ResponseState::Cancelled,
            Err(e) => ResponseState::Failed(e.say(self.strings)),
        };
        if let Some(tab) = self.tab_mut(&key) {
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
                let entries = history.entries().unwrap_or_default();
                // The bodies of named entries stay, as the entries do.
                let keep: Vec<ResponseId> = (entries.iter())
                    .filter(|entry| entry.name().is_some())
                    .filter_map(|entry| match &entry.outcome {
                        HistoryOutcome::Received { body, .. } => Some(body.clone()),
                        HistoryOutcome::Failed(_) => None,
                    })
                    .collect();
                let _ = responses.prune_keeping(response_limits, now, &keep);
                entries
            })
            .await
            .unwrap_or_default();
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::HistoryLoaded(opened, entries));
            }
        });
    }

    /// Takes the history read for collection opening number `opened`, and
    /// shows in each of its tabs still empty its last response still kept.
    fn history_loaded(&mut self, opened: u64, entries: Vec<HistoryEntry>) {
        let Some(root) = self.root_of(opened) else {
            return;
        };
        let Some(open) = self.open.get_mut(&root) else {
            return;
        };
        let responses = open.responses.clone();
        for tab in self.snapshot.tabs.iter_mut().filter(|tab| tab.root == root) {
            if !matches!(tab.response, ResponseState::Idle) {
                continue;
            }
            let last = entries.iter().rev().find(|entry| entry.request == tab.path);
            if let Some(response) = last.and_then(|entry| response_of(&responses, entry, true)) {
                tab.response = response;
            }
        }
        // Sends that ended before the history was read are kept.
        let meanwhile: Vec<HistoryEntry> = (open.entries.iter())
            .filter(|entry| !entries.contains(entry))
            .cloned()
            .collect();
        let mut all = entries;
        all.extend(meanwhile);
        open.entries = Arc::new(all);
    }

    /// Opens an entry of the focused collection's history, or changes it.
    fn handle_history(&mut self, command: Command) {
        match command {
            Command::OpenHistory(index) => self.open_history(index),
            Command::History(command) => self.history_command(command),
            _ => {}
        }
    }

    /// Renames, removes or clears entries of the focused collection's
    /// history, in its file and in what the interface shows. The bodies of
    /// the entries removed go too.
    fn history_command(&mut self, command: HistoryCommand) {
        let Some(open) = (self.focus.as_ref()).and_then(|root| self.open.get_mut(root)) else {
            return;
        };
        let removed = match command {
            HistoryCommand::Rename { id, name } => {
                let name = Some(name.as_str());
                if let Err(error) = open.history.rename(&id, name) {
                    tracing::warn!(%error, "cannot rename a history entry");
                }
                rename_in(Arc::make_mut(&mut open.entries).as_mut_slice(), &id, name);
                Vec::new()
            }
            HistoryCommand::Remove(id) => {
                if let Err(error) = open.history.remove(&id) {
                    tracing::warn!(%error, "cannot remove a history entry");
                }
                let entries = Arc::make_mut(&mut open.entries);
                let index = entries.iter().position(|entry| entry.id == id);
                index
                    .map(|index| entries.remove(index))
                    .into_iter()
                    .collect()
            }
            HistoryCommand::Clear => {
                if let Err(error) = open.history.clear() {
                    tracing::warn!(%error, "cannot clear the history");
                }
                std::mem::take(Arc::make_mut(&mut open.entries))
            }
        };
        for entry in removed {
            if let HistoryOutcome::Received { body, .. } = &entry.outcome {
                // A body that stays is removed by the next pruning.
                let _ = open.responses.remove(body);
            }
        }
    }

    /// Shows entry `index` of the focused collection's history in its
    /// request's tab.
    fn open_history(&mut self, index: usize) {
        let Some(root) = self.focus.clone() else {
            return;
        };
        let Some(open) = self.open.get(&root) else {
            return;
        };
        let Some(entry) = open.entries.get(index).cloned() else {
            return;
        };
        let responses = open.responses.clone();
        let key = TabKey::new(root, entry.request.clone());
        self.open_request(&key);
        let sending = self.sending.contains_key(&key);
        match self.tab_mut(&key) {
            Some(tab) if !sending => {
                if let Some(response) = response_of(&responses, &entry, false) {
                    tab.response = response;
                }
            }
            _ => {}
        }
    }

    fn schedule_session_write(&mut self) {
        if self.workspace.is_none() || self.session_pending {
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

    /// Writes the session of every loaded collection (its drafts and its
    /// environment) and the workspace's state (its tabs).
    fn write_session(&mut self) {
        let roots: Vec<PathBuf> = self.open.keys().cloned().collect();
        for root in roots {
            self.write_collection_session(&root);
        }
        self.write_workspace_state();
    }

    /// Writes the session of the collection at `root`.
    fn write_collection_session(&mut self, root: &Path) {
        let Some(open) = self.open.get(root) else {
            return;
        };
        let mut tabs: Vec<SessionTab> = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root)
            .map(|tab| SessionTab {
                path: tab.path.clone(),
                draft: tab.draft.clone(),
            })
            .collect();
        tabs.extend(open.parked.iter().map(|(path, draft)| SessionTab {
            path: path.clone(),
            draft: Some(draft.clone()),
        }));
        let session = Session {
            tabs,
            active: None,
            environment: open.environment.clone(),
        };
        if let Err(e) = open.sessions.save(&session) {
            self.snapshot.error = Some(e.say(self.strings));
        }
    }
}
