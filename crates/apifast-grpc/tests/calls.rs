//! gRPC calls of the four kinds against a local server. No test uses the
//! external network.

#![allow(clippy::unwrap_used, reason = "test helpers: a failure fails the test")]

use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use apifast_app::memory::{CountingRandom, FixedClock};
use apifast_app::{
    AppHost, CancelToken, GrpcTransport, NoPrompter, RenderedRequest, ResponseHead, WsHandler,
    render_request,
};
use apifast_domain::{
    Chain, Collection, GrpcCall, Header, HttpRequest, Method, ResolvedVariables, WsData, WsEvent,
    WsSide, resolve,
};
use apifast_grpc::TonicTransport;
use apifast_http::ReqwestTransport;
use apifast_template::Mode;
use futures_util::{Stream, StreamExt as _};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::body::Body;
use tonic::server::{
    ClientStreamingService, Grpc, NamedService, ServerStreamingService, StreamingService,
    UnaryService,
};
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};
use tonic_prost::ProstCodec;

const HELLO: &str = r#"syntax = "proto3";
package hello;
message HelloRequest { string greeting = 1; }
message HelloReply { string reply = 1; string team = 2; }
service Greeter {
  rpc SayHello(HelloRequest) returns (HelloReply);
  rpc LotsOfReplies(HelloRequest) returns (stream HelloReply);
  rpc LotsOfGreetings(stream HelloRequest) returns (HelloReply);
  rpc Chat(stream HelloRequest) returns (stream HelloReply);
}
"#;

#[derive(Clone, PartialEq, prost::Message)]
struct HelloRequest {
    #[prost(string, tag = "1")]
    greeting: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct HelloReply {
    #[prost(string, tag = "1")]
    reply: String,
    #[prost(string, tag = "2")]
    team: String,
}

fn reply(text: impl Into<String>) -> HelloReply {
    HelloReply {
        reply: text.into(),
        team: String::new(),
    }
}

type Future<T> = Pin<Box<dyn std::future::Future<Output = Result<T, Status>> + Send>>;
type Replies = Pin<Box<dyn Stream<Item = Result<HelloReply, Status>> + Send>>;

/// `SayHello`: `hello <greeting>` with the `x-team` metadata; `fail` is
/// not found.
struct SayHello;

impl UnaryService<HelloRequest> for SayHello {
    type Response = HelloReply;
    type Future = Future<Response<HelloReply>>;

    fn call(&mut self, request: Request<HelloRequest>) -> Self::Future {
        let team = (request.metadata().get("x-team"))
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let greeting = request.into_inner().greeting;
        Box::pin(async move {
            if greeting == "fail" {
                return Err(Status::not_found("no fail"));
            }
            Ok(Response::new(HelloReply {
                reply: format!("hello {greeting}"),
                team,
            }))
        })
    }
}

/// `LotsOfReplies`: three replies.
struct LotsOfReplies;

impl ServerStreamingService<HelloRequest> for LotsOfReplies {
    type Response = HelloReply;
    type ResponseStream = Replies;
    type Future = Future<Response<Replies>>;

    fn call(&mut self, request: Request<HelloRequest>) -> Self::Future {
        let greeting = request.into_inner().greeting;
        let replies = (1..=3).map(move |n| Ok(reply(format!("{greeting} {n}"))));
        Box::pin(async move {
            Ok(Response::new(
                Box::pin(futures_util::stream::iter(replies)) as Replies
            ))
        })
    }
}

/// `LotsOfGreetings`: the greetings joined by `+`.
struct LotsOfGreetings;

impl ClientStreamingService<HelloRequest> for LotsOfGreetings {
    type Response = HelloReply;
    type Future = Future<Response<HelloReply>>;

    fn call(&mut self, request: Request<Streaming<HelloRequest>>) -> Self::Future {
        let mut greetings = request.into_inner();
        Box::pin(async move {
            let mut all = Vec::new();
            while let Some(request) = greetings.message().await? {
                all.push(request.greeting);
            }
            Ok(Response::new(reply(all.join("+"))))
        })
    }
}

/// `Chat`: each greeting back, in capitals.
struct Chat;

impl StreamingService<HelloRequest> for Chat {
    type Response = HelloReply;
    type ResponseStream = Replies;
    type Future = Future<Response<Replies>>;

