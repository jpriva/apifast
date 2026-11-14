//! In-memory implementations of every port, for tests of the use cases and
//! of the drivers. They never touch the disk or the network.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use apifast_domain::{Collection, Environment, Folder, HttpRequest};

use crate::{
    BodyWriter, CancelToken, Clock, CollectionRepository, CollectionTree, EnvironmentRepository,
    HttpTransport, LoadedRequest, Prompter, Random, RenderedRequest, RepositoryError, RequestPath,
    ResponseHead, ResponseId, ResponseStore, StoreError, StoredBody, TransportError, TreeItem,
};

/// Locks a mutex, recovering the data if a panicking test poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A collection kept in memory. Folders are keyed by their path, such as
/// `users` or `users/admin`.
#[derive(Debug)]
pub struct MemoryCollections {
    collection: Collection,
    folders: BTreeMap<String, Folder>,
    requests: Mutex<BTreeMap<RequestPath, HttpRequest>>,
}

impl MemoryCollections {
    /// An empty collection.
    pub fn new(collection: Collection) -> Self {
        Self {
            collection,
            folders: BTreeMap::new(),
            requests: Mutex::new(BTreeMap::new()),
        }
    }

    /// Adds a folder at `path`.
    #[must_use]
    pub fn with_folder(mut self, path: &str, folder: Folder) -> Self {
        self.folders.insert(path.to_owned(), folder);
        self
    }

    /// Adds a request at `path`. Its folders are the parent directories.
    #[must_use]
    pub fn with_request(self, path: &str, request: HttpRequest) -> Self {
        lock(&self.requests).insert(RequestPath::new(path), request);
        self
    }

    /// The request currently stored at `path`.
    pub fn get(&self, path: &str) -> Option<HttpRequest> {
        lock(&self.requests).get(&RequestPath::new(path)).cloned()
    }

    fn folder_chain(&self, path: &RequestPath) -> Vec<Folder> {
        let parts: Vec<&str> = path.as_str().split('/').collect();
        let mut chain = Vec::new();
        for depth in 1..parts.len() {
            let folder_path = parts[..depth].join("/");
            let folder = self
                .folders
                .get(&folder_path)
                .cloned()
                .unwrap_or_else(|| Folder::new(parts[depth - 1]));
            chain.push(folder);
        }
        chain
    }

    fn items_under(&self, prefix: &str) -> Vec<TreeItem> {
        let requests = lock(&self.requests);
        let mut items: Vec<TreeItem> = requests
            .iter()
            .filter(|(path, _)| parent(path.as_str()) == prefix)
            .map(|(path, request)| TreeItem::Request {
                path: path.clone(),
                name: request.name.clone(),
                method: request.method.clone(),
                seq: request.seq,
            })
            .collect();
        drop(requests);
        items.extend(
            self.folders
                .iter()
                .filter(|(path, _)| parent(path) == prefix)
                .map(|(path, folder)| TreeItem::Folder {
                    path: path.clone(),
                    folder: folder.clone(),
                    children: self.items_under(path),
                }),
        );
        items
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl CollectionRepository for MemoryCollections {
    fn tree(&self) -> Result<CollectionTree, RepositoryError> {
        Ok(CollectionTree {
            collection: self.collection.clone(),
            items: self.items_under(""),
        })
    }

    fn collection(&self) -> Result<Collection, RepositoryError> {
        Ok(self.collection.clone())
    }

    fn request(&self, path: &RequestPath) -> Result<LoadedRequest, RepositoryError> {
        let request = lock(&self.requests)
            .get(path)
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound(path.to_string()))?;
        Ok(LoadedRequest {
            request,
            folders: self.folder_chain(path),
        })
    }

    fn save_request(
        &self,
        path: &RequestPath,
        request: &HttpRequest,
    ) -> Result<(), RepositoryError> {
        lock(&self.requests).insert(path.clone(), request.clone());
        Ok(())
    }
}

/// Environments and process variables kept in memory.
#[derive(Debug, Default, Clone)]
pub struct MemoryEnvironments {
    /// The environments.
    pub environments: Vec<Environment>,
    /// Values for `{{process.env.NAME}}`.
    pub process_env: BTreeMap<String, String>,
}

impl EnvironmentRepository for MemoryEnvironments {
    fn environments(&self) -> Result<Vec<Environment>, RepositoryError> {
        Ok(self.environments.clone())
    }

