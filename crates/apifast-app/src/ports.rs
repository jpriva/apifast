//! Ports: what the use cases need from the outside world. Adapters implement
//! them; [`memory`](crate::memory) has in-memory versions for tests.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use apifast_domain::{Collection, Environment, Folder, HttpRequest, Method};

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

/// Current time.
pub trait Clock {
    /// Milliseconds since the Unix epoch.
    fn unix_millis(&self) -> i64;
}

/// Source of randomness for dynamic variables.
pub trait Random {
    /// A uniformly random number.
    fn next_u64(&self) -> u64;
}

/// Asks the user for `{{?Label}}` values.
pub trait Prompter {
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
