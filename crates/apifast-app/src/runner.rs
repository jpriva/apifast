//! The collection runner (docs/08): sends the requests of a collection or a
//! folder in order, carrying captured values from one to the next, and
//! gathers their results.

use apifast_domain::{RequestKind, Variable};

use crate::ports::is_within;
use crate::use_cases::send_request;
use crate::{
    AppError, CancelToken, CollectionRepository, EnvironmentRepository, HttpTransport, Pause,
    RequestPath, ResponseStore, RunnerRequest, SendOptions, SendOutcome, SendPorts, TreeItem,
    apply_captures, apply_script_variables, keep_script_changes, sort_tree,
};

/// What to run and how.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The folder to run, relative to the collection; `None` runs the whole
    /// collection.
    pub folder: Option<String>,
    /// Whether the requests in subfolders run too.
    pub recursive: bool,
    /// Only requests with at least one of these tags run, when not empty.
    pub tags: Vec<String>,
    /// Requests with any of these tags do not run.
    pub exclude_tags: Vec<String>,
    /// Pause between two requests, in milliseconds.
    pub delay_ms: u64,
    /// Stop at the first request that fails: an assertion that does not
    /// hold, or an error.
    pub bail: bool,
    /// How many times the whole selection runs; at least 1. Ignored when
    /// there is `data`.
    pub iterations: u32,
    /// Rows of a data file ([`parse_data`](crate::parse_data)): the
    /// selection runs once per row, with the row's columns as runtime
    /// variables.
    pub data: Vec<Vec<Variable>>,
    /// The environment, the starting runtime values and whether unresolved
    /// variables are allowed. Its `draft` is ignored.
    pub send: SendOptions,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            folder: None,
            recursive: true,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            delay_ms: 0,
            bail: false,
            iterations: 1,
            data: Vec::new(),
            send: SendOptions::default(),
        }
    }
}

/// A request of the selection that does not run, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Its file.
    pub path: RequestPath,
    /// Its name, or its file name when it cannot be read.
    pub name: String,
    /// Why it does not run.
    pub reason: String,
}

/// The result of one request in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// Which repetition, from 1.
    pub iteration: u32,
    /// Its file.
    pub path: RequestPath,
    /// Its name.
    pub name: String,
    /// The response and its checks, or why there is none.
    pub outcome: Result<SendOutcome, String>,
    /// From start to end, checks included, in milliseconds.
    pub duration_ms: u64,
}

impl StepResult {
    /// Whether it was sent, all its assertions held, and its scripts
    /// finished with their tests passing.
    pub fn passed(&self) -> bool {
        self.outcome
            .as_ref()
            .is_ok_and(|o| o.assertions.iter().all(|a| a.verdict.passed) && o.scripts.passed())
    }
}

/// What happens during a run, as it happens.
#[derive(Debug)]
pub enum RunEvent<'a> {
    /// A request is about to be sent.
    Started {
        /// Which repetition, from 1.
        iteration: u32,
        /// Its place in the selection, from 0.
        index: usize,
        /// How many requests the selection runs.
        total: usize,
        /// Its file.
        path: &'a RequestPath,
    },
    /// A request finished.
    Finished(&'a StepResult),
}

/// Totals of a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSummary {
    /// Requests sent or attempted, across iterations.
    pub requests: usize,
    /// Of those, sent with all their assertions holding.
    pub passed: usize,
    /// Sent, with at least one assertion that does not hold.
    pub failed: usize,
    /// Not sent because of an error.
    pub errors: usize,
    /// Requests of the selection that do not run.
    pub skipped: usize,
    /// Assertions that held.
    pub assertions_passed: usize,
    /// Assertions that did not.
    pub assertions_failed: usize,
    /// Script tests that passed.
    pub tests_passed: usize,
    /// Script tests that failed.
    pub tests_failed: usize,
}

/// The result of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// Each request sent or attempted, in order.
    pub steps: Vec<StepResult>,
    /// Requests of the selection that do not run.
    pub skipped: Vec<Skipped>,
    /// From start to end, in milliseconds.
    pub duration_ms: u64,
    /// Whether it stopped early because of `bail`.
    pub bailed: bool,
    /// Whether it stopped early because it was cancelled.
    pub cancelled: bool,
    /// Whether a script stopped it (`bru.runner.stopExecution()`).
    pub stopped: bool,
    /// The runtime values at the end, with everything captured.
    pub runtime: Vec<Variable>,
    /// What scripts changed in the environment, the collection and the
    /// globals, to carry on ([`keep_script_changes`]).
    pub script_changes: Vec<crate::VariableChange>,
}

