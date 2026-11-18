//! In-memory implementations of every port, for tests of the use cases and
//! of the drivers. They never touch the disk or the network.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use apifast_domain::{Collection, Environment, Folder, HttpRequest, Secret};

use crate::ports::is_within;
use crate::{
    BodyWriter, CancelToken, Clock, CollectionRepository, CollectionTree, EnvironmentRepository,
    HistoryEntry, HistoryLimits, HistoryStore, HttpTransport, LoadedRequest, Prompter, Random,
    RenderedRequest, RepositoryError, RequestPath, ResponseHead, ResponseId, ResponseStore,
    SecretStore, Session, SessionStore, SharedSecretFiles, StoreError, StoredBody, TransportError,
    TreeItem, Workspace, WorkspaceRepository, within_limits,
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
        // In the order of a real collection: by `seq`, then by name.
        crate::sort_tree(&mut items);
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

    fn exists(&self, path: &str) -> Result<bool, RepositoryError> {
        let folders = lock(&self.folders);
        let requests = lock(&self.requests);
        // A folder exists while it has a `folder.yml` or anything inside.
        Ok(folders.keys().any(|f| is_within(f, path))
            || requests.keys().any(|r| is_within(r.as_str(), path)))
    }

    fn rename_path(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        if !self.exists(from)? {
            return Err(RepositoryError::NotFound(from.to_owned()));
        }
        if self.exists(to)? {
            return Err(RepositoryError::Invalid {
                path: to.to_owned(),
                message: "something already exists there".into(),
            });
        }
        let moved = |path: &str| format!("{to}{}", &path[from.len()..]);
        let mut folders = lock(&self.folders);
        let keys: Vec<String> = folders
            .keys()
            .filter(|f| is_within(f, from))
            .cloned()
            .collect();
        for key in keys {
            if let Some(folder) = folders.remove(&key) {
                folders.insert(moved(&key), folder);
            }
        }
        let mut requests = lock(&self.requests);
        let keys: Vec<RequestPath> = requests
            .keys()
            .filter(|r| is_within(r.as_str(), from))
            .cloned()
            .collect();
        for key in keys {
            if let Some(request) = requests.remove(&key) {
                requests.insert(RequestPath::new(moved(key.as_str())), request);
            }
        }
        Ok(())
    }

    fn delete_path(&self, path: &str) -> Result<(), RepositoryError> {
        if !self.exists(path)? {
            return Err(RepositoryError::NotFound(path.to_owned()));
        }
        lock(&self.folders).retain(|f, _| !is_within(f, path));
        lock(&self.requests).retain(|r, _| !is_within(r.as_str(), path));
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

    fn rename_environment(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        let mut all = lock(&self.environments);
        if all.iter().any(|e| e.name == to) {
            return Err(RepositoryError::Invalid {
                path: to.to_owned(),
                message: "an environment with that name already exists".into(),
            });
        }
        let environment = all
            .iter_mut()
            .find(|e| e.name == from)
            .ok_or_else(|| RepositoryError::NotFound(from.to_owned()))?;
        to.clone_into(&mut environment.name);
        Ok(())
    }

    fn delete_environment(&self, name: &str) -> Result<(), RepositoryError> {
        let mut all = lock(&self.environments);
        let before = all.len();
        all.retain(|e| e.name != name);
        if all.len() == before {
            return Err(RepositoryError::NotFound(name.to_owned()));
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
                version: "HTTP/1.1".into(),
                url: String::new(),
                redirects: 0,
                headers: Vec::new(),
                elapsed_ms: 0,
                timeline: Vec::new(),
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
        let mut head = self.head.clone();
        request.url().clone_into(&mut head.url);
        Ok(head)
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

/// A workspace kept in memory.
#[derive(Debug, Default)]
pub struct MemoryWorkspace {
    workspace: Mutex<Workspace>,
}

impl MemoryWorkspace {
    /// Holding `workspace`.
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace: Mutex::new(workspace),
        }
    }
}

impl WorkspaceRepository for MemoryWorkspace {
    fn workspace(&self) -> Result<Workspace, RepositoryError> {
        Ok(lock(&self.workspace).clone())
    }

    fn save_workspace(&self, workspace: &Workspace) -> Result<(), RepositoryError> {
        workspace.clone_into(&mut lock(&self.workspace));
        Ok(())
    }
}

/// A session kept in memory.
#[derive(Debug, Default)]
pub struct MemorySessions {
    session: Mutex<Session>,
}

impl MemorySessions {
    /// The session saved last.
    pub fn saved(&self) -> Session {
        lock(&self.session).clone()
    }
}

impl SessionStore for MemorySessions {
    fn load(&self) -> Result<Session, StoreError> {
        Ok(self.saved())
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        session.clone_into(&mut lock(&self.session));
        Ok(())
    }
}

/// A history kept in memory.
#[derive(Debug, Default)]
pub struct MemoryHistory {
    entries: Mutex<Vec<HistoryEntry>>,
}

impl HistoryStore for MemoryHistory {
    fn append(&self, entry: &HistoryEntry) -> Result<(), StoreError> {
        lock(&self.entries).push(entry.clone());
        Ok(())
    }

    fn entries(&self) -> Result<Vec<HistoryEntry>, StoreError> {
        Ok(lock(&self.entries).clone())
    }

    fn prune(&self, limits: HistoryLimits, now_ms: i64) -> Result<usize, StoreError> {
        let mut entries = lock(&self.entries);
        let before = entries.len();
        *entries = within_limits(std::mem::take(&mut *entries), limits, now_ms);
        Ok(before - entries.len())
    }
}

/// Secret values in memory, by environment and name.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    values: Mutex<BTreeMap<(String, String), String>>,
}

impl MemorySecrets {
    /// Adds the value of secret `name` of `environment`.
    #[must_use]
    pub fn with(self, environment: &str, name: &str, value: &str) -> Self {
        lock(&self.values).insert((environment.to_owned(), name.to_owned()), value.to_owned());
        self
    }
}

impl SecretStore for MemorySecrets {
    fn secret(&self, environment: &str, name: &str) -> Result<Option<Secret>, StoreError> {
        Ok(lock(&self.values)
            .get(&(environment.to_owned(), name.to_owned()))
            .map(Secret::new))
    }
}

/// Shared secret files and the challenge, in memory.
#[derive(Debug, Default)]
pub struct MemorySharedSecrets {
    secrets: Mutex<BTreeMap<(String, String), String>>,
    challenge: Mutex<Option<String>>,
}

impl SharedSecretFiles for MemorySharedSecrets {
    fn shared_secret(
        &self,
        environment: &str,
        name: &str,
    ) -> Result<Option<String>, RepositoryError> {
        Ok(lock(&self.secrets)
            .get(&(environment.to_owned(), name.to_owned()))
            .cloned())
    }

    fn set_shared_secret(
        &self,
        environment: &str,
        name: &str,
        envelope: &str,
    ) -> Result<(), RepositoryError> {
        lock(&self.secrets).insert(
            (environment.to_owned(), name.to_owned()),
            envelope.to_owned(),
        );
        Ok(())
    }

    fn remove_shared_secret(&self, environment: &str, name: &str) -> Result<bool, RepositoryError> {
        Ok(lock(&self.secrets)
            .remove(&(environment.to_owned(), name.to_owned()))
            .is_some())
    }

    fn shared_secret_entries(&self) -> Result<Vec<(String, String)>, RepositoryError> {
        Ok(lock(&self.secrets).keys().cloned().collect())
    }

    fn vault_challenge(&self) -> Result<Option<String>, RepositoryError> {
        Ok(lock(&self.challenge).clone())
    }

    fn set_vault_challenge(&self, challenge: &str) -> Result<(), RepositoryError> {
        *lock(&self.challenge) = Some(challenge.to_owned());
        Ok(())
    }
}

/// A pause that does not wait and records how long it was asked to.
#[derive(Debug, Default)]
pub struct RecordedPauses(Mutex<Vec<u64>>);

impl RecordedPauses {
    /// The pauses asked for, in milliseconds.
    pub fn asked(&self) -> Vec<u64> {
        lock(&self.0).clone()
    }
}

impl crate::Pause for RecordedPauses {
    fn pause(
        &self,
        ms: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        lock(&self.0).push(ms);
        Box::pin(std::future::ready(()))
    }
}

/// OAuth 2.0 tokens kept in memory: for a run of the CLI, or while a
/// collection is loaded in the interface.
#[derive(Debug, Default)]
pub struct MemoryTokens(Mutex<BTreeMap<String, crate::OAuth2Token>>);

impl crate::TokenStore for MemoryTokens {
    fn token(&self, key: &str) -> Result<Option<crate::OAuth2Token>, StoreError> {
        Ok(lock(&self.0).get(key).cloned())
    }

    fn save(&self, key: &str, token: &crate::OAuth2Token) -> Result<(), StoreError> {
        lock(&self.0).insert(key.to_owned(), token.clone());
        Ok(())
    }

    fn forget(&self, key: &str) -> Result<(), StoreError> {
        lock(&self.0).remove(key);
        Ok(())
    }
}

/// A [`CookieStore`](crate::CookieStore) in memory.
#[derive(Debug, Default)]
pub struct MemoryCookies(Mutex<Vec<apifast_domain::Cookie>>);

impl crate::CookieStore for MemoryCookies {
    fn load(&self) -> Result<Vec<apifast_domain::Cookie>, StoreError> {
        Ok(lock(&self.0).clone())
    }

    fn save(&self, cookies: &[apifast_domain::Cookie]) -> Result<(), StoreError> {
        cookies.clone_into(&mut lock(&self.0));
        Ok(())
    }
}