    fn call(&mut self, request: Request<Streaming<HelloRequest>>) -> Self::Future {
        let replies =
            (request.into_inner()).map(|request| request.map(|r| reply(r.greeting.to_uppercase())));
        Box::pin(async move { Ok(Response::new(Box::pin(replies) as Replies)) })
    }
}

/// The `hello.Greeter` service, routed by path.
#[derive(Clone)]
struct Greeter;

impl NamedService for Greeter {
    const NAME: &'static str = "hello.Greeter";
}

impl tower_service::Service<http::Request<Body>> for Greeter {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        Box::pin(async move {
            let mut grpc = Grpc::new(ProstCodec::<HelloReply, HelloRequest>::default());
            Ok(match request.uri().path() {
                "/hello.Greeter/SayHello" => grpc.unary(SayHello, request).await,
                "/hello.Greeter/LotsOfReplies" => {
                    grpc.server_streaming(LotsOfReplies, request).await
                }
                "/hello.Greeter/LotsOfGreetings" => {
                    grpc.client_streaming(LotsOfGreetings, request).await
                }
                "/hello.Greeter/Chat" => grpc.streaming(Chat, request).await,
                _ => Status::unimplemented("no such method").into_http(),
            })
        })
    }
}

/// A folder with `hello.proto`.
fn protos() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.proto"), HELLO).unwrap();
    dir
}

/// A server with `hello.Greeter` and reflection; its address.
async fn server() -> String {
    let dir = protos();
    let mut compiler = protox::Compiler::new([dir.path()]).unwrap();
    compiler.open_file("hello.proto").unwrap();
    let reflection = tonic_reflection::server::Builder::configure()
        .register_file_descriptor_set(compiler.file_descriptor_set())
        .build_v1()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let serving = Server::builder()
        .add_service(Greeter)
        .add_service(reflection)
        .serve_with_incoming(TcpListenerStream::new(listener));
    tokio::spawn(serving);
    address
}

fn rendered(url: &str) -> RenderedRequest {
    let mut collection = Collection::new("t");
    collection.defaults.headers = vec![Header::new("X-Team", "core")];
    let request = HttpRequest::new("call", Method::Get, url);
    let (clock, random) = (FixedClock(0), CountingRandom::new(0));
    let mut host = AppHost::new(&clock, &random, &NoPrompter, BTreeMap::new());
    render_request(
        &resolve(Chain::new(&collection, &[], &request)),
        &ResolvedVariables::default(),
        &mut host,
        Mode::Strict,
    )
    .unwrap()
    .request
}

/// Gives its messages, then ends the client's stream (or waits, with
/// `hold`); keeps what happens, and cancels on the first answer if asked.
#[derive(Default)]
struct Recorder {
    outbox: VecDeque<WsData>,
    hold: bool,
    opened: Option<ResponseHead>,
    events: Vec<WsEvent>,
    cancel_on_answer: Option<CancelToken>,
}

impl Recorder {
    fn new(messages: &[&str]) -> Self {
        Self {
            outbox: messages.iter().map(|m| WsData::Text((*m).into())).collect(),
            ..Self::default()
        }
    }

