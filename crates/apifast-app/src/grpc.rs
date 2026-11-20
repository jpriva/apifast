//! gRPC (docs/07, F8): a gRPC request is rendered like any other
//! (inheritance, templates, auth), its headers being the call's metadata;
//! a [`GrpcTransport`] then asks its server what it offers, or reads the
//! `.proto` file that describes it.

use std::future::Future;

use apifast_domain::{Body, GrpcCall, GrpcCatalog, HttpRequest, RequestKind};

use crate::use_cases::{ChainContext, Prepared, host_for, prepare};
use crate::websocket::{Exchange, exchange};
use crate::{
    AppError, CancelToken, CollectionRepository, EnvironmentRepository, HttpTransport,
    RenderedRequest, RequestPath, ResponseHead, ResponseId, ResponseStore, SendOptions,
    SendOutcome, SendPorts, TransportError, WsHandler, WsOutbox,
};

/// Talks to gRPC servers.
pub trait GrpcTransport {
    /// The services the server of `request` offers, and the fields of the
    /// messages they take, as reflection says; or, with `proto` (relative
    /// to the collection's folder), as that file describes them, without
    /// asking the server. The request's headers go as metadata.
    ///
    /// # Errors
    ///
    /// If the server cannot be reached or does not offer reflection, or
    /// the `.proto` file cannot be read or compiled.
    fn describe(
        &self,
        request: &RenderedRequest,
        proto: Option<&str>,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<GrpcCatalog, TransportError>> + Send;

    /// Calls `call.method` on the server of `request`, its messages
    /// described by `call.proto_path` or else by reflection; the request's
    /// headers go as metadata.
    ///
    /// `handler` gives the messages to send, as JSON: only the first for a
    /// method whose client does not stream; else all until it gives
    /// `None`, which ends the client's stream. It is told each message
    /// sent and received (as JSON), one that could not be encoded, and how
    /// the call ended: closed by the server with the gRPC status code and
    /// its name and message (`OK`, `NOT_FOUND: …`), or by the client when
    /// `cancel` asks to. A call that ends in an error status is no error:
    /// its log says so.
    ///
    /// Returns the response's head: HTTP status 200, its headers the
    /// response's metadata and trailers (with `content-type:
    /// application/grpc`), its time that of the whole call.
    ///
    /// # Errors
    ///
    /// If the server cannot be reached or described, the method is not
    /// one of its methods, or there is no message to send to a method
    /// whose client does not stream.
    fn call<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        call: &GrpcCall,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send;
}

/// Calls the gRPC request at `path` (or the draft) and keeps the call until
/// it ends: `outbox` gives the messages to send, which are rendered with
/// the request's variables as they go. Everything that happens is written
/// to a log in `ports.responses`, as a WebSocket connection's, which the
/// outcome's `body` is.
///
/// A message that does not render is not sent; the log says why. Secret
/// values are masked in the log.
///
/// # Errors
///
/// As [`send_request`](crate::send_request) up to the transport, and if the
/// request is not a gRPC one or the call cannot start.
pub async fn call_grpc<C, E, T, S, G, O>(
    ports: SendPorts<'_, C, E, T, S>,
    (grpc, outbox): (&G, &mut O),
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
    G: GrpcTransport + Sync,
    O: WsOutbox,
{
    exchange(ports, (&Call(grpc), outbox), path, options, cancel).await
}

/// A gRPC call, as an [`Exchange`].
struct Call<'g, G>(&'g G);

impl<G: GrpcTransport + Sync> Exchange for Call<'_, G> {
    const KIND: RequestKind = RequestKind::Grpc;

    async fn run<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        loaded: &HttpRequest,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> Result<ResponseHead, TransportError> {
        let Body::Grpc(call) = &loaded.body else {
            return Err(TransportError::InvalidRequest(
                "the request is not a gRPC call".into(),
            ));
        };
        self.0.call(request, call, handler, cancel).await
    }
}

/// What a build without gRPC says when asked for it.
pub const NO_GRPC: &str = "this build of apifast has no gRPC support";

/// The [`GrpcTransport`] of a build without gRPC: every call fails, saying
/// so.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoGrpc;

impl GrpcTransport for NoGrpc {
    fn describe(
        &self,
        _: &RenderedRequest,
        _: Option<&str>,
        _: &CancelToken,
    ) -> impl Future<Output = Result<GrpcCatalog, TransportError>> + Send {
        std::future::ready(Err(TransportError::Other(NO_GRPC.into())))
    }

    fn call<H: WsHandler>(
        &self,
        _: &RenderedRequest,
        _: &GrpcCall,
        _: &mut H,
        _: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send {
        std::future::ready(Err(TransportError::Other(NO_GRPC.into())))
    }
}

/// The services the gRPC request at `path` (or the draft) can call, as its
/// `.proto` file describes them or, without one, as its server says.
///
/// # Errors
///
/// As [`send_request`](crate::send_request) up to the transport, and if the
/// request is not a gRPC one or the transport fails.
pub async fn describe_grpc<C, E, T, S, G>(
    ports: SendPorts<'_, C, E, T, S>,
    grpc: &G,
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
) -> Result<GrpcCatalog, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
    G: GrpcTransport + Sync,
{
    let mut context = ChainContext::default();
    let read = |id: &ResponseId, len: usize| ports.responses.read(id, 0, len);
    let mut host = host_for(&ports, &read)?;
    let Prepared {
        loaded, rendered, ..
    } = prepare(
        &ports,
        path,
        options,
        cancel,
        (&mut context, &mut host, RequestKind::Grpc),
    )
    .await?;
    let proto = match &loaded.request.body {
        Body::Grpc(call) => call.proto_path.clone(),
        _ => None,
    };
    Ok(grpc
        .describe(&rendered.request, proto.as_deref(), cancel)
        .await?)
}
