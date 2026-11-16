use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use apifast_domain::{
    Chain, HttpRequest, Origin, ResolvedRequest, ResolvedVariables, Secret, Variable,
    VariableValue, environment_chain, resolve, resolve_variables,
};
use apifast_template::{Mode, Problem};

use crate::checks::{CHECK_LIMIT, has_checks, run_checks};
use crate::functions::{Fetched, ResponseFunctions};
use crate::{
    AppError, AppHost, BodyWriter, CancelToken, Clock, CollectionRepository, CollectionTree,
    EnvironmentRepository, HttpTransport, LoadedRequest, Prompter, Random, RenderOutcome,
    RequestPath, ResponseHead, ResponseStore, SecretStore, StoredBody, render_request, sort_tree,
};
use crate::{AssertionOutcome, CaptureOutcome, ResponseFacts};
use crate::{HistoryEntry, HistoryStore, ResponseId, TransportError};

/// Loads the collection tree, sorted by `seq` and name.
///
/// # Errors
///
/// If the repository cannot read the collection.
pub fn open_collection(repo: &impl CollectionRepository) -> Result<CollectionTree, AppError> {
    let mut tree = repo.tree()?;
    sort_tree(&mut tree.items);
    Ok(tree)
}

/// Loads a request and its containing folders.
///
/// # Errors
///
/// If the request or a folder cannot be read.
pub fn load_request(
    repo: &impl CollectionRepository,
    path: &RequestPath,
) -> Result<LoadedRequest, AppError> {
    Ok(repo.request(path)?)
}

/// Validates and saves a request.
///
/// # Errors
///
/// [`AppError::Validation`] if the request has no name, or the repository
/// error if writing fails.
pub fn save_request(
    repo: &impl CollectionRepository,
    path: &RequestPath,
    request: &HttpRequest,
) -> Result<(), AppError> {
    if request.name.trim().is_empty() {
        return Err(AppError::Validation("the request needs a name".into()));
    }
    Ok(repo.save_request(path, request)?)
}

/// Builds the variables visible to a request, with their origins, following
/// the documented precedence: collection, environment (with `extends`),
/// folders, request and runtime.
///
/// # Errors
///
/// If `environment` does not exist or its `extends` chain is broken.
pub fn resolve_request_variables(
    collections: &impl CollectionRepository,
    environments: &impl EnvironmentRepository,
    loaded: &LoadedRequest,
    environment: Option<&str>,
    runtime: &[Variable],
) -> Result<ResolvedVariables, AppError> {
    let collection = collections.collection()?;
    let all = environments.environments()?;
    let env_chain = match environment {
        Some(name) => environment_chain(name, &all)?,
        None => Vec::new(),
    };
    let chain = Chain::new(&collection, &loaded.folders, &loaded.request);
    let mut layers = chain.variable_layers(&env_chain);
    layers.push((Origin::Runtime, runtime));
    Ok(resolve_variables(layers))
}

/// Ports needed by [`send_request`].
pub struct SendPorts<'a, C, E, T, S> {
    /// Where the request is read from.
    pub collections: &'a C,
    /// Environments and process variables.
    pub environments: &'a E,
    /// Network.
    pub transport: &'a T,
    /// Where the response body goes.
    pub responses: &'a S,
    /// Time for dynamic variables.
    pub clock: &'a dyn Clock,
    /// Randomness for dynamic variables.
    pub random: &'a dyn Random,
    /// Answers for `{{?Label}}`.
    pub prompter: &'a dyn Prompter,
    /// Values of the secret variables.
    pub secrets: &'a dyn SecretStore,
    /// Called with each template problem right after rendering, before
    /// sending, when `allow_unresolved` lets the send go ahead. So a driver
    /// can show the warnings even if the send then fails.
    pub report: &'a (dyn Fn(&Problem) + Sync),
    /// Where `response(...)` finds saved responses, and where the requests
    /// it sends are recorded. `None`: every one is sent when needed.
    pub history: Option<&'a (dyn HistoryStore + Sync)>,
}

/// Options for [`send_request`].
#[derive(Debug, Clone, Default)]
pub struct SendOptions {
    /// Active environment.
    pub environment: Option<String>,
    /// Values captured from previous responses.
    pub runtime: Vec<Variable>,
    /// Send even if some variables are missing (they render empty).
    pub allow_unresolved: bool,
    /// Send this instead of the request as saved: an unsaved draft. Its
    /// folders and collection still come from the repository.
    pub draft: Option<HttpRequest>,
}

