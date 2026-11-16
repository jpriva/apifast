use apifast_domain::{
    Chain, HttpRequest, Origin, ResolvedVariables, Secret, Variable, VariableValue,
    environment_chain, resolve, resolve_variables,
};
use apifast_template::{Mode, Problem};

use crate::{
    AppError, AppHost, BodyWriter, CancelToken, Clock, CollectionRepository, CollectionTree,
    EnvironmentRepository, HttpTransport, LoadedRequest, Prompter, Random, RequestPath,
    ResponseHead, ResponseStore, SecretStore, StoredBody, render_request, sort_tree,
};

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
}

/// Loads a request, resolves inheritance and variables, renders it, sends it
/// and stores the response body.
///
/// # Errors
///
/// If any step fails: loading, the environment chain, rendering (a missing
/// variable, unless `allow_unresolved`), the transport or the response store.
pub async fn send_request<C, E, T, S>(
    ports: SendPorts<'_, C, E, T, S>,
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
) -> Result<SendOutcome, AppError>
where
    C: CollectionRepository,
    E: EnvironmentRepository,
    T: HttpTransport,
    S: ResponseStore,
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

    let mut host = AppHost::new(
        ports.clock,
        ports.random,
        ports.prompter,
        ports.environments.process_env()?,
    );
    let mode = if options.allow_unresolved {
        Mode::Lenient
    } else {
        Mode::Strict
    };
    let rendered = render_request(&resolved, &variables, &mut host, mode)?;
    rendered.problems.iter().for_each(ports.report);

    let mut writer = ports.responses.create()?;
    let head = ports
        .transport
        .send(&rendered.request, &mut writer, cancel)
        .await?;
    let body = writer.finish()?;

    Ok(SendOutcome {
        head,
        body,
        problems: rendered.problems,
        contains_secret: rendered.request.contains_secret(),
        secrets: rendered.request.secrets().to_vec(),
    })
}
