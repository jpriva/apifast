use apifast_domain::EnvironmentError;
use apifast_template::RenderError;

/// Error from a [`CollectionRepository`](crate::CollectionRepository) or an
/// [`EnvironmentRepository`](crate::EnvironmentRepository).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RepositoryError {
    /// Nothing exists at that path.
    #[error("`{0}` does not exist")]
    NotFound(String),
    /// The file exists but cannot be understood.
    #[error("`{path}` is not valid: {message}")]
    Invalid {
        /// Path relative to the collection.
        path: String,
        /// What is wrong.
        message: String,
    },
    /// The file is valid but holds something this version cannot handle yet,
    /// such as a GraphQL request.
    #[error("`{path}`: {what} is not supported yet")]
    Unsupported {
        /// Path relative to the collection.
        path: String,
        /// What is not supported.
        what: String,
    },
    /// The file could not be read or written.
    #[error("cannot access `{path}`: {message}")]
    Io {
        /// Path relative to the collection.
        path: String,
        /// The system's message.
        message: String,
    },
}

/// Error from an [`HttpTransport`](crate::HttpTransport). Messages never
/// contain the URL, which may carry secrets in its query.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The URL cannot be sent.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// Something else in the request cannot be sent, such as a header name
    /// or a file to upload.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The host name could not be resolved.
    #[error("{0}")]
    Dns(String),
    /// The connection could not be made.
    #[error("{0}")]
    Connect(String),
    /// The TLS handshake failed, for example because of the certificate.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The server's answer is not valid HTTP, or broke off.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// More redirects than allowed.
    #[error("stopped after {0} redirects")]
    TooManyRedirects(u32),
    /// The configured timeout elapsed.
    #[error("timed out after {0} ms")]
    Timeout(u64),
    /// The send was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Any other transport failure.
    #[error("{0}")]
    Other(String),
}

/// Error from a [`ResponseStore`](crate::ResponseStore).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("response store: {0}")]
pub struct StoreError(pub String);

