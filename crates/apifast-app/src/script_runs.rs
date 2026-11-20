//! The scripts of a send (docs/08, F9): those of the collection, the
//! request's folders and the request, in Bruno's "sandwich" order. Before
//! the request, from the outside in: collection, folders from the
//! outermost, request; after the response and for the tests, from the
//! inside out.
//!
//! Scripts before the request see it resolved (inherited headers
//! included) and unrendered, and change it before its templates are
//! rendered; the variables they set are used to render it. Scripts after
//! the response run before the captures and assertions, which see the
//! variables they set; the tests scripts run last.

use std::fmt;

use apifast_domain::{
    Body, Collection, Folder, Header, HttpRequest, Method, Origin, Resolved, ResolvedRequest,
    ResolvedVariables, ScriptBlock, ScriptKind, Variable, script_of,
};

use crate::{
    AppError, CancelToken, ConsoleLine, ResponseHead, RunnerRequest, Script, ScriptEngine,
    ScriptError, ScriptInput, ScriptRequest, ScriptResponse, ScriptVariables, TestOutcome,
    VariableChange, VariableScope, reason_phrase,
};

/// Where a script comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    /// `opencollection.yml`.
    Collection,
    /// A folder, by its name.
    Folder(String),
    /// The request.
    Request,
}

impl fmt::Display for ScriptSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collection => f.write_str("collection"),
            Self::Folder(name) => write!(f, "folder `{name}`"),
            Self::Request => f.write_str("request"),
        }
    }
}

/// A script that ran in a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRun {
    /// When it ran.
    pub kind: ScriptKind,
    /// Where it comes from.
    pub source: ScriptSource,
    /// What it wrote to the console.
    pub console: Vec<ConsoleLine>,
    /// Its tests.
    pub tests: Vec<TestOutcome>,
    /// Why it did not finish; `None` if it did.
    pub error: Option<ScriptError>,
}

/// What the scripts of a send did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptReport {
    /// The scripts that ran, in order.
    pub runs: Vec<ScriptRun>,
    /// The variables they set or deleted, in order. Pass them to
    /// [`apply_script_variables`] to keep them for the next requests.
    pub variables: Vec<VariableChange>,
    /// What they asked of a collection run, the last thing asked.
    pub runner: Option<RunnerRequest>,
}

impl ScriptReport {
    /// Their tests, in order.
    pub fn tests(&self) -> impl Iterator<Item = &TestOutcome> {
        self.runs.iter().flat_map(|run| &run.tests)
    }

    /// Whether every test passed and every script finished.
    pub fn passed(&self) -> bool {
        self.runs
            .iter()
            .all(|run| run.error.is_none() && run.tests.iter().all(|t| t.error.is_none()))
    }
}

/// Keeps the variables scripts set for the next requests, as runtime
/// values: those of `bru.setVar` and of `bru.setEnvVar` (the caller also
/// writes the latter to the environment, when they asked to `persist`).
pub fn apply_script_variables(runtime: &mut Vec<Variable>, changes: &[VariableChange]) {
    for change in changes {
        runtime.retain(|v| v.name != change.name);
        if let Some(value) = &change.value {
            runtime.push(Variable {
                name: change.name.clone(),
                value: Some(value.clone()),
                enabled: true,
                secret: false,
            });
        }
    }
}

/// The scripts of a send, between the request and its response: what
/// they are given, and what they did so far.
pub(crate) struct SendScripts {
    /// The scripts of each level, from the outermost.
    levels: Vec<(ScriptSource, Vec<ScriptBlock>)>,
    input: ScriptInput,
    pub(crate) report: ScriptReport,
}

/// The variables of each scope, as scripts read them: `variables` gives
/// the values of the secrets taken from the vault.
pub(crate) struct Scopes<'a> {
    pub(crate) collection: &'a Collection,
    pub(crate) environment: (Option<&'a str>, &'a [Variable]),
    pub(crate) runtime: &'a [Variable],
    pub(crate) variables: &'a ResolvedVariables,
}

