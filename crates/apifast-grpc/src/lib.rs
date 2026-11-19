//! gRPC transport of apifast (docs/07, F8). The connection is opened by
//! the HTTP transport ([`ReqwestTransport::open`]), so it has the
//! collection's TLS trust, client certificates and DNS overrides; tonic
//! speaks HTTP/2 and gRPC over it. Services are discovered by reflection,
//! or read from `.proto` files compiled by protox, without `protoc`.
//!
//! Must run inside a tokio runtime with the time driver enabled.

mod channel;
mod protos;
mod reflection;
mod services;
mod target;

use std::time::Duration;

use apifast_app::{CancelToken, GrpcTransport, RenderedRequest, TransportAuth, TransportError};
use apifast_domain::GrpcService;
use apifast_http::ReqwestTransport;
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};
use tonic::{Code, Status};

use crate::target::Target;

/// How often a call in progress checks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// The real [`GrpcTransport`].
#[derive(Debug, Clone)]
pub struct TonicTransport {
    http: ReqwestTransport,
}

impl TonicTransport {
    /// A transport whose connections are opened by `http`, with its
    /// settings; `.proto` paths are relative to its files' folder.
    pub fn new(http: ReqwestTransport) -> Self {
        Self { http }
    }

    async fn reflect(&self, request: &RenderedRequest) -> Result<Vec<GrpcService>, TransportError> {
        let target = Target::parse(request.url())?;
        let metadata = metadata(request)?;
        let channel = channel::connect(&self.http, &target).await?;
        let pool = reflection::reflect(&channel, &metadata).await?;
        Ok(services::services(&pool))
    }
}

impl GrpcTransport for TonicTransport {
    async fn describe(
        &self,
        request: &RenderedRequest,
        proto: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<Vec<GrpcService>, TransportError> {
        if let Some(proto) = proto {
            let pool = protos::compile(self.http.files_root(), proto)?;
            return Ok(services::services(&pool));
        }
        within(request, cancel, self.reflect(request)).await
    }
}

/// Runs `work` under `request`'s timeout, stopping if `cancel` asks to.
async fn within<T>(
    request: &RenderedRequest,
    cancel: &CancelToken,
    work: impl Future<Output = Result<T, TransportError>>,
) -> Result<T, TransportError> {
    if cancel.is_cancelled() {
        return Err(TransportError::Cancelled);
    }
    let timeout_ms = request.timeout_ms();
    let expired = async {
        if timeout_ms == 0 {
            std::future::pending::<()>().await;
        }
        tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
    };
    tokio::select! {
        result = work => result,
        () = expired => Err(TransportError::Timeout(timeout_ms)),
        () = cancelled(cancel) => Err(TransportError::Cancelled),
    }
}

async fn cancelled(token: &CancelToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// The request's headers as the call's metadata, names in lowercase.
///
/// # Errors
///
/// If the auth needs the transport (Digest, NTLM, AWS), or a header is not
/// valid metadata.
fn metadata(request: &RenderedRequest) -> Result<MetadataMap, TransportError> {
    if !matches!(request.auth(), TransportAuth::None) {
        return Err(TransportError::InvalidRequest(
            "Digest, NTLM and AWS Signature auth cannot be used for gRPC".into(),
        ));
    }
    let mut map = MetadataMap::new();
    for (name, value) in request.headers() {
        let invalid = || TransportError::InvalidRequest(format!("invalid metadata `{name}`"));
        let key =
            MetadataKey::from_bytes(name.to_ascii_lowercase().as_bytes()).map_err(|_| invalid())?;
        let value = MetadataValue::try_from(value.as_str()).map_err(|_| invalid())?;
        map.append(key, value);
    }
    Ok(map)
}

/// A status from the server, or from tonic, as a transport error.
fn status_error(status: &Status) -> TransportError {
    let message = status.message();
    match status.code() {
        Code::Unavailable => {
            TransportError::Connect(format!("the server is unavailable: {message}"))
        }
        Code::Cancelled => TransportError::Cancelled,
        code => TransportError::Protocol(format!("{}: {message}", code.description())),
    }
}

/// An error with its causes, as one line.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text = format!("{text}: {cause_text}");
        }
        source = cause.source();
    }
    text
}