impl RunReport {
    /// Its totals.
    pub fn summary(&self) -> RunSummary {
        let mut summary = RunSummary {
            requests: self.steps.len(),
            skipped: self.skipped.len(),
            ..RunSummary::default()
        };
        for step in &self.steps {
            match &step.outcome {
                Err(_) => summary.errors += 1,
                Ok(outcome) => {
                    let failed = outcome
                        .assertions
                        .iter()
                        .filter(|a| !a.verdict.passed)
                        .count();
                    summary.assertions_failed += failed;
                    summary.assertions_passed += outcome.assertions.len() - failed;
                    let tests = outcome.scripts.tests().count();
                    let tests_failed = (outcome.scripts.tests())
                        .filter(|t| t.error.is_some())
                        .count();
                    summary.tests_failed += tests_failed;
                    summary.tests_passed += tests - tests_failed;
                    if failed == 0 && outcome.scripts.passed() {
                        summary.passed += 1;
                    } else {
                        summary.failed += 1;
                    }
                }
            }
        }
        summary
    }

    /// Whether every request was sent and every assertion held.
    pub fn succeeded(&self) -> bool {
        !self.cancelled && self.steps.iter().all(StepResult::passed)
    }
}

/// The requests a run considers, before filtering by tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Requests to run, with their names, in order.
    pub runnable: Vec<(RequestPath, String)>,
    /// Requests that cannot run.
    pub skipped: Vec<Skipped>,
}

/// The requests a run would consider, in the order of the tree: by `seq`,
/// then by name, each folder's requests where the folder is. `Other` and
/// unreadable files come back as skipped.
///
/// # Errors
///
/// If `folder` is not a folder of the tree.
pub fn run_selection(
    items: &[TreeItem],
    folder: Option<&str>,
    recursive: bool,
) -> Result<Selection, AppError> {
    let folder = folder
        .map(|f| f.trim_matches('/'))
        .filter(|f| !f.is_empty());
    let start = match folder {
        None => items,
        Some(path) => find_folder(items, path).ok_or_else(|| {
            AppError::Validation(format!("`{path}` is not a folder of the collection"))
        })?,
    };
    let mut runnable = Vec::new();
    let mut skipped = Vec::new();
    collect(start, recursive, &mut runnable, &mut skipped);
    Ok(Selection { runnable, skipped })
}

fn find_folder<'a>(items: &'a [TreeItem], path: &str) -> Option<&'a [TreeItem]> {
    items.iter().find_map(|item| match item {
        TreeItem::Folder {
            path: p, children, ..
        } if p == path => Some(children.as_slice()),
        TreeItem::Folder {
            path: p, children, ..
        } if is_within(path, p) => find_folder(children, path),
        _ => None,
    })
}

fn collect(
    items: &[TreeItem],
    recursive: bool,
    runnable: &mut Vec<(RequestPath, String)>,
    skipped: &mut Vec<Skipped>,
) {
    for item in items {
        match item {
            TreeItem::Folder { children, .. } => {
                if recursive {
                    collect(children, recursive, runnable, skipped);
                }
            }
            TreeItem::Request {
                path,
                name,
                kind: RequestKind::WebSocket,
                ..
            } => skipped.push(Skipped {
                path: path.clone(),
                name: name.clone(),
                reason: "websocket requests do not run in a run".into(),
            }),
            TreeItem::Request {
                path,
                name,
                kind: RequestKind::Grpc,
                ..
            } => skipped.push(Skipped {
                path: path.clone(),
                name: name.clone(),
                reason: "grpc requests cannot run yet".into(),
            }),
            TreeItem::Request { path, name, .. } => runnable.push((path.clone(), name.clone())),
            TreeItem::Other {
                path, name, kind, ..
            } => skipped.push(Skipped {
                path: path.clone(),
                name: name.clone(),
                reason: format!("{kind} requests cannot run yet"),
            }),
            TreeItem::Invalid { path, message } => skipped.push(Skipped {
                path: path.clone(),
                name: path
                    .as_str()
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                reason: message.clone(),
            }),
        }
    }
}

/// Whether the tags of a request let it run.
fn tags_allow(tags: &[String], include: &[String], exclude: &[String]) -> Result<(), String> {
    let has = |tag: &String| tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
    if let Some(tag) = exclude.iter().find(|t| has(t)) {
        return Err(format!("excluded by the tag `{tag}`"));
    }
    if !include.is_empty() && !include.iter().any(has) {
        return Err(format!("has none of the tags {}", include.join(", ")));
    }
    Ok(())
}

