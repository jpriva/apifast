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
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use apifast_app::{
    AppError, CancelToken, CollectionTree, EnvironmentRepository, ItemPath, NoPrompter, Placement,
    RequestPath, ResponseHead, ResponseStore, SendOptions, SendPorts, Session, SessionStore,
    SessionTab, TransportError, create_folder, create_request, delete_item, load_request,
    move_item, open_collection, rename_item, save_request, send_request,
};
use apifast_domain::HttpRequest;
use apifast_history::FsResponseStore;
use apifast_http::ReqwestTransport;
use apifast_storage_fs::{FsCollection, FsSessionStore};
use tokio::sync::mpsc;

use crate::system::{SystemClock, SystemRandom};

/// How much of a response body the snapshot carries for display. The viewer
/// that reads bodies in windows replaces this.
pub const BODY_PREVIEW_BYTES: usize = 256 * 1024;

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
    /// The active environment.
    pub environment: Option<String>,
    /// Open tabs, in order.
    pub tabs: Vec<Tab>,
    /// The request of the active tab.
    pub active: Option<RequestPath>,
    /// The last error that is not about a send, such as a collection that
    /// cannot be opened.
    pub error: Option<String>,
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
    /// The start of the body as text (invalid UTF-8 replaced).
    pub preview: String,
    /// Whether the body is longer than the preview.
    pub truncated: bool,
}

/// Messages the core thread handles: commands from the interface and
/// results of its own tasks.
enum Message {
    Command(Command),
    /// A send ended; the number identifies it in [`State::sending`].
    SendFinished(u64, Result<Received, AppError>),
    WriteSession,
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
    sessions: FsSessionStore,
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
                Message::SendFinished(id, result) => self.finish_send(id, result),
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
            Command::SelectEnvironment(name) => self.snapshot.environment = name,
            Command::OpenRequest(path) => self.open_request(path),
            Command::ActivateTab(path) => {
                if self.tab(&path).is_some() {
                    self.snapshot.active = Some(path);
                }
            }
            Command::CloseTab(path) => self.close_tab(&path),
            Command::Edit { path, request } => {
                if let Some(tab) = self.tab_mut(&path) {
                    tab.draft = (*request != tab.saved).then_some(*request);
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
        }
    }

    fn tab(&self, path: &RequestPath) -> Option<&Tab> {
        self.snapshot.tabs.iter().find(|tab| tab.path == *path)
    }

    fn tab_mut(&mut self, path: &RequestPath) -> Option<&mut Tab> {
        self.snapshot.tabs.iter_mut().find(|tab| tab.path == *path)
    }

    fn open_collection(&mut self, root: PathBuf) -> Result<(), String> {
        let repo = FsCollection::open(&root).map_err(|e| e.to_string())?;
        let tree = open_collection(&repo).map_err(|e| e.to_string())?;
        let environments: Vec<String> = repo
            .environments()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| e.name)
            .collect();
        let transport = ReqwestTransport::new(&root).map_err(|e| e.to_string())?;
        let id = tree
            .collection
            .id
            .as_ref()
            .map(apifast_domain::CollectionId::as_str);
        let responses = FsResponseStore::new(apifast_history::dirs::responses_dir_in(
            &self.data_dir,
            &root,
            id,
        ));
        let sessions = FsSessionStore::new(apifast_history::dirs::sessions_dir_in(
            &self.data_dir,
            &root,
            id,
        ));

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
            let (saved, draft) = match load_request(&repo, &path) {
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
            });
        }
        let active = (session.active)
            .filter(|active| tabs.iter().any(|tab| tab.path == *active))
            .or_else(|| tabs.first().map(|tab| tab.path.clone()));
        let environment = (session.environment).filter(|name| environments.contains(name));

        self.open = Some(Open {
            repo: Arc::new(repo),
            transport: Arc::new(transport),
            responses: Arc::new(responses),
            sessions,
        });
        self.snapshot = Snapshot {
            version: self.snapshot.version,
            collection: Some(CollectionView { root, tree }),
            environments,
            environment,
            tabs,
            active,
            error,
        };
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
                };
                // New tabs go right after the active one.
                let at = (self.snapshot.active.as_ref())
                    .and_then(|active| self.snapshot.tabs.iter().position(|t| t.path == *active))
                    .map_or(self.snapshot.tabs.len(), |i| i + 1);
                self.snapshot.tabs.insert(at, tab);
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
        }
        self.snapshot.error = None;
        // The name or method shown in the tree may have changed.
        self.reload_tree();
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
            tab.path = to;
            tab.revision = revision;
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
        let (repo, transport, responses) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
        );
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
                report: &|_| {},
            };
            let result = send_request(ports, &path, &options, &cancel)
                .await
                .and_then(|outcome| {
                    let window = responses.read(&outcome.body.id, 0, BODY_PREVIEW_BYTES)?;
                    Ok(Received {
                        head: outcome.head,
                        size: outcome.body.size,
                        truncated: outcome.body.size > window.len() as u64,
                        preview: String::from_utf8_lossy(&window).into_owned(),
                    })
                });
            // Gone only when the core is stopping, and then nobody waits.
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::SendFinished(id, result));
            }
        });
    }

    /// Records the result of send number `id` in its tab, wherever the tab
    /// is now. A tab closed meanwhile has no entry, and the result is
    /// dropped.
    fn finish_send(&mut self, id: u64, result: Result<Received, AppError>) {
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
