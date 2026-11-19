//! OAuth 2.0 (docs/07): the token a request needs, from the cache, by
//! refreshing it, or from the token endpoint, before the request is
//! rendered. The token then goes in as a header or a query parameter,
//! through a secret variable, so it is masked wherever the request shows.

use std::fmt::Write as _;

use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, CredentialsPlacement, Json, Method, OAuth2Auth,
    OAuth2Flow, OAuth2Request, ParameterPlacement, ResolvedRequest, ResolvedVariables, Secret,
    TokenPlacement, VariableValue,
};
use apifast_template::{Host, Mode, render};

use crate::render::{RenderedBody, RenderedRequest, base64};
use crate::{AppError, BodyWriter, CancelToken, Clock, HttpTransport, StoreError, StoredBody};

/// The secret variable the token travels in.
const TOKEN_VARIABLE: &str = "__apifast_oauth2_token";

/// Largest token response read.
const RESPONSE_LIMIT: usize = 1 << 20;

/// A token too close to expiring is refreshed rather than sent.
const EXPIRY_MARGIN_MS: i64 = 5_000;

/// A token from an authorization server.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuth2Token {
    /// `access_token`.
    pub access_token: Secret,
    /// `id_token`, for `OpenID` Connect.
    pub id_token: Option<Secret>,
    /// `refresh_token`.
    pub refresh_token: Option<Secret>,
    /// `token_type`, such as `Bearer`.
    pub token_type: Option<String>,
    /// When it expires, in milliseconds since the Unix epoch; `None` when
    /// the server did not say.
    pub expires_at_ms: Option<i64>,
    /// The scope granted.
    pub scope: Option<String>,
}

/// Never shows the tokens.
impl std::fmt::Debug for OAuth2Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2Token")
            .field("token_type", &self.token_type)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("scope", &self.scope)
            .field("refreshable", &self.refresh_token.is_some())
            .finish_non_exhaustive()
    }
}

impl OAuth2Token {
    fn expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms
            .is_some_and(|at| at <= now_ms + EXPIRY_MARGIN_MS)
    }
}

/// Where tokens are kept between sends.
pub trait TokenStore: Send + Sync {
    /// The token kept under `key`.
    ///
    /// # Errors
    ///
    /// If the store cannot be read.
    fn token(&self, key: &str) -> Result<Option<OAuth2Token>, StoreError>;

    /// Keeps `token` under `key`.
    ///
    /// # Errors
    ///
    /// If the store cannot be written.
    fn save(&self, key: &str, token: &OAuth2Token) -> Result<(), StoreError>;

    /// Forgets the token under `key`.
    ///
    /// # Errors
    ///
    /// If the store cannot be written.
    fn forget(&self, key: &str) -> Result<(), StoreError>;
}

/// The fields of an OAuth 2.0 auth with their variables rendered.
struct Rendered {
    flow: OAuth2Flow,
    authorization_url: String,
    callback_url: String,
    state: String,
    pkce: bool,
    token_url: String,
    refresh_url: String,
    client_id: String,
    client_secret: String,
    basic: bool,
    username: String,
    password: String,
    scope: String,
    token_id: String,
    parameters: Vec<(OAuth2Request, ParameterPlacement, String, String)>,
}

fn render_fields(
    oauth: &OAuth2Auth,
    variables: &ResolvedVariables,
    host: &mut dyn Host,
) -> Result<Rendered, AppError> {
    let mut text = |template: &str| -> Result<String, AppError> {
        Ok(render(template, variables, host, Mode::Strict)?
            .text
            .into_owned())
    };
    let mut parameters = Vec::new();
    for p in &oauth.parameters {
        parameters.push((p.request, p.placement, text(&p.name)?, text(&p.value)?));
    }
    let token_url = text(&oauth.access_token_url)?;
    let refresh_url = text(&oauth.refresh_token_url)?;
    Ok(Rendered {
        flow: oauth.flow,
        authorization_url: text(&oauth.authorization_url)?,
        callback_url: text(&oauth.callback_url)?,
        state: text(&oauth.state)?,
        pkce: oauth.pkce,
        refresh_url: if refresh_url.trim().is_empty() {
            token_url.clone()
        } else {
            refresh_url
        },
        token_url,
        client_id: text(&oauth.client_id)?,
        client_secret: text(&oauth.client_secret)?,
        basic: oauth.credentials_placement == CredentialsPlacement::BasicAuthHeader,
        username: text(&oauth.username)?,
        password: text(&oauth.password)?,
        scope: text(&oauth.scope)?,
        token_id: oauth.token_id.clone(),
        parameters,
    })
}