/// Runs the requests of a collection or a folder (see [`RunOptions`]).
/// Each request sees what the previous ones captured. A request that fails
/// to send is recorded and the run goes on, unless `bail` is set.
/// `on_event` is told of each request as it starts and finishes.
///
/// # Errors
///
/// If the collection tree cannot be read or the folder does not exist.
/// Errors of single requests are part of the report.
pub async fn run_collection<C, E, T, S>(
    ports: SendPorts<'_, C, E, T, S>,
    pause: &dyn Pause,
    options: &RunOptions,
    cancel: &CancelToken,
    on_event: &(dyn Fn(RunEvent<'_>) + Sync),
) -> Result<RunReport, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    let started = ports.clock.unix_millis();
    let Selection {
        runnable: selection,
        skipped,
    } = select(ports.collections, options)?;

    let mut send = options.send.clone();
    send.in_run = true;
    send.draft = None;
    let mut report = RunReport {
        steps: Vec::new(),
        skipped,
        duration_ms: 0,
        bailed: false,
        cancelled: false,
        stopped: false,
        runtime: Vec::new(),
        script_changes: Vec::new(),
    };
    let total = selection.len();
    let iterations = if options.data.is_empty() {
        options.iterations.max(1)
    } else {
        u32::try_from(options.data.len()).unwrap_or(u32::MAX)
    };
    'run: for iteration in 1..=iterations {
        if let Some(row) = options.data.get(iteration as usize - 1) {
            set_row(&mut send.runtime, row);
        }
        let mut index = 0;
        while let Some((path, name)) = selection.get(index) {
            if !report.steps.is_empty() && options.delay_ms > 0 {
                pause.pause(options.delay_ms).await;
            }
            if cancel.is_cancelled() {
                report.cancelled = true;
                break 'run;
            }
            on_event(RunEvent::Started {
                iteration,
                index,
                total,
                path,
            });
            let at = ports.clock.unix_millis();
            let outcome = send_request(ports, path, &send, cancel).await;
            let asked = match &outcome {
                Ok(outcome) => {
                    apply_captures(&mut send.runtime, &outcome.captures);
                    apply_script_variables(&mut send.runtime, &outcome.scripts.variables);
                    keep_script_changes(&mut send.script_changes, &outcome.scripts.variables);
                    outcome.scripts.runner.clone()
                }
                Err(AppError::Skipped(reason)) => {
                    report.skipped.push(Skipped {
                        path: path.clone(),
                        name: name.clone(),
                        reason: reason.clone(),
                    });
                    index += 1;
                    continue;
                }
                Err(_) => None,
            };
            let step = StepResult {
                iteration,
                path: path.clone(),
                name: name.clone(),
                outcome: outcome.map_err(|e| e.to_string()),
                duration_ms: elapsed(at, ports.clock.unix_millis()),
            };
            on_event(RunEvent::Finished(&step));
            let failed = !step.passed();
            let cancelled = step.outcome.is_err() && cancel.is_cancelled();
            report.steps.push(step);
            if cancelled {
                report.cancelled = true;
                break 'run;
            }
            if failed && options.bail {
                report.bailed = true;
                break 'run;
            }
            index = match asked {
                Some(RunnerRequest::Stop) => {
                    report.stopped = true;
                    break 'run;
                }
                // To the request of that name; to the end when there is none.
                Some(RunnerRequest::Next(next)) => (selection.iter())
                    .position(|(_, name)| *name == next)
                    .unwrap_or(selection.len()),
                _ => index + 1,
            };
        }
    }
    report.runtime = send.runtime;
    report.script_changes = send.script_changes;
    report.duration_ms = elapsed(started, ports.clock.unix_millis());
    Ok(report)
}

/// The requests of the run, with their names, and those that do not run,
/// tags applied.
fn select(
    collections: &impl CollectionRepository,
    options: &RunOptions,
) -> Result<Selection, AppError> {
    let mut tree = collections.tree()?;
    // Repositories list files as they find them; a run follows `seq`.
    sort_tree(&mut tree.items);
    let Selection {
        runnable: candidates,
        mut skipped,
    } = run_selection(&tree.items, options.folder.as_deref(), options.recursive)?;
    let mut selection = Vec::new();
    for (path, name) in candidates {
        let reason = match collections.request(&path) {
            Ok(loaded) => {
                let tags = apifast_domain::effective_tags(&loaded.request.tags, &loaded.folders);
                tags_allow(&tags, &options.tags, &options.exclude_tags).err()
            }
            Err(e) => Some(e.to_string()),
        };
        match reason {
            Some(reason) => skipped.push(Skipped { path, name, reason }),
            None => selection.push((path, name)),
        }
    }
    Ok(Selection {
        runnable: selection,
        skipped,
    })
}

/// Puts the columns of a data row in the runtime values, replacing what
/// earlier rows or captures left under the same names.
fn set_row(runtime: &mut Vec<Variable>, row: &[Variable]) {
    for column in row {
        runtime.retain(|v| v.name != column.name);
        runtime.push(column.clone());
    }
}

fn elapsed(from: i64, to: i64) -> u64 {
    u64::try_from(to.saturating_sub(from)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_include_any_and_exclude_any() {
        let tags: Vec<String> = vec!["smoke".into(), "Slow".into()];
        let list = |t: &[&str]| t.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(tags_allow(&tags, &[], &[]).is_ok());
        assert!(tags_allow(&tags, &list(&["smoke", "auth"]), &[]).is_ok());
        assert!(tags_allow(&tags, &list(&["auth"]), &[]).is_err());
        assert_eq!(
            tags_allow(&tags, &list(&["smoke"]), &list(&["slow"])),
            Err("excluded by the tag `slow`".into())
        );
    }
}
