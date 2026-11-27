//! A request rendered to copy it (docs/12, F6: "Copiar como cURL"): what a
//! send would send, but without sending anything. Nothing goes to the
//! network: an OAuth 2.0 token is the one stored, if still valid, and a
//! `response(...)` that needs a send renders empty. What is missing is
//! rendered empty and reported, instead of failing.

use apifast_domain::{Chain, VariableValue, resolve};
use apifast_template::{Mode, Problem};

use crate::functions::ResponseFunctions;
use crate::{
    AppError, AppHost, CollectionRepository, EnvironmentRepository, RenderedRequest, RequestPath,
    ResponseId, ResponseStore, SendOptions, SendPorts, render_request, resolve_request_variables,
};

/// A request rendered without sending, and what it could not fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedRequest {
    /// The request, as a send would send it.
    pub request: RenderedRequest,
    /// The templates that rendered empty.
    pub problems: Vec<Problem>,
    /// The request uses OAuth 2.0 and no valid token is stored: it goes
    /// without one.
    pub missing_token: bool,
}

/// The request at `path`, with `options`, rendered as a send would render
/// it, without sending anything.
///
/// # Errors
///
/// If the request or its variables cannot be read, or a secret cannot be
/// decrypted.
pub fn render_for_copy<C, E, T, S>(
    ports: &SendPorts<'_, C, E, T, S>,
    path: &RequestPath,
    options: &SendOptions,
) -> Result<CopiedRequest, AppError>
where
    C: CollectionRepository,
    E: EnvironmentRepository,
    S: ResponseStore + Sync,
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
        (options.environment.as_deref(), &options.globals),
        &options.runtime,
    )?;
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
    let (resolved, has_token) = crate::oauth2::with_stored_token(
        ports.tokens,
        ports.clock.unix_millis(),
        resolved,
        &mut variables,
        &mut host,
    )?;
    let outcome = render_request(&resolved, &variables, &mut host, Mode::Lenient)?;
    Ok(CopiedRequest {
        request: outcome.request,
        problems: outcome.problems,
        missing_token: !has_token,
    })
}