/// Result of [`send_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    /// Status and headers.
    pub head: ResponseHead,
    /// Where the body was stored. The UI reads it in windows.
    pub body: StoredBody,
    /// Template problems, when `allow_unresolved` let the send go ahead.
    pub problems: Vec<Problem>,
    /// Whether the request carried a secret.
    pub contains_secret: bool,
    /// The values of the secrets it carried, to mask them in what is shown
    /// or stored (history, timeline).
    pub secrets: Vec<Secret>,
    /// The captures that ran, in order. Pass them to
    /// [`apply_captures`](crate::apply_captures) to keep their values.
    pub captures: Vec<CaptureOutcome>,
    /// The assertions checked, in order.
    pub assertions: Vec<AssertionOutcome>,
    /// The entries added to the history for the requests `response(...)`
    /// sent first, in order, so a driver can show them.
    pub dependencies: Vec<HistoryEntry>,
}

/// How many rounds of `response(...)` dependencies one render may need.
const MAX_ROUNDS: usize = 16;

/// What the requests of one send share: the requests on the way to the
/// current one, to refuse cycles, and the dependencies already sent, so
/// each is sent once however many times it is used.
#[derive(Default)]
struct ChainContext {
    chain: Vec<RequestPath>,
    fetched: BTreeMap<RequestPath, Arc<Fetched>>,
    /// History entries written for dependencies.
    recorded: Vec<HistoryEntry>,
}

/// Loads a request, resolves inheritance and variables, renders it, sends it
/// and stores the response body. Then runs its captures and assertions.
///
/// A request that uses `{{ response(request='…') }}` or `header(…)` sends
/// that request first when its saved response does not do (`refresh`).
/// Those dependencies are recorded in `history`, when given.
///
/// # Errors
///
/// If any step fails: loading, the environment chain, rendering (a missing
/// variable, unless `allow_unresolved`), a dependency, a cycle between
/// requests, the transport or the response store.
pub async fn send_request<C, E, T, S>(
    ports: SendPorts<'_, C, E, T, S>,
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
) -> Result<SendOutcome, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    let mut context = ChainContext::default();
    let mut outcome = send_in_chain(&ports, path, options, cancel, &mut context).await?;
    outcome.dependencies = context.recorded;
    Ok(outcome)
}

/// [`send_request`] for one request of a chain; boxed, since it calls
/// itself for the dependencies.
fn send_in_chain<'s, 'p: 's, C, E, T, S>(
    ports: &'s SendPorts<'p, C, E, T, S>,
    path: &'s RequestPath,
    options: &'s SendOptions,
    cancel: &'s CancelToken,
    context: &'s mut ChainContext,
) -> Pin<Box<dyn Future<Output = Result<SendOutcome, AppError>> + Send + 's>>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    Box::pin(async move {
        if context.chain.contains(path) {
            let mut names: Vec<String> = context.chain.iter().map(ToString::to_string).collect();
            names.push(path.to_string());
            return Err(AppError::Validation(format!(
                "requests depend on each other: {}",
                names.join(" → ")
            )));
        }
        context.chain.push(path.clone());
        let result = send_one(ports, path, options, cancel, context).await;
        context.chain.pop();
        result
    })
}

async fn send_one<C, E, T, S>(
    ports: &SendPorts<'_, C, E, T, S>,
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
    context: &mut ChainContext,
) -> Result<SendOutcome, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    let mut loaded = ports.collections.request(path)?;
    if let Some(draft) = &options.draft {
        loaded.request = draft.clone();
    }
    let collection = ports.collections.collection()?;
    let mut variables = resolve_request_variables(
        ports.collections,
        ports.environments,
        &loaded,
        options.environment.as_deref(),
        &options.runtime,
    )?;
    // Secrets come from the vault only now, for this send.
    for (name, environment) in variables.secrets_without_value() {
        if let Some(value) = ports.secrets.secret(&environment, &name)? {
            variables.fill_secret(&name, VariableValue::text(value.expose()));
        }
    }
    let resolved = resolve(Chain::new(&collection, &loaded.folders, &loaded.request));

    let read = |id: &ResponseId, len: usize| ports.responses.read(id, 0, len);
    let mut host = AppHost::new(
        ports.clock,
        ports.random,
        ports.prompter,
        ports.environments.process_env()?,
    )
    .with_functions(ResponseFunctions::new(
        ports.history,
        &read,
        ports.clock.unix_millis(),
    ));
    let mode = if options.allow_unresolved {
        Mode::Lenient
    } else {
        Mode::Strict
    };
    let rendered = render_with_dependencies(
        ports,
        (&resolved, &variables, &mut host),
        mode,
        options,
        cancel,
        context,
    )
    .await?;
    rendered.problems.iter().for_each(ports.report);

    let at_ms = ports.clock.unix_millis();
    let mut writer = ports.responses.create()?;
    let head = ports
        .transport
        .send(&rendered.request, &mut writer, cancel)
        .await?;
    let body = writer.finish()?;

    let (captures, assertions) = checks_after(
        ports.responses,
        &loaded.request,
        (&head, &body),
        &mut variables,
        &mut host,
        rendered.request.secrets(),
    )?;

    // A dependency goes to the history here; the request the driver asked
    // for is recorded by the driver.
    if context.chain.len() > 1
        && let Some(history) = ports.history
    {
        let written = (
            loaded.request.method.as_str().to_owned(),
            loaded.request.url.clone(),
        );
        let entry = HistoryEntry::received(
            at_ms,
            path.clone(),
            written,
            head.clone(),
            (body.size, body.id.clone()),
            rendered.request.secrets(),
        );
        // A history that cannot be written does not stop the send.
        if history.append(&entry).is_ok() {
            context.recorded.push(entry);
        }
    }

    Ok(SendOutcome {
        head,
        body,
        problems: rendered.problems,
        contains_secret: rendered.request.contains_secret(),
        secrets: rendered.request.secrets().to_vec(),
        captures,
        assertions,
        dependencies: Vec::new(),
    })
}