    fn process_env(&self) -> Result<BTreeMap<String, String>, RepositoryError> {
        Ok(self.process_env.clone())
    }
}

/// A transport that answers every request with a canned response and keeps
/// the requests it received.
#[derive(Debug)]
pub struct MemoryTransport {
    head: ResponseHead,
    body: Vec<u8>,
    sent: Mutex<Vec<RenderedRequest>>,
}

impl MemoryTransport {
    /// Answers with `status`, no headers and `body`.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            head: ResponseHead {
                status,
                headers: Vec::new(),
                elapsed_ms: 0,
            },
            body: body.into(),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// The requests received so far.
    pub fn sent(&self) -> Vec<RenderedRequest> {
        lock(&self.sent).clone()
    }
}

impl MemoryTransport {
    fn answer<W: BodyWriter>(
        &self,
        request: &RenderedRequest,
        body: &mut W,
        cancel: &CancelToken,
    ) -> Result<ResponseHead, TransportError> {
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        lock(&self.sent).push(request.clone());
        // Two chunks, to exercise streaming.
        let (first, second) = self.body.split_at(self.body.len() / 2);
        for chunk in [first, second] {
            body.write(chunk)
                .map_err(|e| TransportError::Other(e.to_string()))?;
        }
        Ok(self.head.clone())
    }
}

impl HttpTransport for MemoryTransport {
    fn send<W: BodyWriter + Send>(
        &self,
        request: &RenderedRequest,
        body: &mut W,
        cancel: &CancelToken,
    ) -> impl std::future::Future<Output = Result<ResponseHead, TransportError>> + Send {
        std::future::ready(self.answer(request, body, cancel))
    }
}

/// Response bodies kept in memory.
#[derive(Debug, Default)]
pub struct MemoryResponses {
    bodies: std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
}

/// Writer of [`MemoryResponses`].
#[derive(Debug)]
pub struct MemoryBodyWriter {
    bodies: std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
    buffer: Vec<u8>,
}

impl BodyWriter for MemoryBodyWriter {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    fn finish(self) -> Result<StoredBody, StoreError> {
        let mut bodies = lock(&self.bodies);
        let size = self.buffer.len() as u64;
        bodies.push(self.buffer);
        Ok(StoredBody {
            id: ResponseId((bodies.len() - 1).to_string()),
            size,
        })
    }
}

impl ResponseStore for MemoryResponses {
    type Writer = MemoryBodyWriter;

    fn create(&self) -> Result<MemoryBodyWriter, StoreError> {
        Ok(MemoryBodyWriter {
            bodies: self.bodies.clone(),
            buffer: Vec::new(),
        })
    }

    fn read(&self, id: &ResponseId, offset: u64, len: usize) -> Result<Vec<u8>, StoreError> {
        let bodies = lock(&self.bodies);
        let body =
            id.0.parse::<usize>()
                .ok()
                .and_then(|i| bodies.get(i))
                .ok_or_else(|| StoreError(format!("no response `{}`", id.0)))?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(body.len());
        let end = start.saturating_add(len).min(body.len());
        Ok(body[start..end].to_vec())
    }
}

/// A clock stopped at a fixed time.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn unix_millis(&self) -> i64 {
        self.0
    }
}

/// "Random" numbers that count up from a seed, for repeatable tests.
#[derive(Debug, Default)]
pub struct CountingRandom(AtomicU64);

impl CountingRandom {
    /// Starts counting at `seed`.
    pub fn new(seed: u64) -> Self {
        Self(AtomicU64::new(seed))
    }
}

impl Random for CountingRandom {
    fn next_u64(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

/// A prompter with fixed answers.
#[derive(Debug, Default, Clone)]
pub struct ScriptedPrompter {
    answers: BTreeMap<String, String>,
    asked: std::sync::Arc<Mutex<Vec<String>>>,
}

impl ScriptedPrompter {
    /// Adds an answer for `label`.
    #[must_use]
    pub fn answer(mut self, label: &str, value: &str) -> Self {
        self.answers.insert(label.to_owned(), value.to_owned());
        self
    }

    /// Labels asked so far, in order.
    pub fn asked(&self) -> Vec<String> {
        lock(&self.asked).clone()
    }
}

impl Prompter for ScriptedPrompter {
    fn prompt(&self, label: &str) -> Option<String> {
        lock(&self.asked).push(label.to_owned());
        self.answers.get(label).cloned()
    }
}