impl Rendered {
    /// The cache key: the token's id, the token URL and the client.
    fn key(&self) -> String {
        format!("{}\n{}\n{}", self.token_id, self.token_url, self.client_id)
    }
}

/// Collects a response body in memory, up to [`RESPONSE_LIMIT`].
#[derive(Default)]
struct Collect(Vec<u8>);

impl BodyWriter for Collect {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        if self.0.len() + chunk.len() > RESPONSE_LIMIT {
            return Err(StoreError("the token response is too large".into()));
        }
        self.0.extend_from_slice(chunk);
        Ok(())
    }

    fn finish(self) -> Result<StoredBody, StoreError> {
        Err(StoreError("token responses are not stored".into()))
    }
}

/// Sends a token request (`fields` as a form) and reads the token.
async fn request_token<T: HttpTransport + Sync>(
    transport: &T,
    rendered: &Rendered,
    (url, request, mut fields): (&str, OAuth2Request, Vec<(String, String)>),
    now_ms: i64,
    cancel: &CancelToken,
) -> Result<OAuth2Token, AppError> {
    if url.trim().is_empty() {
        return Err(AppError::Validation(
            "OAuth 2.0 needs the access token URL".into(),
        ));
    }
    let mut url = url.to_owned();
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    let mut secrets = vec![Secret::new(rendered.client_secret.clone())];
    if rendered.basic {
        let credentials = format!("{}:{}", rendered.client_id, rendered.client_secret);
        headers.push((
            "Authorization".into(),
            format!("Basic {}", base64(credentials.as_bytes())),
        ));
    } else {
        fields.push(("client_id".into(), rendered.client_id.clone()));
        if !rendered.client_secret.is_empty() {
            fields.push(("client_secret".into(), rendered.client_secret.clone()));
        }
    }
    for (_, value) in &fields {
        secrets.push(Secret::new(value.clone()));
    }
    for (_, placement, name, value) in rendered.parameters.iter().filter(|p| p.0 == request) {
        match placement {
            ParameterPlacement::Header => headers.push((name.clone(), value.clone())),
            ParameterPlacement::Body => fields.push((name.clone(), value.clone())),
            ParameterPlacement::Query => {
                url.push(if url.contains('?') { '&' } else { '?' });
                let _ = write!(url, "{}={}", encode(name), encode(value));
            }
        }
    }
    let token_request = RenderedRequest::own(
        Method::Post,
        url,
        headers,
        RenderedBody::Form(fields),
        secrets,
    );
    let mut body = Collect::default();
    let head = transport.send(&token_request, &mut body, cancel).await?;
    read_token(head.status, &body.0, now_ms)
}

/// Percent-encodes a query component.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The token in a token endpoint's answer, or its error (RFC 6749 5.1/5.2).
fn read_token(status: u16, body: &[u8], now_ms: i64) -> Result<OAuth2Token, AppError> {
    let text = String::from_utf8_lossy(body);
    let json = Json::parse(&text).ok();
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name)).map(Json::to_text);
    if status >= 400 || json.is_none() {
        let reason = match (field("error"), field("error_description")) {
            (Some(error), Some(description)) => format!("{error}: {description}"),
            (Some(error), None) => error,
            _ if json.is_none() => "the answer is not JSON".into(),
            _ => format!("status {status}"),
        };
        return Err(AppError::Validation(format!(
            "the token request failed ({status}): {reason}"
        )));
    }
    let access_token = field("access_token")
        .ok_or_else(|| AppError::Validation("the token response has no access_token".into()))?;
    let expires_at_ms = field("expires_in")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|seconds| now_ms.saturating_add(seconds.saturating_mul(1000)));
    Ok(OAuth2Token {
        access_token: Secret::new(access_token),
        id_token: field("id_token").map(Secret::new),
        refresh_token: field("refresh_token").map(Secret::new),
        token_type: field("token_type"),
        expires_at_ms,
        scope: field("scope"),
    })
}

