//! Ports: what the use cases need from the outside world. Adapters implement
//! them; [`memory`](crate::memory) has in-memory versions for tests.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use apifast_domain::{Collection, Environment, Folder, HttpRequest, Method, Secret};

use crate::{RenderedRequest, RepositoryError, StoreError, TransportError};

/// Path of a request file relative to the collection root, with `/` as
/// separator, such as `users/list.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestPath(String);

impl RequestPath {
    /// Wraps a relative path. Backslashes become `/`.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into().replace('\\', "/"))
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A request together with the folders that contain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedRequest {
    /// The request.
    pub request: HttpRequest,
    /// Containing folders, from outermost to innermost.
    pub folders: Vec<Folder>,
}

/// Summary of a collection for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionTree {
    /// The collection root.
    pub collection: Collection,
    /// Top-level items, sorted.
    pub items: Vec<TreeItem>,
}

/// A folder or request in the [`CollectionTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeItem {
    /// A folder and its children.
    Folder {
        /// Path of the folder relative to the collection root.
        path: String,
        /// Folder metadata.
        folder: Folder,
        /// Children, sorted.
        children: Vec<TreeItem>,
    },
    /// A request.
    Request {
        /// Path of the request file.
        path: RequestPath,
        /// Display name.
        name: String,
        /// Method, shown next to the name.
        method: Method,
        /// Position among its siblings.
        seq: Option<u32>,
    },
    /// A request of a kind this version cannot open yet, such as GraphQL.
    /// It is listed so the tree matches the files on disk.
    Other {
        /// Path of the request file.
        path: RequestPath,
        /// Display name.
        name: String,
        /// Its `info.type`, such as `graphql`.
        kind: String,
        /// Position among its siblings.
        seq: Option<u32>,
    },
    /// A file that could not be read. One broken file must not hide the rest
    /// of the collection.
    Invalid {
        /// Path of the file.
        path: RequestPath,
        /// Why it could not be read.
        message: String,
    },
}

impl TreeItem {
    fn sort_key(&self) -> (Option<u32>, &str) {
        match self {
            Self::Folder { folder, .. } => (folder.seq, folder.name.as_str()),
            Self::Request { seq, name, .. } | Self::Other { seq, name, .. } => {
                (*seq, name.as_str())
            }
            Self::Invalid { path, .. } => (None, path.as_str()),
        }
    }
}

/// Sorts items by `seq`, then by name; items without `seq` go last.
/// Applies recursively to folder children.
pub fn sort_tree(items: &mut [TreeItem]) {
    items.sort_by(|a, b| {
        let (sa, na) = a.sort_key();
        let (sb, nb) = b.sort_key();
        (sa.is_none(), sa, na).cmp(&(sb.is_none(), sb, nb))
    });
    for item in items {
        if let TreeItem::Folder { children, .. } = item {
            sort_tree(children);
        }
    }
}

/// Collections on disk (or wherever the adapter keeps them).
pub trait CollectionRepository {
    /// Loads the whole tree of folders and requests.
    ///
    /// # Errors
    ///
    /// If the collection root cannot be read.
    fn tree(&self) -> Result<CollectionTree, RepositoryError>;

    /// Loads the collection root (`opencollection.yml`).
    ///
    /// # Errors
    ///
    /// If it cannot be read or is not valid.
    fn collection(&self) -> Result<Collection, RepositoryError>;

    /// Loads a request and its containing folders.
    ///
    /// # Errors
    ///
    /// If the request or one of its folders cannot be read or is not valid.
    fn request(&self, path: &RequestPath) -> Result<LoadedRequest, RepositoryError>;

