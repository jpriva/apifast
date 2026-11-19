//! A gRPC call (docs/07, F8). The four kinds go the same way, as one
//! HTTP/2 stream each way: the client sends one message, or as many as it
//! is given, and reads what the server answers until it ends the call with
//! a status.

use std::pin::pin;
use std::str::FromStr as _;
use std::time::{Duration, Instant};

use apifast_app::{CancelToken, RenderedRequest, ResponseHead, TransportError, WsHandler};
use apifast_domain::{GrpcCall, WsData, WsEvent, WsSide, grpc_status_name};
use apifast_http::ReqwestTransport;
use http::uri::PathAndQuery;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor};
use tokio::sync::mpsc;
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};

use crate::codec::{DynamicCodec, from_json, to_json};
use crate::target::Target;
use crate::{cancelled, channel, metadata, protos, reflection};

/// Calls `call.method` on the server of `request`; see
/// [`apifast_app::GrpcTransport::call`].
pub(crate) async fn call<H: WsHandler>(
    http: &ReqwestTransport,
    request: &RenderedRequest,
    call: &GrpcCall,
    handler: &mut H,
    cancel: &CancelToken,
) -> Result<ResponseHead, TransportError> {
    let started = Instant::now();
    let deadline = (request.timeout_ms() > 0)
        .then(|| tokio::time::Instant::now() + Duration::from_millis(request.timeout_ms()));
    let expired = async {
        match deadline {
            Some(at) => tokio::time::sleep_until(at).await,
            None => std::future::pending().await,
        }
    };
    let mut expired = pin!(expired);
    let target = Target::parse(request.url())?;
    let metadata = metadata(request)?;
    // Reaching the server and finding the method count for the timeout.
    let (channel, method) = tokio::select! {
        found = find(http, &target, &metadata, call) => found?,
        () = &mut expired => return Err(TransportError::Timeout(request.timeout_ms())),
        () = cancelled(cancel) => return Err(TransportError::Cancelled),
    };
    let mut head = head(request);
    handler.opened(&head);
    let input = method.input();
    let (sender, receiver) = mpsc::unbounded_channel::<DynamicMessage>();
    let mut sender = Some(sender);
    // A method whose client does not stream takes its one message first.
    if !method.is_client_streaming() {
        let Some((message, data)) = next_message(handler, &input).await else {
            return Err(TransportError::InvalidRequest(
                "there is no message to send".into(),
            ));
        };
        let _ = sender.take().map(|s| s.send(message));
        handler.event(WsEvent::Sent(data));
    }
    let outgoing = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|message| (message, receiver))
    });
    let mut outgoing = tonic::Request::new(outgoing);
    *outgoing.metadata_mut() = metadata;
    let path = format!("/{}/{}", method.parent_service().full_name(), method.name());
    let path = PathAndQuery::from_str(&path)
        .map_err(|e| TransportError::InvalidRequest(format!("`{path}`: {e}")))?;
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready()
        .await
        .map_err(|e| TransportError::Protocol(crate::chain(&e)))?;
    let codec = DynamicCodec::new(method.output());
    let mut response = pin!(grpc.streaming(outgoing, path, codec));
    // Until the server answers, messages may go out.
    let response = loop {
        tokio::select! {
            response = &mut response => break response,
            next = next_message(handler, &input), if sender.is_some() => {
                forward(next, &mut sender, handler);
            }
            () = &mut expired => return Err(TransportError::Timeout(request.timeout_ms())),
            () = cancelled(cancel) => {
                handler.event(by_client());
                return Ok(finished(head, started));
            }
        }
    };
    let mut answers = match response {
        Ok(response) => {
            head.headers.extend(headers(response.metadata()));
            response.into_inner()
        }
        Err(status) => {
            head.headers.extend(headers(status.metadata()));
            handler.event(ended(&status));
            return Ok(finished(head, started));
        }
    };
    let end = loop {
        tokio::select! {
            answer = answers.message() => match answer {
                Ok(Some(message)) => handler.event(WsEvent::Received(WsData::Text(to_json(&message)))),
                Ok(None) => {
                    if let Ok(Some(trailers)) = answers.trailers().await {
                        head.headers.extend(headers(&trailers));
                    }
                    break ended(&Status::new(Code::Ok, ""));
                }
                Err(status) => {
                    head.headers.extend(headers(status.metadata()));
                    break ended(&status);
                }
            },
            next = next_message(handler, &input), if sender.is_some() => {
                forward(next, &mut sender, handler);
            }
            () = cancelled(cancel) => break by_client(),
        }
    };
    handler.event(end);
    Ok(finished(head, started))
}

