//! The bridge between the interface and the use cases.
//!
//! [`Core`] runs on its own thread with a tokio runtime. The interface sends
//! it [`Command`]s and reads the latest [`Snapshot`], an immutable value the
//! core replaces whenever something changes. The interface never waits on
//! I/O: it only reads snapshots and sends commands.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use apifast_app::{
    AppError, CancelToken, CollectionTree, EnvironmentRepository, ItemPath, NoPrompter, Placement,
    RequestPath, ResponseHead, ResponseStore, SendOptions, SendPorts, TransportError,
    create_folder, create_request, delete_item, load_request, move_item, open_collection,
    rename_item, send_request,
};
use apifast_domain::HttpRequest;
use apifast_history::FsResponseStore;
use apifast_http::ReqwestTransport;
use apifast_storage_fs::FsCollection;
use tokio::sync::mpsc;

use crate::system::{SystemClock, SystemRandom};

/// How much of a response body the snapshot carries for display. The viewer
/// that reads bodies in windows replaces this.
pub const BODY_PREVIEW_BYTES: usize = 256 * 1024;

/// What the interface asks the core to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Opens the collection in this folder.
    OpenCollection(PathBuf),
    /// Picks the active environment, or none.
    SelectEnvironment(Option<String>),
    /// Loads a request of the open collection.
    OpenRequest(RequestPath),
    /// Sends the open request.
    Send,
    /// Cancels the send in progress.
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
    /// The open request.
    pub request: Option<OpenRequest>,
    /// The last send.
    pub response: ResponseState,
    /// The last error that is not about a send, such as a collection that
    /// cannot be opened.
    pub error: Option<String>,
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
pub struct OpenRequest {
    /// Its path in the collection.
    pub path: RequestPath,
    /// Its contents.
    pub request: HttpRequest,
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
    SendFinished(Result<Received, AppError>),
}

/// Handle to the core thread. Dropping it stops the thread.
pub struct Core {
    messages: mpsc::UnboundedSender<Message>,
    snapshot: Arc<Mutex<Arc<Snapshot>>>,
    thread: Option<JoinHandle<()>>,
}

impl Core {
    /// Starts the core. Responses are stored under `data_dir` (normally
    /// [`apifast_history::dirs::data_dir`]). `notify` is called after every
    /// new snapshot, from the core thread; the interface uses it to request
    /// a repaint.
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
                    session: None,
                    sending: None,
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
        // Closing the channel ends the core loop; the runtime then drops any
        // send in progress.
        let (closed, _) = mpsc::unbounded_channel();
        drop(std::mem::replace(&mut self.messages, closed));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The ports of an open collection.
struct Session {
    repo: Arc<FsCollection>,
    transport: Arc<ReqwestTransport>,
    responses: Arc<FsResponseStore>,
}

struct State {
    data_dir: PathBuf,
    snapshot: Snapshot,
    shared: Arc<Mutex<Arc<Snapshot>>>,
    notify: Box<dyn Fn() + Send + Sync>,
    session: Option<Session>,
    /// The send in progress, if any.
    sending: Option<CancelToken>,
    /// To post the results of spawned tasks back to the loop. Weak: a strong
    /// sender here would keep the loop alive after the handle is dropped.
    messages: mpsc::WeakUnboundedSender<Message>,
}

impl State {
    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = receiver.recv().await {
            match message {
                Message::Command(command) => self.handle(command),
                Message::SendFinished(result) => self.finish_send(result),
            }
            self.publish();
        }
    }

