//! gRPC (docs/07, F8): a gRPC request is rendered like any other
//! (inheritance, templates, auth), its headers being the call's metadata;
//! a [`GrpcTransport`] then asks its server what it offers, or reads the
//! `.proto` file that describes it.

use std::future::Future;

use apifast_domain::{Body, GrpcService, RequestKind};

use crate::use_cases::{ChainContext, Prepared, host_for, prepare};
use crate::{
    AppError, CancelToken, CollectionRepository, EnvironmentRepository, HttpTransport,
    RenderedRequest, RequestPath, ResponseId, ResponseStore, SendOptions, SendPorts,
    TransportError,
};

/// Talks to gRPC servers.
pub trait GrpcTransport {
    /// The services the server of `request` offers, as reflection says;
    /// or, with `proto` (relative to the collection's folder), as that
    /// file describes them, without asking the server. The request's
    /// headers go as metadata.
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
    ) -> impl Future<Output = Result<Vec<GrpcService>, TransportError>> + Send;
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
) -> Result<Vec<GrpcService>, AppError>
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
