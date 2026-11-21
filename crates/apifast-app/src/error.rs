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
    /// The data does not pass validation; the message says why.
    #[error("{0}")]
    Validation(String),
    /// A script before the request failed: it was not sent.
    #[error("{0}")]
    Script(String),
    /// A script asked to skip the request: it was not sent.
    #[error("{0}")]
    Skipped(String),
}