    /// Saves a request atomically, keeping keys the domain does not model.
    /// Creates the file if it does not exist.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    fn save_request(
        &self,
        path: &RequestPath,
        request: &HttpRequest,
    ) -> Result<(), RepositoryError>;

    /// Saves the metadata of the folder at `path` (relative to the root,
    /// such as `users/admin`), creating its `folder.yml` if needed.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    fn save_folder(&self, path: &str, folder: &Folder) -> Result<(), RepositoryError>;

    /// Saves the collection root.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    fn save_collection(&self, collection: &Collection) -> Result<(), RepositoryError>;

    /// Whether a request file or folder exists at `path` (relative to the
    /// root, such as `users/list.yml` or `users`).
    ///
    /// # Errors
    ///
    /// If `path` is not a valid path inside the collection.
    fn exists(&self, path: &str) -> Result<bool, RepositoryError>;

    /// Moves the request file or folder at `from` to `to`, with everything
    /// inside it. Never replaces something that already exists at `to`.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] if nothing is at `from`,
    /// [`RepositoryError::Invalid`] if something is already at `to`, or the
    /// error of the move.
    fn rename_path(&self, from: &str, to: &str) -> Result<(), RepositoryError>;

    /// Deletes the request file or folder at `path`, with everything inside
    /// it.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] if nothing is at `path`, or the error
    /// of the deletion.
    fn delete_path(&self, path: &str) -> Result<(), RepositoryError>;
}

/// A workspace: a named group of collections, written in `workspace.yml`
/// as Bruno does (ADR 0009).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    /// Its name (`info.name`).
    pub name: String,
    /// Its collections, in order.
    pub collections: Vec<WorkspaceEntry>,
}

/// A collection of a [`Workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    /// The name Bruno shows for it.
    pub name: String,
    /// Its folder as written: relative to the workspace's folder, or
    /// absolute.
    pub path: String,
}

/// Reads and saves a [`Workspace`], keeping what it does not model.
pub trait WorkspaceRepository {
    /// The workspace.
    ///
    /// # Errors
    ///
    /// If it cannot be read or is not valid.
    fn workspace(&self) -> Result<Workspace, RepositoryError>;

    /// Saves it, changing only what differs.
    ///
    /// # Errors
    ///
    /// If it cannot be written.
    fn save_workspace(&self, workspace: &Workspace) -> Result<(), RepositoryError>;
}

/// What the interface had open in a collection, kept outside it in the
/// user's data directory (ADR 0008), so that tabs and unsaved changes
/// survive a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// Open tabs, in order.
    pub tabs: Vec<SessionTab>,
    /// The active tab.
    pub active: Option<RequestPath>,
    /// The active environment.
    pub environment: Option<String>,
}

/// An open tab of a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTab {
    /// The request it shows.
    pub path: RequestPath,
    /// Its unsaved changes, if any.
    pub draft: Option<HttpRequest>,
}

/// Keeps the [`Session`] of one collection.
pub trait SessionStore {
    /// The saved session, or an empty one if there is none.
    ///
    /// # Errors
    ///
    /// If it exists but cannot be read.
    fn load(&self) -> Result<Session, StoreError>;

    /// Replaces the saved session.
    ///
    /// # Errors
    ///
    /// If it cannot be written.
    fn save(&self, session: &Session) -> Result<(), StoreError>;
}

/// A request file or a folder of the collection: what the sidebar creates,
/// renames, moves and deletes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemPath {
    /// A request file, of any kind (also one that cannot be read).
    Request(RequestPath),
    /// A folder, such as `users/admin`.
    Folder(String),
}

impl ItemPath {
    /// The path relative to the collection root.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Request(path) => path.as_str(),
            Self::Folder(path) => path,
        }
    }

    /// The folder that holds the item, `""` for the root.
    pub fn parent(&self) -> &str {
        self.as_str()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent)
    }

    /// The last part of the path: the file or folder name.
    pub fn file_name(&self) -> &str {
        let path = self.as_str();
        path.rsplit_once('/').map_or(path, |(_, name)| name)
    }

    /// Whether this item is `other` or is inside it.
    pub fn is_within(&self, other: &ItemPath) -> bool {
        is_within(self.as_str(), other.as_str())
    }
}

