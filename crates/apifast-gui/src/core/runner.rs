//! The collection runner in the interface (docs/08): one run at a time, in
//! the background, its progress published as it goes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::{
    CancelToken, Clock, CollectionRepository, DataFormat, HistoryEntry, HistoryStore, NoPrompter,
    NoSecrets, Pause, RequestPath, RunEvent, RunOptions, RunReport, SecretStore, SendOptions,
    SendPorts, StepResult, parse_data, run_collection,
};

use super::{Checks, Message, Received, ResponseState, State, TabKey, received};
use crate::system::{SystemClock, SystemRandom};

/// What the user chose to run, before the core adds the environment and
/// the runtime values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    /// The folder, relative to the collection; `None` for all of it.
    pub folder: Option<String>,
    /// Whether subfolders run too.
    pub recursive: bool,
    /// Only requests with one of these tags.
    pub tags: Vec<String>,
    /// No requests with any of these tags.
    pub exclude_tags: Vec<String>,
    /// Pause between requests, in milliseconds.
    pub delay_ms: u64,
    /// Stop at the first failure.
    pub bail: bool,
    /// Repetitions, without data.
    pub iterations: u32,
    /// A CSV or JSON file: one iteration per row.
    pub data: Option<PathBuf>,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            folder: None,
            recursive: true,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            delay_ms: 0,
            bail: false,
            iterations: 1,
            data: None,
        }
    }
}

/// A run, as the interface sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    /// The collection.
    pub root: PathBuf,
    /// Its name, for reports.
    pub collection: String,
    /// What was asked.
    pub settings: RunSettings,
    /// The active environment when it started.
    pub environment: Option<String>,
    /// When it started, in milliseconds since the Unix epoch.
    pub started_ms: i64,
    /// Whether it is still going.
    pub running: bool,
    /// Requests per iteration, once known.
    pub total: usize,
    /// How many iterations.
    pub iterations: u32,
    /// The request being sent: iteration, place and file.
    pub current: Option<(u32, usize, RequestPath)>,
    /// The requests done, in order.
    pub steps: Arc<Vec<StepResult>>,
    /// The result, once it ended.
    pub report: Option<Arc<RunReport>>,
    /// Why it could not run.
    pub error: Option<String>,
}

impl RunView {
    /// How far it went, from 0 to 1.
    pub fn progress(&self) -> f32 {
        let all = self.total * self.iterations as usize;
        if all == 0 {
            return if self.running { 0.0 } else { 1.0 };
        }
        #[expect(clippy::cast_precision_loss, reason = "a fraction for a progress bar")]
        let done = self.steps.len() as f32 / all as f32;
        done.min(1.0)
    }
}

/// What a run tells the core while it goes.
pub(super) enum RunMessage {
    Started {
        iteration: u32,
        index: usize,
        total: usize,
        path: RequestPath,
    },
    Step(Box<StepResult>, Option<HistoryEntry>),
    Done(Result<RunReport, String>),
}

/// Waits on the tokio timer.
struct TokioPause;

impl Pause for TokioPause {
    fn pause(&self, ms: u64) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(ms)))
    }
}

fn read_data(file: &Path) -> Result<Vec<Vec<apifast_domain::Variable>>, String> {
    let name = file.to_string_lossy();
    let format = DataFormat::of(&name)
        .ok_or_else(|| format!("`{name}`: the data file must end in .csv or .json"))?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("cannot read `{name}`: {e}"))?;
    parse_data(&text, format).map_err(|e| format!("`{name}`: {e}"))
}

impl State {
    /// Starts a run of the collection at `root`, unless one is going.
    pub(super) fn start_run(&mut self, root: &Path, settings: RunSettings) {
        if self.snapshot.run.as_ref().is_some_and(|run| run.running) {
            return;
        }
        if !self.open.contains_key(root) && self.load(root).is_err() {
            return;
        }
        let id = self.next_revision();
        let Some(open) = self.open.get(root) else {
            return;
        };
        let data = match settings.data.as_deref().map(read_data).transpose() {
            Ok(data) => data.unwrap_or_default(),
            Err(message) => {
                self.snapshot.error = Some(message);
                return;
            }
        };
        let options = RunOptions {
            folder: settings.folder.clone(),
            recursive: settings.recursive,
            tags: settings.tags.clone(),
            exclude_tags: settings.exclude_tags.clone(),
            delay_ms: settings.delay_ms,
            bail: settings.bail,
            iterations: settings.iterations.max(1),
            data,
            send: SendOptions {
                environment: open.environment.clone(),
                runtime: open.runtime.clone(),
                ..SendOptions::default()
            },
        };
        let iterations = if options.data.is_empty() {
            options.iterations
        } else {
            u32::try_from(options.data.len()).unwrap_or(u32::MAX)
        };
        let ports = Ports {
            repo: open.repo.clone(),
            transport: open.transport.clone(),
            responses: open.responses.clone(),
            history: open.history.clone(),
            tokens: open.tokens.clone(),
            secrets: match &open.vault {
                Some(vault) => vault.clone(),
                None => Arc::new(NoSecrets),
            },
        };
        let cancel = CancelToken::new();
        let view = RunView {
            root: root.to_path_buf(),
            collection: open.tree.collection.name.clone(),
            settings,
            environment: open.environment.clone(),
            started_ms: SystemClock.unix_millis(),
            running: true,
            total: 0,
            iterations,
            current: None,
            steps: Arc::default(),
            report: None,
            error: None,
        };
        self.snapshot.run = Some(view);
        self.run = Some((id, cancel.clone()));

        spawn_run(id, ports, options, cancel, self.messages.clone());
    }

