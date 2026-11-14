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

/// Error from an [`HttpTransport`](crate::HttpTransport).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The URL cannot be sent.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// The connection could not be made (DNS, refused, TLS...).
    #[error("connection failed: {0}")]
    Connect(String),
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
    /// The data does not pass validation.
    #[error("invalid request: {0}")]
    Validation(String),
}
