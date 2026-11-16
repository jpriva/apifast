//! The collection runner (docs/08): sends the requests of a collection or a
//! folder in order, carrying captured values from one to the next, and
//! gathers their results.

use apifast_domain::{HttpRequest, Variable};

use crate::ports::is_within;
use crate::use_cases::send_request;
use crate::{
    AppError, CancelToken, CollectionRepository, EnvironmentRepository, HttpTransport, Pause,
    RequestPath, ResponseStore, SendOptions, SendOutcome, SendPorts, TreeItem, apply_captures,
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
    /// How many times the whole selection runs; at least 1.
    pub iterations: u32,
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
    /// Whether it was sent and all its assertions held.
    pub fn passed(&self) -> bool {
        self.outcome
            .as_ref()
            .is_ok_and(|o| o.assertions.iter().all(|a| a.verdict.passed))
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
    /// The runtime values at the end, with everything captured.
    pub runtime: Vec<Variable>,
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
                    if failed == 0 {
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
fn tags_allow(request: &HttpRequest, include: &[String], exclude: &[String]) -> Result<(), String> {
    let has = |tag: &String| request.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
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
    let tree = ports.collections.tree()?;
    let Selection {
        runnable: candidates,
        mut skipped,
    } = run_selection(&tree.items, options.folder.as_deref(), options.recursive)?;
    let mut selection = Vec::new();
    for (path, name) in candidates {
        let reason = match ports.collections.request(&path) {
            Ok(loaded) => tags_allow(&loaded.request, &options.tags, &options.exclude_tags).err(),
            Err(e) => Some(e.to_string()),
        };
        match reason {
            Some(reason) => skipped.push(Skipped { path, name, reason }),
            None => selection.push((path, name)),
        }
    }

    let mut send = options.send.clone();
    send.draft = None;
    let mut report = RunReport {
        steps: Vec::new(),
        skipped,
        duration_ms: 0,
        bailed: false,
        cancelled: false,
        runtime: Vec::new(),
    };
    let total = selection.len();
    'run: for iteration in 1..=options.iterations.max(1) {
        for (index, (path, name)) in selection.iter().enumerate() {
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
            if let Ok(outcome) = &outcome {
                apply_captures(&mut send.runtime, &outcome.captures);
            }
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
        }
    }
    report.runtime = send.runtime;
    report.duration_ms = elapsed(started, ports.clock.unix_millis());
    Ok(report)
}

fn elapsed(from: i64, to: i64) -> u64 {
    u64::try_from(to.saturating_sub(from)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_include_any_and_exclude_any() {
        let mut request = HttpRequest::new("a", apifast_domain::Method::Get, "http://x");
        request.tags = vec!["smoke".into(), "Slow".into()];
        let list = |t: &[&str]| t.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(tags_allow(&request, &[], &[]).is_ok());
        assert!(tags_allow(&request, &list(&["smoke", "auth"]), &[]).is_ok());
        assert!(tags_allow(&request, &list(&["auth"]), &[]).is_err());
        assert_eq!(
            tags_allow(&request, &list(&["smoke"]), &list(&["slow"])),
            Err("excluded by the tag `slow`".into())
        );
    }
}