/// A fresh token from the token endpoint, as `rendered`'s grant asks.
async fn fetch<T: HttpTransport + Sync>(
    ports: &TokenPorts<'_, T>,
    rendered: &Rendered,
    cancel: &CancelToken,
) -> Result<OAuth2Token, AppError> {
    let (transport, now_ms) = (ports.transport, ports.clock.unix_millis());
    let mut fields = Vec::new();
    match rendered.flow {
        OAuth2Flow::ClientCredentials => {
            fields.push(("grant_type".to_owned(), "client_credentials".to_owned()));
        }
        OAuth2Flow::Password => {
            fields.push(("grant_type".into(), "password".into()));
            fields.push(("username".into(), rendered.username.clone()));
            fields.push(("password".into(), rendered.password.clone()));
        }
        OAuth2Flow::AuthorizationCode => {
            fields = authorization_code(ports, rendered, cancel).await?;
        }
        OAuth2Flow::Implicit => {
            return Err(AppError::Validation(
                "the implicit grant is deprecated and not supported".into(),
            ));
        }
    }
    if !rendered.scope.is_empty() {
        fields.push(("scope".into(), rendered.scope.clone()));
    }
    let url = rendered.token_url.clone();
    request_token(
        transport,
        rendered,
        (&url, OAuth2Request::Token, fields),
        now_ms,
        cancel,
    )
    .await
}

/// Random text of `len` characters from PKCE's alphabet (RFC 7636 4.1),
/// also good for `state`.
fn random_text(random: &dyn crate::Random, len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..len)
        .map(|_| {
            // The alphabet has 66 characters; the bias of a modulo of a
            // 64-bit number is negligible.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "below the alphabet's length"
            )]
            let i = (random.next_u64() % ALPHABET.len() as u64) as usize;
            char::from(ALPHABET[i])
        })
        .collect()
}

/// `BASE64URL(SHA256(verifier))`, without padding: PKCE's S256 challenge.
fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    base64(&Sha256::digest(verifier.as_bytes()))
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// The user authorizes in a browser (RFC 6749 4.1, with PKCE of RFC 7636):
/// the fields of the token request that exchanges the code.
async fn authorization_code<T>(
    ports: &TokenPorts<'_, T>,
    rendered: &Rendered,
    cancel: &CancelToken,
) -> Result<Vec<(String, String)>, AppError> {
    let authorizer = ports.authorizer.ok_or_else(|| {
        AppError::Validation(
            "the authorization code grant needs a browser, which is not available here".into(),
        )
    })?;
    for (name, value) in [
        ("authorization URL", &rendered.authorization_url),
        ("callback URL", &rendered.callback_url),
    ] {
        if value.trim().is_empty() {
            return Err(AppError::Validation(format!("OAuth 2.0 needs the {name}")));
        }
    }
    let state = if rendered.state.is_empty() {
        random_text(ports.random, 32)
    } else {
        rendered.state.clone()
    };
    let verifier = random_text(ports.random, 64);
    let mut url = rendered.authorization_url.clone();
    let mut query = vec![
        ("response_type", "code".to_owned()),
        ("client_id", rendered.client_id.clone()),
        ("redirect_uri", rendered.callback_url.clone()),
        ("state", state.clone()),
    ];
    if !rendered.scope.is_empty() {
        query.push(("scope", rendered.scope.clone()));
    }
    if rendered.pkce {
        query.push(("code_challenge", pkce_challenge(&verifier)));
        query.push(("code_challenge_method", "S256".into()));
    }
    let extra = rendered
        .parameters
        .iter()
        .filter(|p| p.0 == OAuth2Request::Authorization);
    for (name, value) in query
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .chain(extra.map(|p| (p.2.clone(), p.3.clone())))
    {
        url.push(if url.contains('?') { '&' } else { '?' });
        let _ = write!(url, "{}={}", encode(&name), encode(&value));
    }
    let answer = authorizer
        .authorize(&url, &rendered.callback_url, cancel)
        .await?;
    let get = |name: &str| {
        answer
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    if let Some(error) = get("error") {
        let detail = get("error_description")
            .map(|d| format!(": {d}"))
            .unwrap_or_default();
        return Err(AppError::Validation(format!(
            "the authorization was refused: {error}{detail}"
        )));
    }
    // The state ties the answer to this request: another one is an attack
    // or a stale tab.
    if get("state").as_deref() != Some(state.as_str()) {
        return Err(AppError::Validation(
            "the authorization answer has another state; try again".into(),
        ));
    }
    let code = get("code")
        .ok_or_else(|| AppError::Validation("the authorization answer has no code".into()))?;
    let mut fields = vec![
        ("grant_type".to_owned(), "authorization_code".to_owned()),
        ("code".to_owned(), code),
        ("redirect_uri".to_owned(), rendered.callback_url.clone()),
    ];
    if rendered.pkce {
        fields.push(("code_verifier".to_owned(), verifier));
    }
    Ok(fields)
}

/// A refreshed token (RFC 6749 section 6). The server may not send a new
/// refresh token; the old one is kept then.
async fn refresh<T: HttpTransport + Sync>(
    transport: &T,
    rendered: &Rendered,
    old: &OAuth2Token,
    now_ms: i64,
    cancel: &CancelToken,
) -> Result<OAuth2Token, AppError> {
    let Some(refresh_token) = &old.refresh_token else {
        return Err(AppError::Validation("the token cannot be refreshed".into()));
    };
    let mut fields = vec![
        ("grant_type".to_owned(), "refresh_token".to_owned()),
        (
            "refresh_token".to_owned(),
            refresh_token.expose().to_owned(),
        ),
    ];
    if !rendered.scope.is_empty() {
        fields.push(("scope".into(), rendered.scope.clone()));
    }
    let url = rendered.refresh_url.clone();
    let mut token = request_token(
        transport,
        rendered,
        (&url, OAuth2Request::Refresh, fields),
        now_ms,
        cancel,
    )
    .await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh_token.clone());
    }
    Ok(token)
}