    fn publish(&mut self) {
        self.snapshot.version += 1;
        *self.shared.lock().unwrap_or_else(PoisonError::into_inner) =
            Arc::new(self.snapshot.clone());
        (self.notify)();
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
            Command::Send => self.start_send(),
            Command::Cancel => {
                if let Some(token) = &self.sending {
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
                    let open_inside = (self.snapshot.request.as_ref())
                        .is_some_and(|open| ItemPath::Request(open.path.clone()).is_within(&item));
                    if open_inside {
                        self.cancel_send();
                        self.snapshot.request = None;
                        self.snapshot.response = ResponseState::Idle;
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

    /// Runs a use case that changes the collection's files, then reloads the
    /// tree. Returns the use case's result, or `None` after reporting its
    /// error.
    fn organize<T>(
        &mut self,
        change: impl FnOnce(&FsCollection) -> Result<T, AppError>,
    ) -> Option<T> {
        let session = self.session.as_ref()?;
        let result = change(&session.repo);
        // Reload even after an error: part of the change may have happened.
        match open_collection(&*session.repo) {
            Ok(tree) => {
                if let Some(collection) = &mut self.snapshot.collection {
                    collection.tree = tree;
                }
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
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

    /// After `old` moved to `new`, keeps the open request open if it was
    /// `old` or inside it, reloading it (its name may have changed) but
    /// keeping the response.
    fn follow_move(&mut self, old: &ItemPath, new: &ItemPath) {
        let Some(open) = &self.snapshot.request else {
            return;
        };
        if !ItemPath::Request(open.path.clone()).is_within(old) {
            return;
        }
        let path = RequestPath::new(format!(
            "{}{}",
            new.as_str(),
            &open.path.as_str()[old.as_str().len()..]
        ));
        let Some(session) = &self.session else { return };
        match load_request(&*session.repo, &path) {
            Ok(loaded) => {
                self.snapshot.request = Some(OpenRequest {
                    path,
                    request: loaded.request,
                });
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    fn open_collection(&mut self, root: PathBuf) -> Result<(), String> {
        let repo = FsCollection::open(&root).map_err(|e| e.to_string())?;
        let tree = open_collection(&repo).map_err(|e| e.to_string())?;
        let environments = repo
            .environments()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| e.name)
            .collect();
        let transport = ReqwestTransport::new(&root).map_err(|e| e.to_string())?;
        let responses = FsResponseStore::new(apifast_history::dirs::responses_dir_in(
            &self.data_dir,
            &root,
            tree.collection
                .id
                .as_ref()
                .map(apifast_domain::CollectionId::as_str),
        ));
        self.cancel_send();
        self.session = Some(Session {
            repo: Arc::new(repo),
            transport: Arc::new(transport),
            responses: Arc::new(responses),
        });
        self.snapshot = Snapshot {
            version: self.snapshot.version,
            collection: Some(CollectionView { root, tree }),
            environments,
            ..Snapshot::default()
        };
        Ok(())
    }

    fn open_request(&mut self, path: RequestPath) {
        let Some(session) = &self.session else { return };
        match load_request(&*session.repo, &path) {
            Ok(loaded) => {
                self.cancel_send();
                self.snapshot.request = Some(OpenRequest {
                    path,
                    request: loaded.request,
                });
                self.snapshot.response = ResponseState::Idle;
                self.snapshot.error = None;
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    fn cancel_send(&mut self) {
        if let Some(token) = self.sending.take() {
            token.cancel();
        }
    }

    fn start_send(&mut self) {
        if self.sending.is_some() {
            return;
        }
        let (Some(session), Some(request)) = (&self.session, &self.snapshot.request) else {
            return;
        };
        let cancel = CancelToken::new();
        self.sending = Some(cancel.clone());
        self.snapshot.response = ResponseState::Sending;

        let (repo, transport, responses) = (
            session.repo.clone(),
            session.transport.clone(),
            session.responses.clone(),
        );
        let path = request.path.clone();
        let options = SendOptions {
            environment: self.snapshot.environment.clone(),
            ..SendOptions::default()
        };
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
                let _ = messages.send(Message::SendFinished(result));
            }
        });
    }

    fn finish_send(&mut self, result: Result<Received, AppError>) {
        self.sending = None;
        self.snapshot.response = match result {
            Ok(received) => ResponseState::Received(received),
            Err(AppError::Transport(TransportError::Cancelled)) => ResponseState::Cancelled,
            Err(e) => ResponseState::Failed(e.to_string()),
        };
    }
}