/// Error from a use case. Messages never contain secret values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    /// Reading or writing the collection failed.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// The environment chain is broken.
    #[error(transparent)]
    Environment(#[from] EnvironmentError),
    /// A template could not be rendered.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// Sending failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// Storing the response failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The collection's git repository failed.
    #[error(transparent)]
    Git(#[from] crate::GitError),
    /// The data does not pass validation: why.
    #[error(transparent)]
    Validation(#[from] Invalid),
    /// A script before the request failed: it was not sent.
    #[error("{0}")]
    Script(String),
    /// A script asked to skip the request: it was not sent.
    #[error("{0}")]
    Skipped(String),
}

/// Why what was asked cannot be done with the data as it is. Each variant
/// carries what its message needs, so an interface can say it in its own
/// language; `Display` says it in English.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Invalid {
    /// A new or renamed environment without a name.
    #[error("an environment needs a name")]
    EnvironmentNameEmpty,
    /// Another environment has that name.
    #[error("an environment named `{0}` already exists")]
    EnvironmentExists(String),
    /// An environment others extend cannot be deleted.
    #[error("`{name}` cannot be deleted: {} extend it", children.join(", "))]
    EnvironmentExtended {
        /// The environment.
        name: String,
        /// Those that extend it.
        children: Vec<String>,
    },
    /// A folder moved into itself or one of its folders.
    #[error("a folder cannot go inside itself")]
    FolderIntoItself,
    /// Something with that file name is already in the target folder.
    #[error("`{0}` already exists in that folder")]
    ExistsInFolder(String),
    /// A name that is empty.
    #[error("the name cannot be empty")]
    NameEmpty,
    /// A request without a name.
    #[error("the request needs a name")]
    RequestNameEmpty,
    /// The runner was given a folder that is not one.
    #[error("`{0}` is not a folder of the collection")]
    NotAFolder(String),
    /// The introspection query was answered with this status.
    #[error("the server answered {0} to the introspection query")]
    IntrospectionStatus(u16),
    /// The schema is larger than this many megabytes.
    #[error("the schema is larger than {0} MB")]
    SchemaTooLarge(u64),
    /// The introspection's answer is not JSON: why.
    #[error("the answer is not JSON: {0}")]
    SchemaNotJson(String),
    /// The introspection's answer is not a schema.
    #[error(transparent)]
    Schema(#[from] apifast_domain::SchemaError),
    /// A commit without a message.
    #[error("a commit needs a message")]
    CommitMessageEmpty,
    /// A commit of no file.
    #[error("choose the files to commit")]
    CommitNothingChosen,
    /// An AWS Signature v4 field that is empty.
    #[error("AWS Signature v4 needs the {0}")]
    AwsMissing(AwsField),
    /// An OAuth 2.0 request rendered without sending it.
    #[error("an OAuth 2.0 request is rendered with its token, when it is sent")]
    OAuth2RenderedAlone,
    /// An auth scheme this version does not know, by its name.
    #[error("auth scheme `{0}` is not supported yet")]
    AuthUnsupported(String),
    /// An auth scheme this version cannot send yet.
    #[error("this auth scheme is not supported yet")]
    AuthNotYet,
    /// A body kind this version cannot send yet.
    #[error("this body kind is not supported yet")]
    BodyNotYet,
    /// The GraphQL variables are not JSON: why.
    #[error("the GraphQL variables are not valid JSON: {0}")]
    GraphQlVariablesNotJson(String),
    /// The GraphQL variables are JSON, but not an object.
    #[error("the GraphQL variables must be a JSON object")]
    GraphQlVariablesNotObject,
    /// OAuth 2.0 without the access token URL.
    #[error("OAuth 2.0 needs the access token URL")]
    OAuth2NoTokenUrl,
    /// An OAuth 2.0 field that is empty.
    #[error("OAuth 2.0 needs the {0}")]
    OAuth2Missing(OAuth2Field),
    /// The token endpoint refused.
    #[error("the token request failed ({status}): {reason}")]
    TokenRequestFailed {
        /// Its status.
        status: u16,
        /// What it said.
        reason: TokenFailure,
    },
    /// The token response has no `access_token`.
    #[error("the token response has no access_token")]
    NoAccessToken,
    /// The token response has no `id_token`, asked for.
    #[error("the token response has no id_token")]
    NoIdToken,
    /// The implicit grant.
    #[error("the implicit grant is deprecated and not supported")]
    ImplicitGrant,
    /// The authorization code grant where no browser can be opened.
    #[error("the authorization code grant needs a browser, which is not available here")]
    NoBrowser,
    /// The authorization server refused: its error, and its description.
    #[error("the authorization was refused: {error}{}", description.as_ref().map(|d| format!(": {d}")).unwrap_or_default())]
    AuthorizationRefused {
        /// Its `error`.
        error: String,
        /// Its `error_description`.
        description: Option<String>,
    },
    /// The authorization answer is for another request.
    #[error("the authorization answer has another state; try again")]
    AuthorizationState,
    /// The authorization answer has no code.
    #[error("the authorization answer has no code")]
    AuthorizationNoCode,
    /// Nobody authorized in the browser before the wait ran out.
    #[error("nobody authorized in the browser in time")]
    AuthorizationTimedOut,
    /// A token without a refresh token.
    #[error("the token cannot be refreshed")]
    NoRefreshToken,
    /// The token expired, and fetching one is off.
    #[error("the OAuth 2.0 token has expired")]
    TokenExpired,
    /// No token, and fetching one is off.
    #[error("there is no OAuth 2.0 token, and getting one automatically is off")]
    NoToken,
    /// A token asked of a request without OAuth 2.0.
    #[error("this request does not use OAuth 2.0")]
    NotOAuth2,
    /// The callback URL cannot be used.
    #[error("the OAuth 2.0 callback URL `{url}` {problem}")]
    Callback {
        /// The URL.
        url: String,
        /// What is wrong with it.
        problem: CallbackProblem,
    },
    /// The browser could not be opened: why, and the URL to open by hand.
    #[error("cannot open the browser: {error}; open this URL: {url}")]
    BrowserFailed {
        /// Why.
        error: String,
        /// The authorization URL.
        url: String,
    },
    /// The callback could not be received: why.
    #[error("the OAuth 2.0 callback failed: {0}")]
    CallbackFailed(String),
    /// Requests that use each other's responses in a loop, by path.
    #[error("requests depend on each other: {}", .0.join(" → "))]
    DependencyCycle(Vec<String>),
    /// More requests chained with `response()` than allowed.
    #[error("too many requests chained with response()")]
    TooManyChained,
    /// A request `response()` needed failed.
    #[error("`{dependency}`, used by response(): {error}")]
    Dependency {
        /// Its path.
        dependency: String,
        /// Why it failed.
        error: Box<AppError>,
    },
    /// A request of another kind than the one asked for.
    #[error("`{path}` {kind}")]
    WrongKind {
        /// Its path.
        path: String,
        /// How it is wrong.
        kind: WrongKind,
    },
}

/// An AWS Signature v4 field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AwsField {
    /// The access key ID.
    #[error("access key id")]
    AccessKeyId,
    /// The secret access key.
    #[error("secret access key")]
    SecretAccessKey,
    /// The service.
    #[error("service")]
    Service,
    /// The region.
    #[error("region")]
    Region,
}

/// An OAuth 2.0 field of the authorization code grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OAuth2Field {
    /// The authorization URL.
    #[error("authorization URL")]
    AuthorizationUrl,
    /// The callback URL.
    #[error("callback URL")]
    CallbackUrl,
}

/// What a token endpoint said when it refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenFailure {
    /// Its `error`, and its `error_description`.
    #[error("{error}{}", description.as_ref().map(|d| format!(": {d}")).unwrap_or_default())]
    Error {
        /// Its `error`.
        error: String,
        /// Its `error_description`.
        description: Option<String>,
    },
    /// Its answer is not JSON.
    #[error("the answer is not JSON")]
    NotJson,
    /// Nothing but its status.
    #[error("status {0}")]
    Status(u16),
}

/// What is wrong with an OAuth 2.0 callback URL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallbackProblem {
    /// It is not a URL.
    #[error("is not a URL")]
    NotUrl,
    /// It is not on this machine, over HTTP.
    #[error(
        "must be http://localhost:<port>/… or http://127.0.0.1:<port>/…, so apifast can catch the redirect"
    )]
    NotLocal,
    /// Its port cannot be listened on: why.
    #[error("cannot be listened on: {0}")]
    CannotListen(String),
}

/// How a request is of another kind than the one asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WrongKind {
    /// A WebSocket request, asked to be sent.
    #[error("is a WebSocket request: it connects, it is not sent")]
    IsWebSocket,
    /// Not a WebSocket request, asked to connect.
    #[error("is not a WebSocket request")]
    NotWebSocket,
    /// A gRPC request, asked to be sent.
    #[error("is a gRPC request: it is called, not sent")]
    IsGrpc,
    /// Not a gRPC request, asked to be called.
    #[error("is not a gRPC request")]
    NotGrpc,
}