/// What the OAuth 2.0 step needs from the send.
pub(crate) struct TokenPorts<'a, T> {
    pub transport: &'a T,
    pub clock: &'a dyn Clock,
    pub random: &'a dyn crate::Random,
    pub tokens: Option<&'a dyn TokenStore>,
    pub authorizer: Option<&'a dyn crate::Authorizer>,
}

impl<'a, T> TokenPorts<'a, T> {
    /// The ports of a send that the OAuth 2.0 step uses.
    pub(crate) fn of<C, E, S>(ports: &crate::SendPorts<'a, C, E, T, S>) -> Self {
        Self {
            transport: ports.transport,
            clock: ports.clock,
            random: ports.random,
            tokens: ports.tokens,
            authorizer: ports.authorizer,
        }
    }
}

/// The token for `oauth`: a cached one that has not expired, a refreshed
/// one when it has and `auto_refresh` is on, or a fetched one when
/// `auto_fetch` is on.
async fn token<T: HttpTransport + Sync>(
    ports: &TokenPorts<'_, T>,
    oauth: &OAuth2Auth,
    rendered: &Rendered,
    cancel: &CancelToken,
) -> Result<OAuth2Token, AppError> {
    let now = ports.clock.unix_millis();
    let key = rendered.key();
    let cached = match ports.tokens {
        Some(store) => store.token(&key)?,
        None => None,
    };
    let save = |token: &OAuth2Token| -> Result<(), AppError> {
        if let Some(store) = ports.tokens {
            store.save(&key, token)?;
        }
        Ok(())
    };
    if let Some(cached) = cached {
        if !cached.expired(now) {
            return Ok(cached);
        }
        if oauth.auto_refresh && cached.refresh_token.is_some() {
            match refresh(ports.transport, rendered, &cached, now, cancel).await {
                Ok(token) => {
                    save(&token)?;
                    return Ok(token);
                }
                // A refresh token can expire too: fetch a new one if allowed.
                Err(_) if oauth.auto_fetch => {}
                Err(e) => return Err(e),
            }
        } else if !oauth.auto_fetch {
            return Err(AppError::Validation(
                "the OAuth 2.0 token has expired".into(),
            ));
        }
    }
    if !oauth.auto_fetch {
        return Err(AppError::Validation(
            "there is no OAuth 2.0 token, and getting one automatically is off".into(),
        ));
    }
    let token = fetch(ports, rendered, cancel).await?;
    save(&token)?;
    Ok(token)
}