impl SendScripts {
    /// The scripts of `request`, in `folders` and the collection of
    /// `scopes`; `None` when none has a script apifast runs.
    pub(crate) fn of(
        request: &HttpRequest,
        folders: &[Folder],
        scopes: &Scopes<'_>,
    ) -> Option<Self> {
        let mut levels = vec![(
            ScriptSource::Collection,
            scopes.collection.defaults.scripts.clone(),
        )];
        levels.extend((folders.iter()).map(|f| {
            (
                ScriptSource::Folder(f.name.clone()),
                f.defaults.scripts.clone(),
            )
        }));
        levels.push((ScriptSource::Request, request.scripts.clone()));
        let runs = [
            ScriptKind::BeforeRequest,
            ScriptKind::AfterResponse,
            ScriptKind::Tests,
        ];
        let any = (levels.iter())
            .any(|(_, scripts)| runs.iter().any(|kind| script_of(scripts, kind).is_some()));
        if !any {
            return None;
        }
        let filled = |list: &[Variable]| -> Vec<Variable> {
            (list.iter())
                .map(|v| match (&v.value, scopes.variables.get(&v.name)) {
                    (None, Some(resolved)) if v.secret => Variable {
                        value: resolved.value.clone(),
                        ..v.clone()
                    },
                    _ => v.clone(),
                })
                .collect()
        };
        let variables = ScriptVariables {
            collection_name: scopes.collection.name.clone(),
            environment_name: scopes.environment.0.map(str::to_owned),
            collection: filled(&scopes.collection.defaults.variables),
            environment: filled(scopes.environment.1),
            folders: filled(
                &(folders.iter())
                    .flat_map(|f| f.defaults.variables.iter().cloned())
                    .collect::<Vec<_>>(),
            ),
            request: filled(&request.variables),
            runtime: scopes.runtime.to_vec(),
        };
        Some(Self {
            levels,
            input: ScriptInput {
                request: ScriptRequest {
                    name: request.name.clone(),
                    ..ScriptRequest::default()
                },
                response: None,
                variables,
            },
            report: ScriptReport::default(),
        })
    }

    /// Runs the scripts before the request on `resolved`, changing it and
    /// `variables` as they ask.
    ///
    /// # Errors
    ///
    /// If one of them fails (the request is not sent), or asks to skip it.
    pub(crate) async fn before(
        &mut self,
        engine: &(dyn ScriptEngine + Sync),
        resolved: &mut ResolvedRequest,
        variables: &mut ResolvedVariables,
        cancel: &CancelToken,
    ) -> Result<(), AppError> {
        self.input.request = script_request(&self.input.request.name, resolved);
        for (source, code) in self.scripts(&ScriptKind::BeforeRequest) {
            let script = (&ScriptKind::BeforeRequest, &source, code.as_str());
            let outcome = self.run(engine, script, variables, cancel).await;
            if let Some(request) = outcome {
                apply_request(resolved, &request);
                self.input.request = request;
            }
            let last = self.report.runs.last();
            if let Some(error) = last.and_then(|run| run.error.as_ref()) {
                return Err(AppError::Script(format!(
                    "the {source} script before the request failed: {error}"
                )));
            }
        }
        if self.report.runner == Some(RunnerRequest::Skip) {
            return Err(AppError::Skipped("a script skipped the request".into()));
        }
        Ok(())
    }

    /// Runs the scripts of `kind` after the response (`head` and the text
    /// of its body, when read), changing `variables` as they ask. Their
    /// errors are in the report.
    pub(crate) async fn after(
        &mut self,
        engine: &(dyn ScriptEngine + Sync),
        kind: &ScriptKind,
        (head, body): (&ResponseHead, Option<&str>),
        variables: &mut ResolvedVariables,
        cancel: &CancelToken,
    ) {
        if self.input.response.is_none() {
            self.input.response = Some(ScriptResponse {
                status: head.status,
                status_text: reason_phrase(head.status).to_owned(),
                headers: head.headers.clone(),
                body: body.map(str::to_owned),
                time_ms: head.elapsed_ms,
                size: body.map_or(0, |b| b.len() as u64),
                url: head.url.clone(),
            });
        }
        for (source, code) in self.scripts(kind) {
            self.run(engine, (kind, &source, &code), variables, cancel)
                .await;
        }
    }

    /// The scripts of `kind`, in the order they run.
    fn scripts(&self, kind: &ScriptKind) -> Vec<(ScriptSource, String)> {
        let mut scripts: Vec<(ScriptSource, String)> = (self.levels.iter())
            .filter_map(|(source, scripts)| Some((source.clone(), script_of(scripts, kind)?)))
            .collect();
        if *kind != ScriptKind::BeforeRequest {
            scripts.reverse();
        }
        scripts
    }