    /// Stops the run that is going.
    pub(super) fn stop_run(&mut self) {
        if let Some((_, cancel)) = &self.run {
            cancel.cancel();
        }
    }

    /// Forgets a run that ended.
    pub(super) fn close_run(&mut self) {
        if self.snapshot.run.as_ref().is_some_and(|run| !run.running) {
            self.snapshot.run = None;
        }
    }

    pub(super) fn run_message(&mut self, id: u64, message: RunMessage) {
        if self.run.as_ref().is_none_or(|(run, _)| *run != id) {
            return;
        }
        let Some(view) = self.snapshot.run.as_mut() else {
            return;
        };
        match message {
            RunMessage::Started {
                iteration,
                index,
                total,
                path,
            } => {
                view.total = total;
                view.current = Some((iteration, index, path));
            }
            RunMessage::Step(step, entry) => {
                Arc::make_mut(&mut view.steps).push(*step);
                if let (Some(entry), Some(open)) = (entry, self.open.get_mut(&view.root)) {
                    Arc::make_mut(&mut open.entries).push(entry);
                }
            }
            RunMessage::Done(result) => {
                view.running = false;
                view.current = None;
                self.run = None;
                match result {
                    Ok(report) => {
                        // What the run captured is there for the next sends.
                        if let Some(open) = self.open.get_mut(&view.root) {
                            open.runtime.clone_from(&report.runtime);
                        }
                        view.report = Some(Arc::new(report));
                    }
                    Err(message) => view.error = Some(message),
                }
            }
        }
    }

    /// Opens the request of step `index` of the run, showing its response
    /// and checks.
    pub(super) fn open_run_step(&mut self, index: usize) {
        let Some(view) = &self.snapshot.run else {
            return;
        };
        let Some(step) = view.steps.get(index).cloned() else {
            return;
        };
        let key = TabKey::new(view.root.clone(), step.path.clone());
        let Some(responses) = self.open.get(&key.root).map(|open| open.responses.clone()) else {
            return;
        };
        self.open_request(&key);
        if self.sending.contains_key(&key) {
            return;
        }
        let response = match step.outcome {
            Ok(outcome) => ResponseState::Received(Received {
                checks: Checks {
                    captures: outcome.captures,
                    assertions: outcome.assertions,
                },
                ..received(&responses, outcome.head, outcome.body.size, outcome.body.id)
            }),
            Err(message) => ResponseState::Failed(message),
        };
        if let Some(tab) = self.tab_mut(&key) {
            tab.response = response;
        }
    }
}

/// The adapters of the collection a run uses.
struct Ports {
    repo: Arc<apifast_storage_fs::FsCollection>,
    transport: Arc<apifast_http::ReqwestTransport>,
    responses: Arc<apifast_history::FsResponseStore>,
    history: Arc<apifast_history::FsHistoryStore>,
    tokens: Arc<dyn apifast_app::TokenStore>,
    secrets: Arc<dyn SecretStore + Send>,
}

/// Runs in the background, telling the core of each step as it ends and
/// recording it in the history.
fn spawn_run(
    id: u64,
    ports: Ports,
    options: RunOptions,
    cancel: CancelToken,
    messages: tokio::sync::mpsc::WeakUnboundedSender<Message>,
) {
    tokio::spawn(async move {
        let Ports {
            repo,
            transport,
            responses,
            history,
            secrets,
            tokens,
        } = ports;
        let authorizer = apifast_http::LoopbackAuthorizer::system();
        let (clock, random) = (SystemClock, SystemRandom::default());
        let send = SendPorts {
            collections: &*repo,
            environments: &*repo,
            transport: &*transport,
            responses: &*responses,
            clock: &clock,
            random: &random,
            prompter: &NoPrompter,
            secrets: &*secrets,
            report: &|_| {},
            history: Some(&*history),
            tokens: Some(tokens.as_ref()),
            authorizer: Some(&authorizer),
        };
        let tell = |message: RunMessage| {
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::Run(id, message));
            }
        };
        let on_event = |event: RunEvent<'_>| match event {
            RunEvent::Started {
                iteration,
                index,
                total,
                path,
            } => tell(RunMessage::Started {
                iteration,
                index,
                total,
                path: path.clone(),
            }),
            RunEvent::Finished(step) => {
                // The method and URL as written, as a send records them.
                let written = (repo.request(&step.path).ok())
                    .map(|l| (l.request.method.as_str().to_owned(), l.request.url))
                    .unwrap_or_default();
                let entry = history_entry(step, written, clock.unix_millis());
                // A history that cannot be written does not stop the run.
                let entry = history.append(&entry).is_ok().then_some(entry);
                tell(RunMessage::Step(Box::new(step.clone()), entry));
            }
        };
        let result = run_collection(send, &TokioPause, &options, &cancel, &on_event)
            .await
            .map_err(|e| e.to_string());
        tell(RunMessage::Done(result));
    });
}

/// The history entry of a step: what was sent and how it went.
fn history_entry(step: &StepResult, written: (String, String), at_ms: i64) -> HistoryEntry {
    let at_ms = at_ms - i64::try_from(step.duration_ms).unwrap_or(0);
    match &step.outcome {
        Ok(outcome) => HistoryEntry::received(
            at_ms,
            step.path.clone(),
            written,
            outcome.head.clone(),
            (outcome.body.size, outcome.body.id.clone()),
            &outcome.secrets,
        ),
        Err(message) => HistoryEntry::failed(at_ms, step.path.clone(), written, message.clone()),
    }
}
