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
    collection: Mutex<Collection>,
    folders: Mutex<BTreeMap<String, Folder>>,
    requests: Mutex<BTreeMap<RequestPath, HttpRequest>>,
}

impl MemoryCollections {
    /// An empty collection.
    pub fn new(collection: Collection) -> Self {
        Self {
            collection: Mutex::new(collection),
            folders: Mutex::new(BTreeMap::new()),
            requests: Mutex::new(BTreeMap::new()),
        }
    }

    /// Adds a folder at `path`.
    #[must_use]
    pub fn with_folder(self, path: &str, folder: Folder) -> Self {
        lock(&self.folders).insert(path.to_owned(), folder);
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

    /// The folder currently stored at `path`.
    pub fn folder(&self, path: &str) -> Option<Folder> {
        lock(&self.folders).get(path).cloned()
    }

    fn folder_chain(&self, path: &RequestPath) -> Vec<Folder> {
        let folders = lock(&self.folders);
        let parts: Vec<&str> = path.as_str().split('/').collect();
        (1..parts.len())
            .map(|depth| {
                folders
                    .get(&parts[..depth].join("/"))
                    .cloned()
                    .unwrap_or_else(|| Folder::new(parts[depth - 1]))
            })
            .collect()
    }

    fn items_under(
        prefix: &str,
        folders: &BTreeMap<String, Folder>,
        requests: &BTreeMap<RequestPath, HttpRequest>,
    ) -> Vec<TreeItem> {
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
        items.extend(
            folders
                .iter()
                .filter(|(path, _)| parent(path) == prefix)
                .map(|(path, folder)| TreeItem::Folder {
                    path: path.clone(),
                    folder: folder.clone(),
                    children: Self::items_under(path, folders, requests),
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
            collection: lock(&self.collection).clone(),
            items: Self::items_under("", &lock(&self.folders), &lock(&self.requests)),
        })
    }

    fn collection(&self) -> Result<Collection, RepositoryError> {
        Ok(lock(&self.collection).clone())
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

    fn save_folder(&self, path: &str, folder: &Folder) -> Result<(), RepositoryError> {
        lock(&self.folders).insert(path.to_owned(), folder.clone());
        Ok(())
    }

    fn save_collection(&self, collection: &Collection) -> Result<(), RepositoryError> {
        collection.clone_into(&mut lock(&self.collection));
        Ok(())
    }
}

/// Environments and process variables kept in memory.
#[derive(Debug, Default)]
pub struct MemoryEnvironments {
    environments: Mutex<Vec<Environment>>,
    process_env: BTreeMap<String, String>,
}

impl MemoryEnvironments {
    /// These environments, with no process variables.
    pub fn new(environments: Vec<Environment>) -> Self {
        Self {
            environments: Mutex::new(environments),
            process_env: BTreeMap::new(),
        }
    }

    /// Adds a value for `{{process.env.NAME}}`.
    #[must_use]
    pub fn with_process_env(mut self, name: &str, value: &str) -> Self {
        self.process_env.insert(name.to_owned(), value.to_owned());
        self
    }
}

impl EnvironmentRepository for MemoryEnvironments {
    fn environments(&self) -> Result<Vec<Environment>, RepositoryError> {
        Ok(lock(&self.environments).clone())
    }

    fn save_environment(&self, environment: &Environment) -> Result<(), RepositoryError> {
        let mut all = lock(&self.environments);
        match all.iter_mut().find(|e| e.name == environment.name) {
            Some(existing) => environment.clone_into(existing),
            None => all.push(environment.clone()),
        }
        Ok(())
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