/// The captures and assertions of `request`, if it has any: its body is
/// read back from the store (up to [`CHECK_LIMIT`]) only then.
fn checks_after(
    responses: &impl ResponseStore,
    request: &HttpRequest,
    (head, body): (&ResponseHead, &StoredBody),
    variables: &mut ResolvedVariables,
    host: &mut AppHost<'_>,
    secrets: &[Secret],
) -> Result<(Vec<CaptureOutcome>, Vec<AssertionOutcome>), AppError> {
    if !has_checks(request) {
        return Ok((Vec::new(), Vec::new()));
    }
    let bytes = if body.size <= CHECK_LIMIT {
        let len = usize::try_from(body.size).unwrap_or(usize::MAX);
        Some(responses.read(&body.id, 0, len)?)
    } else {
        None
    };
    let facts = ResponseFacts::new(head, body.size, bytes.as_deref());
    Ok(run_checks(request, &facts, variables, host, secrets))
}

/// Renders the request, sending first the requests its `response(...)`
/// calls need, in rounds, until nothing more is needed.
async fn render_with_dependencies<C, E, T, S>(
    ports: &SendPorts<'_, C, E, T, S>,
    (resolved, variables, host): (&ResolvedRequest, &ResolvedVariables, &mut AppHost<'_>),
    mode: Mode,
    options: &SendOptions,
    cancel: &CancelToken,
    context: &mut ChainContext,
) -> Result<RenderOutcome, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    for _ in 0..=MAX_ROUNDS {
        let rendered = render_request(resolved, variables, host, mode);
        let needed = host
            .functions()
            .map(|f| std::mem::take(&mut f.needed))
            .unwrap_or_default();
        if needed.is_empty() {
            return rendered;
        }
        for dependency in needed {
            let fetched = if let Some(fetched) = context.fetched.get(&dependency) {
                fetched.clone()
            } else {
                let fetched = send_dependency(ports, &dependency, options, cancel, context).await?;
                context.fetched.insert(dependency.clone(), fetched.clone());
                fetched
            };
            if let Some(functions) = host.functions() {
                functions.add(dependency, fetched);
            }
        }
    }
    Err(AppError::Validation(
        "too many requests chained with response()".into(),
    ))
}

/// Sends `dependency` for a `response(...)` of the current request, with
/// the same environment and runtime values, and reads its response.
async fn send_dependency<C, E, T, S>(
    ports: &SendPorts<'_, C, E, T, S>,
    dependency: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
    context: &mut ChainContext,
) -> Result<Arc<Fetched>, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    let dependency_options = SendOptions {
        environment: options.environment.clone(),
        runtime: options.runtime.clone(),
        allow_unresolved: options.allow_unresolved,
        draft: None,
    };
    let outcome = send_in_chain(ports, dependency, &dependency_options, cancel, context)
        .await
        .map_err(|error| match error {
            AppError::Transport(TransportError::Cancelled) => error,
            AppError::Validation(message) if message.starts_with("requests depend") => {
                AppError::Validation(message)
            }
            other => AppError::Validation(format!("`{dependency}`, used by response(): {other}")),
        })?;
    let read = |id: &ResponseId, len: usize| ports.responses.read(id, 0, len);
    Ok(Arc::new(Fetched::sent(
        ports.clock.unix_millis(),
        &outcome,
        &read,
    )?))
}