    /// Runs one script, keeps what it did, and gives the request if it
    /// changed it.
    async fn run(
        &mut self,
        engine: &(dyn ScriptEngine + Sync),
        (kind, source, code): (&ScriptKind, &ScriptSource, &str),
        variables: &mut ResolvedVariables,
        cancel: &CancelToken,
    ) -> Option<ScriptRequest> {
        let name = source.to_string();
        let script = Script { name: &name, code };
        let outcome = engine.run(script, &self.input, cancel).await;
        for change in &outcome.variables {
            self.keep(change, variables);
        }
        if let (Some(body), Some(response)) = (&outcome.response_body, &mut self.input.response) {
            response.body = Some(body.clone());
        }
        if outcome.runner.is_some() {
            self.report.runner.clone_from(&outcome.runner);
        }
        self.report.variables.extend(outcome.variables);
        self.report.runs.push(ScriptRun {
            kind: kind.clone(),
            source: source.clone(),
            console: outcome.console,
            tests: outcome.tests,
            error: outcome.error,
        });
        outcome.request
    }

    /// Sets or deletes the variable of `change`, for the next scripts and
    /// the templates.
    fn keep(&mut self, change: &VariableChange, variables: &mut ResolvedVariables) {
        let list = match change.scope {
            VariableScope::Runtime => &mut self.input.variables.runtime,
            VariableScope::Environment => &mut self.input.variables.environment,
        };
        list.retain(|v| v.name != change.name);
        match &change.value {
            Some(value) => {
                list.push(Variable {
                    name: change.name.clone(),
                    value: Some(value.clone()),
                    enabled: true,
                    secret: false,
                });
                variables.capture(&change.name, value.clone());
            }
            None => variables.forget(&change.name),
        }
    }
}

/// `resolved` as a script sees it.
fn script_request(name: &str, resolved: &ResolvedRequest) -> ScriptRequest {
    ScriptRequest {
        name: name.to_owned(),
        method: resolved.method().as_str().to_owned(),
        url: resolved.url().to_owned(),
        headers: (resolved.headers().iter())
            .map(|h| (h.value.name.clone(), h.value.value.clone()))
            .collect(),
        body: body_text(resolved.body()),
        timeout_ms: Some(resolved.settings().timeout_ms.value),
    }
}

/// The text of a body a script can read and change.
fn body_text(body: &Body) -> Option<String> {
    match body {
        Body::Json(text) | Body::Text(text) | Body::Xml(text) | Body::Sparql(text) => {
            Some(text.clone())
        }
        _ => None,
    }
}

/// `resolved`, changed as `request` says. A header keeps where it came
/// from when the script kept it.
fn apply_request(resolved: &mut ResolvedRequest, request: &ScriptRequest) {
    resolved.set_method(Method::parse(&request.method));
    resolved.set_url(request.url.clone());
    let headers = (request.headers.iter())
        .map(|(name, value)| {
            let origin = (resolved.headers().iter())
                .find(|h| h.value.name.eq_ignore_ascii_case(name))
                .map_or(Origin::Request, |h| h.origin.clone());
            Resolved::new(Header::new(name.clone(), value.clone()), origin)
        })
        .collect();
    resolved.set_headers(headers);
    if body_text(resolved.body()) != request.body {
        let body = match (resolved.body(), request.body.clone()) {
            (_, None) => Body::None,
            (Body::Json(_), Some(text)) => Body::Json(text),
            (Body::Text(_), Some(text)) => Body::Text(text),
            (Body::Xml(_), Some(text)) => Body::Xml(text),
            (Body::Sparql(_), Some(text)) => Body::Sparql(text),
            (_, Some(text)) if text.trim_start().starts_with(['{', '[']) => Body::Json(text),
            (_, Some(text)) => Body::Text(text),
        };
        resolved.set_body(body);
    }
    if let Some(timeout) = request.timeout_ms
        && timeout != resolved.settings().timeout_ms.value
    {
        resolved.set_timeout_ms(timeout);
    }
}

#[cfg(test)]
mod tests {
    use apifast_domain::VariableValue;

    use super::*;

    #[test]
    fn script_variables_are_kept_and_deleted() {
        let mut runtime = vec![Variable::text("a", "1"), Variable::text("b", "2")];
        let change = |name: &str, value: Option<VariableValue>| VariableChange {
            scope: VariableScope::Runtime,
            name: name.into(),
            value,
            persist: false,
        };
        apply_script_variables(
            &mut runtime,
            &[
                change("a", None),
                change("c", Some(VariableValue::text("3"))),
            ],
        );
        assert_eq!(
            runtime,
            [Variable::text("b", "2"), Variable::text("c", "3")]
        );
    }
}