/// `resolved` with its OAuth 2.0 auth replaced by the token, as a header or
/// a query parameter; the token goes in `variables` as a secret. Requests
/// with another auth come back as they are.
///
/// # Errors
///
/// If there is no token and none can be obtained.
pub(crate) async fn with_token<T: HttpTransport + Sync>(
    ports: &TokenPorts<'_, T>,
    resolved: ResolvedRequest,
    variables: &mut ResolvedVariables,
    host: &mut (dyn Host + Send),
    cancel: &CancelToken,
) -> Result<ResolvedRequest, AppError> {
    let AuthScheme::OAuth2(oauth) = &resolved.auth().value else {
        return Ok(resolved);
    };
    let oauth = oauth.as_ref().clone();
    let rendered = render_fields(&oauth, variables, host)?;
    let token = token(ports, &oauth, &rendered, cancel).await?;
    apply_token(resolved, &oauth, &token, variables)
}

/// As [`with_token`], with only the token already stored: none is fetched
/// or refreshed. Without a valid one, the request goes as it is, and the
/// second value says so. For copying a request, which must not send.
///
/// # Errors
///
/// If the auth's fields do not render, or the store cannot be read.
pub(crate) fn with_stored_token(
    tokens: Option<&dyn TokenStore>,
    now_ms: i64,
    resolved: ResolvedRequest,
    variables: &mut ResolvedVariables,
    host: &mut (dyn Host + Send),
) -> Result<(ResolvedRequest, bool), AppError> {
    let AuthScheme::OAuth2(oauth) = &resolved.auth().value else {
        return Ok((resolved, true));
    };
    let oauth = oauth.as_ref().clone();
    let rendered = render_fields(&oauth, variables, host)?;
    let stored = match tokens {
        Some(store) => store.token(&rendered.key())?,
        None => None,
    };
    match stored.filter(|t| !t.expired(now_ms)) {
        Some(token) => Ok((apply_token(resolved, &oauth, &token, variables)?, true)),
        None => Ok((resolved.with_auth(AuthScheme::None), false)),
    }
}