/// The channel to `target`, and `call.method` among the methods its
/// `.proto` or its reflection describes.
async fn find(
    http: &ReqwestTransport,
    target: &Target,
    metadata: &MetadataMap,
    call: &GrpcCall,
) -> Result<(channel::Prefixed, MethodDescriptor), TransportError> {
    let channel = channel::connect(http, target).await?;
    let pool = match &call.proto_path {
        Some(proto) => protos::compile(http.files_root(), proto)?,
        None => reflection::reflect(&channel, metadata).await?,
    };
    let method = method(&pool, &call.method)?;
    Ok((channel, method))
}

/// The method at `path` (`/package.Service/Method`) in `pool`.
///
/// # Errors
///
/// If the path is not one of its methods.
fn method(pool: &DescriptorPool, path: &str) -> Result<MethodDescriptor, TransportError> {
    let path = path.trim();
    let not_found = || {
        TransportError::InvalidRequest(if path.is_empty() {
            "choose the method to call".into()
        } else {
            format!("`{path}` is not a method of the server")
        })
    };
    let (service, name) = path
        .trim_start_matches('/')
        .split_once('/')
        .ok_or_else(not_found)?;
    let service = pool.get_service_by_name(service).ok_or_else(not_found)?;
    let found = service.methods().find(|m| m.name() == name);
    found.ok_or_else(not_found)
}

/// The next message `handler` gives that fits `input`; those that do not
/// are logged as not sent. `None` when it has no more.
async fn next_message<H: WsHandler>(
    handler: &mut H,
    input: &MessageDescriptor,
) -> Option<(DynamicMessage, WsData)> {
    loop {
        let data = handler.outgoing().await?;
        match from_json(&data, input) {
            Ok(message) => return Some((message, data)),
            Err(why) => handler.event(WsEvent::Failed(format!("not sent: {why}"))),
        }
    }
}

/// Sends `next` down `sender`, and tells `handler`; `None` ends the
/// client's stream.
fn forward<H: WsHandler>(
    next: Option<(DynamicMessage, WsData)>,
    sender: &mut Option<mpsc::UnboundedSender<DynamicMessage>>,
    handler: &mut H,
) {
    match next {
        Some((message, data)) => {
            if sender.as_ref().is_some_and(|s| s.send(message).is_ok()) {
                handler.event(WsEvent::Sent(data));
            }
        }
        None => *sender = None,
    }
}

/// The head of a call's response, before the server answers.
fn head(request: &RenderedRequest) -> ResponseHead {
    ResponseHead {
        status: 200,
        version: "HTTP/2.0".into(),
        url: request.url().to_owned(),
        redirects: 0,
        headers: vec![("content-type".into(), "application/grpc".into())],
        elapsed_ms: 0,
        timeline: Vec::new(),
    }
}

/// `head`, its time that of the whole call.
fn finished(mut head: ResponseHead, started: Instant) -> ResponseHead {
    head.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    head
}

/// Metadata as headers; binary values as they came, lossily.
fn headers(metadata: &MetadataMap) -> Vec<(String, String)> {
    let map = metadata.clone().into_headers();
    (map.iter())
        .filter(|(name, _)| name.as_str() != "content-type")
        .map(|(name, value)| {
            let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
            (name.as_str().to_owned(), value)
        })
        .collect()
}

/// The call ended with `status`, from the server.
fn ended(status: &Status) -> WsEvent {
    let code = status.code() as i32;
    let name = u16::try_from(code)
        .ok()
        .and_then(grpc_status_name)
        .unwrap_or("UNKNOWN");
    let reason = if status.message().is_empty() {
        name.to_owned()
    } else {
        format!("{name}: {}", status.message())
    };
    WsEvent::Closed {
        by: WsSide::Server,
        code: u16::try_from(code).ok(),
        reason,
    }
}

/// The user cancelled the call.
fn by_client() -> WsEvent {
    WsEvent::Closed {
        by: WsSide::Client,
        code: Some(Code::Cancelled as u16),
        reason: "CANCELLED".into(),
    }
}