/// Whether `path` is `folder` or is inside it.
pub(crate) fn is_within(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Environments and process variables of a collection.
pub trait EnvironmentRepository {
    /// All environments of the collection.
    ///
    /// # Errors
    ///
    /// If an environment file cannot be read or is not valid.
    fn environments(&self) -> Result<Vec<Environment>, RepositoryError>;

    /// Saves an environment, matched by name, creating its file if needed.
    ///
    /// # Errors
    ///
    /// If the file cannot be written.
    fn save_environment(&self, environment: &Environment) -> Result<(), RepositoryError>;

    /// Renames environment `from` to `to`, keeping the rest of its file.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no `from`, `Invalid` if `to` already exists,
    /// or if the file cannot be written.
    fn rename_environment(&self, from: &str, to: &str) -> Result<(), RepositoryError>;

    /// Removes an environment.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such environment, or if its file cannot be
    /// removed.
    fn delete_environment(&self, name: &str) -> Result<(), RepositoryError>;

    /// Values for `{{process.env.NAME}}`: the collection's `.env` over the
    /// system environment.
    ///
    /// # Errors
    ///
    /// If `.env` exists but cannot be read.
    fn process_env(&self) -> Result<BTreeMap<String, String>, RepositoryError>;
}

/// Status line and headers of a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    /// HTTP status code.
    pub status: u16,
    /// HTTP version, such as `HTTP/1.1` or `HTTP/2.0`.
    pub version: String,
    /// URL of the response, after any redirects.
    pub url: String,
    /// How many redirects were followed.
    pub redirects: u32,
    /// Header names and values, in order.
    pub headers: Vec<(String, String)>,
    /// Time from sending to the end of the body, in milliseconds.
    pub elapsed_ms: u64,
    /// What happened while sending, in order.
    pub timeline: Vec<TimelineEvent>,
}

/// Something that happened while sending, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEvent {
    /// Microseconds since the send started.
    pub at_us: u64,
    /// What happened.
    pub kind: TimelineKind,
}

/// What happened while sending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineKind {
    /// A request went out: the first one, or the next after a redirect.
    Request {
        /// Its method.
        method: String,
        /// Its URL.
        url: String,
    },
    /// The server answered with a redirect, which was followed.
    Redirect {
        /// The redirect's status.
        status: u16,
        /// Where it pointed, as sent by the server.
        location: String,
    },
    /// The status and headers of the final response arrived.
    Headers {
        /// The status.
        status: u16,
    },
    /// The whole body arrived.
    Body {
        /// Its size in bytes.
        bytes: u64,
    },
}

/// Sends rendered requests.
pub trait HttpTransport {
    /// Sends `request`, writes the response body into `body` as it arrives
    /// and returns the status and headers. Checks `cancel` between reads.
    ///
    /// # Errors
    ///
    /// On connection failure, timeout, cancellation or a body write error.
    fn send<W: BodyWriter + Send>(
        &self,
        request: &RenderedRequest,
        body: &mut W,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send;
}

/// Identifier of a stored response body.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResponseId(pub String);

/// A stored response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBody {
    /// Identifier to read it back.
    pub id: ResponseId,
    /// Size in bytes.
    pub size: u64,
}

/// Receives a response body in chunks.
pub trait BodyWriter {
    /// Appends a chunk.
    ///
    /// # Errors
    ///
    /// If the chunk cannot be stored.
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError>;

    /// Ends the body and returns where it was stored.
    ///
    /// # Errors
    ///
    /// If the body cannot be finalized.
    fn finish(self) -> Result<StoredBody, StoreError>;
}

/// Response bodies, written in chunks and read back in windows, so the UI
/// never holds a large body in memory.
pub trait ResponseStore {
    /// Writer for a new body.
    type Writer: BodyWriter + Send;

    /// Starts a new body.
    ///
    /// # Errors
    ///
    /// If the store cannot create it.
    fn create(&self) -> Result<Self::Writer, StoreError>;

    /// Reads up to `len` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// If the body does not exist or cannot be read.
    fn read(&self, id: &ResponseId, offset: u64, len: usize) -> Result<Vec<u8>, StoreError>;
}

/// Current time. `Send + Sync` so use cases can run as tasks on any thread.
pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch.
    fn unix_millis(&self) -> i64;
}

/// Source of randomness for dynamic variables. `Send + Sync` so use cases
/// can run as tasks on any thread.
pub trait Random: Send + Sync {
    /// A uniformly random number.
    fn next_u64(&self) -> u64;
}