    /// The text of what arrived, and how the call ended.
    fn received(&self) -> Vec<String> {
        (self.events.iter())
            .filter_map(|e| match e {
                WsEvent::Received(WsData::Text(text)) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn end(&self) -> Option<&WsEvent> {
        self.events.last()
    }
}

impl WsHandler for Recorder {
    fn opened(&mut self, head: &ResponseHead) {
        self.opened = Some(head.clone());
    }

    async fn outgoing(&mut self) -> Option<WsData> {
        match self.outbox.pop_front() {
            Some(data) => Some(data),
            None if self.hold => std::future::pending().await,
            None => None,
        }
    }

    fn event(&mut self, event: WsEvent) {
        if matches!(event, WsEvent::Received(_))
            && let Some(cancel) = &self.cancel_on_answer
        {
            cancel.cancel();
        }
        self.events.push(event);
    }
}

fn call(method: &str, proto: bool) -> GrpcCall {
    GrpcCall {
        method: format!("/hello.Greeter/{method}"),
        proto_path: proto.then(|| "hello.proto".into()),
        ..GrpcCall::default()
    }
}

fn transport(root: &Path) -> TonicTransport {
    TonicTransport::new(ReqwestTransport::new(root).unwrap())
}

fn closed(code: u16, reason: &str) -> WsEvent {
    WsEvent::Closed {
        by: WsSide::Server,
        code: Some(code),
        reason: reason.into(),
    }
}

#[tokio::test]
async fn a_unary_call_sends_the_first_message_that_fits_with_the_metadata() {
    let (dir, address) = (protos(), server().await);
    let grpc = transport(dir.path());
    let mut handler = Recorder::new(&[r#"{"nope": 1}"#, r#"{"greeting": "Ana"}"#, "{}"]);
    let head = grpc
        .call(
            &rendered(&address),
            &call("SayHello", true),
            &mut handler,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert!(
        matches!(&handler.events[0], WsEvent::Failed(why) if why.contains("hello.HelloRequest"))
    );
    assert_eq!(
        handler.events[1],
        WsEvent::Sent(WsData::Text(r#"{"greeting": "Ana"}"#.into()))
    );
    assert_eq!(
        handler.received(),
        [r#"{"reply":"hello Ana","team":"core"}"#]
    );
    assert_eq!(handler.end(), Some(&closed(0, "OK")));
    assert!(apifast_domain::is_grpc_response(&head.headers), "{head:?}");
    assert!(handler.opened.is_some());

    // A status other than OK ends the call; it is no error.
    let mut handler = Recorder::new(&[r#"{"greeting": "fail"}"#]);
    grpc.call(
        &rendered(&address),
        &call("SayHello", true),
        &mut handler,
        &CancelToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(handler.end(), Some(&closed(5, "NOT_FOUND: no fail")));

    // Without a message, or with an unknown method, it does not start.
    let error = grpc
        .call(
            &rendered(&address),
            &call("SayHello", true),
            &mut Recorder::new(&[]),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no message"), "{error}");
    let error = grpc
        .call(
            &rendered(&address),
            &call("Nope", true),
            &mut Recorder::new(&["{}"]),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("is not a method"), "{error}");
}

#[tokio::test]
async fn streams_go_both_ways() {
    let (dir, address) = (protos(), server().await);
    let grpc = transport(dir.path());

    let mut handler = Recorder::new(&[r#"{"greeting": "hi"}"#]);
    grpc.call(
        &rendered(&address),
        &call("LotsOfReplies", true),
        &mut handler,
        &CancelToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        handler.received(),
        [1, 2, 3].map(|n| format!(r#"{{"reply":"hi {n}","team":""}}"#))
    );

    let mut handler = Recorder::new(&[r#"{"greeting": "a"}"#, r#"{"greeting": "b"}"#]);
    grpc.call(
        &rendered(&address),
        &call("LotsOfGreetings", true),
        &mut handler,
        &CancelToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(handler.received(), [r#"{"reply":"a+b","team":""}"#]);
    assert_eq!(handler.end(), Some(&closed(0, "OK")));

    // Found by reflection this time.
    let mut handler = Recorder::new(&[r#"{"greeting": "x"}"#, r#"{"greeting": "y"}"#]);
    grpc.call(
        &rendered(&address),
        &call("Chat", false),
        &mut handler,
        &CancelToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        handler.received(),
        [r#"{"reply":"X","team":""}"#, r#"{"reply":"Y","team":""}"#]
    );
    assert_eq!(handler.end(), Some(&closed(0, "OK")));
}

#[tokio::test]
async fn cancelling_a_stream_ends_it_from_the_client() {
    let (dir, address) = (protos(), server().await);
    let cancel = CancelToken::new();
    let mut handler = Recorder {
        hold: true,
        cancel_on_answer: Some(cancel.clone()),
        ..Recorder::new(&[r#"{"greeting": "x"}"#])
    };
    transport(dir.path())
        .call(
            &rendered(&address),
            &call("Chat", true),
            &mut handler,
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(handler.received().len(), 1);
    assert_eq!(
        handler.end(),
        Some(&WsEvent::Closed {
            by: WsSide::Client,
            code: Some(1),
            reason: "CANCELLED".into(),
        })
    );
}