/// `resolved` with `token` where `oauth` puts it, as a secret variable.
fn apply_token(
    resolved: ResolvedRequest,
    oauth: &OAuth2Auth,
    token: &OAuth2Token,
    variables: &mut ResolvedVariables,
) -> Result<ResolvedRequest, AppError> {
    let value = if oauth.token_source == "id_token" {
        token
            .id_token
            .clone()
            .ok_or_else(|| AppError::Validation("the token response has no id_token".into()))?
    } else {
        token.access_token.clone()
    };
    variables.add_secret(TOKEN_VARIABLE, VariableValue::text(value.expose()));
    let reference = format!("{{{{{TOKEN_VARIABLE}}}}}");
    let auth = match &oauth.token_placement {
        TokenPlacement::Header(prefix) if prefix.trim().is_empty() => ApiKeyAuth {
            key: "Authorization".into(),
            value: reference,
            placement: ApiKeyPlacement::Header,
        },
        TokenPlacement::Header(prefix) => ApiKeyAuth {
            key: "Authorization".into(),
            value: format!("{} {reference}", prefix.trim()),
            placement: ApiKeyPlacement::Header,
        },
        TokenPlacement::Query(key) => ApiKeyAuth {
            key: key.clone(),
            value: reference,
            placement: ApiKeyPlacement::Query,
        },
    };
    Ok(resolved.with_auth(AuthScheme::ApiKey(auth)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636, appendix B.
    #[test]
    fn pkce_challenges_match_the_rfc() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn token_answers_and_errors_are_read() {
        let token = read_token(
            200,
            br#"{"access_token":"a","token_type":"Bearer","expires_in":60,"refresh_token":"r","scope":"x"}"#,
            1_000,
        )
        .unwrap();
        assert_eq!(token.access_token.expose(), "a");
        assert_eq!(token.expires_at_ms, Some(61_000));
        assert!(token.expired(57_000) && !token.expired(50_000));
        assert!(!format!("{token:?}").contains("\"a\""));
        let error = read_token(
            400,
            br#"{"error":"invalid_client","error_description":"bad"}"#,
            0,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "the token request failed (400): invalid_client: bad");
        assert!(
            read_token(200, b"<html>", 0)
                .unwrap_err()
                .to_string()
                .contains("not JSON")
        );
        assert!(
            read_token(200, b"{}", 0)
                .unwrap_err()
                .to_string()
                .contains("no access_token")
        );
    }
}

/// What the interface shows of a stored token; never the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    /// When it expires, in milliseconds since the Unix epoch.
    pub expires_at_ms: Option<i64>,
    /// The scope granted.
    pub scope: Option<String>,
    /// `token_type`.
    pub token_type: Option<String>,
    /// Whether it has a refresh token.
    pub refreshable: bool,
}

impl From<&OAuth2Token> for TokenInfo {
    fn from(token: &OAuth2Token) -> Self {
        Self {
            expires_at_ms: token.expires_at_ms,
            scope: token.scope.clone(),
            token_type: token.token_type.clone(),
            refreshable: token.refresh_token.is_some(),
        }
    }
}

/// The OAuth 2.0 auth of the request at `path`, inherited or its own, and
/// its rendered fields; `None` when it has another auth.
fn target<C, E, T, S>(
    ports: &crate::SendPorts<'_, C, E, T, S>,
    path: &crate::RequestPath,
    options: &crate::SendOptions,
) -> Result<Option<(OAuth2Auth, Rendered)>, AppError>
where
    C: crate::CollectionRepository,
    E: crate::EnvironmentRepository,
{
    let mut loaded = ports.collections.request(path)?;
    if let Some(draft) = &options.draft {
        loaded.request = draft.clone();
    }
    let collection = ports.collections.collection()?;
    let resolved = apifast_domain::resolve(apifast_domain::Chain::new(
        &collection,
        &loaded.folders,
        &loaded.request,
    ));
    let AuthScheme::OAuth2(oauth) = &resolved.auth().value else {
        return Ok(None);
    };
    let mut variables = crate::resolve_request_variables(
        ports.collections,
        ports.environments,
        &loaded,
        options.environment.as_deref(),
        &options.runtime,
    )?;
    for (name, environment) in variables.secrets_without_value() {
        if let Some(value) = ports.secrets.secret(&environment, &name)? {
            variables.fill_secret(&name, VariableValue::text(value.expose()));
        }
    }
    let mut host = crate::AppHost::new(
        ports.clock,
        ports.random,
        ports.prompter,
        ports.environments.process_env()?,
    );
    let rendered = render_fields(oauth, &variables, &mut host)?;
    Ok(Some((oauth.as_ref().clone(), rendered)))
}

fn no_oauth2() -> AppError {
    AppError::Validation("this request does not use OAuth 2.0".into())
}

/// The token stored for the request at `path`, as the interface shows it.
///
/// # Errors
///
/// If the request cannot be read or its fields rendered, or the store
/// fails.
pub fn stored_token<C, E, T, S>(
    ports: &crate::SendPorts<'_, C, E, T, S>,
    path: &crate::RequestPath,
    options: &crate::SendOptions,
) -> Result<Option<TokenInfo>, AppError>
where
    C: crate::CollectionRepository,
    E: crate::EnvironmentRepository,
{
    let (Some((_, rendered)), Some(store)) = (target(ports, path, options)?, ports.tokens) else {
        return Ok(None);
    };
    Ok(store.token(&rendered.key())?.as_ref().map(TokenInfo::from))
}

/// Gets a new token for the request at `path` from its token endpoint,
/// whatever is stored, and stores it.
///
/// # Errors
///
/// If the request does not use OAuth 2.0, or the token request fails.
pub async fn get_new_token<C, E, T, S>(
    ports: crate::SendPorts<'_, C, E, T, S>,
    path: &crate::RequestPath,
    options: &crate::SendOptions,
    cancel: &CancelToken,
) -> Result<TokenInfo, AppError>
where
    C: crate::CollectionRepository + Sync,
    E: crate::EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
{
    let (_, rendered) = target(&ports, path, options)?.ok_or_else(no_oauth2)?;
    let token = fetch(&TokenPorts::of(&ports), &rendered, cancel).await?;
    if let Some(store) = ports.tokens {
        store.save(&rendered.key(), &token)?;
    }
    Ok(TokenInfo::from(&token))
}

/// Forgets the token stored for the request at `path`.
///
/// # Errors
///
/// If the request does not use OAuth 2.0, or the store fails.
pub fn forget_token<C, E, T, S>(
    ports: &crate::SendPorts<'_, C, E, T, S>,
    path: &crate::RequestPath,
    options: &crate::SendOptions,
) -> Result<(), AppError>
where
    C: crate::CollectionRepository,
    E: crate::EnvironmentRepository,
{
    let (_, rendered) = target(ports, path, options)?.ok_or_else(no_oauth2)?;
    if let Some(store) = ports.tokens {
        store.forget(&rendered.key())?;
    }
    Ok(())
}