/// Waits between the requests of a run. `Send + Sync` so use cases can run
/// as tasks on any thread.
pub trait Pause: Send + Sync {
    /// Waits `ms` milliseconds.
    fn pause(&self, ms: u64) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Asks the user for `{{?Label}}` values. `Send + Sync` so use cases can run
/// as tasks on any thread.
pub trait Prompter: Send + Sync {
    /// The value typed for `label`, or `None` if the user gave none.
    fn prompt(&self, label: &str) -> Option<String>;
}

/// A prompter that never answers, for contexts without a user.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrompter;

impl Prompter for NoPrompter {
    fn prompt(&self, _label: &str) -> Option<String> {
        None
    }
}

/// Shared flag to cancel a send. Clones share the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(name: &str, seq: Option<u32>) -> TreeItem {
        TreeItem::Request {
            path: RequestPath::new(format!("{name}.yml")),
            name: name.into(),
            method: Method::Get,
            seq,
        }
    }

    fn names(items: &[TreeItem]) -> Vec<&str> {
        items.iter().map(|i| i.sort_key().1).collect()
    }

    #[test]
    fn tree_sorts_by_seq_then_name_with_unsequenced_last() {
        let mut items = vec![
            request("zeta", None),
            request("beta", Some(2)),
            request("alpha", Some(2)),
            request("first", Some(1)),
            request("omega", None),
        ];
        sort_tree(&mut items);
        assert_eq!(names(&items), ["first", "alpha", "beta", "omega", "zeta"]);
    }

    #[test]
    fn request_paths_use_forward_slashes() {
        assert_eq!(
            RequestPath::new("users\\list.yml").as_str(),
            "users/list.yml"
        );
    }

    #[test]
    fn cancel_tokens_share_their_flag() {
        let token = CancelToken::new();
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
    }
}

/// The values of the secret variables of one collection, kept in the vault.
/// Values are only read here; storing them is up to the vault itself.
pub trait SecretStore: Sync {
    /// The value of secret `name` of `environment`, if this user has it.
    ///
    /// # Errors
    ///
    /// If the vault cannot be read, such as a locked keyring.
    fn secret(&self, environment: &str, name: &str) -> Result<Option<Secret>, StoreError>;
}

/// The collection's side of shared secrets: the sealed values in
/// `.apifast/secrets/` and the vault's challenge, both versioned with the
/// collection. Values are envelopes; only the vault opens them.
pub trait SharedSecretFiles: Send + Sync {
    /// The envelope of shared secret `name` of `environment`, if any.
    ///
    /// # Errors
    ///
    /// If the files cannot be read.
    fn shared_secret(
        &self,
        environment: &str,
        name: &str,
    ) -> Result<Option<String>, RepositoryError>;

    /// Stores `envelope` as shared secret `name` of `environment`.
    ///
    /// # Errors
    ///
    /// If the files cannot be written.
    fn set_shared_secret(
        &self,
        environment: &str,
        name: &str,
        envelope: &str,
    ) -> Result<(), RepositoryError>;

    /// Removes shared secret `name` of `environment`. Returns whether it was
    /// there.
    ///
    /// # Errors
    ///
    /// If the files cannot be written.
    fn remove_shared_secret(&self, environment: &str, name: &str) -> Result<bool, RepositoryError>;

    /// Every shared secret, as `(environment, name)`.
    ///
    /// # Errors
    ///
    /// If the files cannot be read.
    fn shared_secret_entries(&self) -> Result<Vec<(String, String)>, RepositoryError>;

    /// The vault's challenge, if the collection has shared secrets.
    ///
    /// # Errors
    ///
    /// If the file cannot be read.
    fn vault_challenge(&self) -> Result<Option<String>, RepositoryError>;

    /// Writes the vault's challenge.
    ///
    /// # Errors
    ///
    /// If the collection has no id yet, or the file cannot be written.
    fn set_vault_challenge(&self, challenge: &str) -> Result<(), RepositoryError>;
}

/// No vault: every secret is without value.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSecrets;

impl SecretStore for NoSecrets {
    fn secret(&self, _: &str, _: &str) -> Result<Option<Secret>, StoreError> {
        Ok(None)
    }
}
