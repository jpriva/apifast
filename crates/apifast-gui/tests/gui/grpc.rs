//! gRPC calls through the core, against a local gRPC server: loading the
//! methods, calling, sending the client's messages and ending its stream.
//! No external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use apifast_app::RequestPath;
use apifast_domain::{GrpcMethodType, WsData, WsEvent, WsSide, parse_ws_log};
use apifast_gui::core::{Command, Core, ResponseState, Snapshot, TabKey};
use apifast_vault::SystemKeyring;
use tonic::body::Body;
use tonic::server::{ClientStreamingService, Grpc, NamedService};
use tonic::{Request, Response, Status, Streaming};
use tonic_prost::ProstCodec;

const HELLO: &str = "syntax = \"proto3\";\npackage hello;\nmessage HelloRequest { string greeting = 1; }\nmessage HelloReply { string reply = 1; }\nservice Greeter {\n  rpc LotsOfGreetings(stream HelloRequest) returns (HelloReply);\n}\n";

#[derive(Clone, PartialEq, prost::Message)]
struct HelloRequest {
    #[prost(string, tag = "1")]
    greeting: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct HelloReply {
    #[prost(string, tag = "1")]
    reply: String,
}

/// The greetings, joined by `+`, once the client's stream ends.
struct LotsOfGreetings;

impl ClientStreamingService<HelloRequest> for LotsOfGreetings {
    type Response = HelloReply;
    type Future = Pin<Box<dyn Future<Output = Result<Response<HelloReply>, Status>> + Send>>;

    fn call(&mut self, request: Request<Streaming<HelloRequest>>) -> Self::Future {
        let mut greetings = request.into_inner();
        Box::pin(async move {
            let mut all = Vec::new();
            while let Some(request) = greetings.message().await? {
                all.push(request.greeting);
            }
            Ok(Response::new(HelloReply {
                reply: all.join("+"),
            }))
        })
    }
}

#[derive(Clone)]
struct Greeter;

impl NamedService for Greeter {
    const NAME: &'static str = "hello.Greeter";
}

impl tower_service::Service<http::Request<Body>> for Greeter {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        Box::pin(async move {
            let mut grpc = Grpc::new(ProstCodec::<HelloReply, HelloRequest>::default());
            Ok(match request.uri().path() {
                "/hello.Greeter/LotsOfGreetings" => {
                    grpc.client_streaming(LotsOfGreetings, request).await
                }
                _ => Status::unimplemented("no such method").into_http(),
            })
        })
    }
}

/// A gRPC server on its own thread; its port.
fn server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            tonic::transport::Server::builder()
                .add_service(Greeter)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
    });
    port
}

fn response(snapshot: &Snapshot) -> ResponseState {
    snapshot
        .active_tab()
        .map_or(ResponseState::Idle, |tab| tab.response.clone())
}

/// Waits until the snapshot satisfies `done`, failing after 10 seconds.
fn wait(core: &Core, what: &str, done: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let start = Instant::now();
    loop {
        let snapshot = core.snapshot();
        if done(&snapshot) {
            return snapshot;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}: {snapshot:#?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_grpc_request_loads_its_methods_and_streams_the_messages_picked() {
    let port = server();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: API\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("hello.proto"), HELLO).unwrap();
    std::fs::write(
        dir.path().join("greet.yml"),
        format!(
            "info:\n  name: Greet\n  type: grpc\ngrpc:\n  url: localhost:{port}\n  method: ''\n  protoFilePath: hello.proto\n  message:\n    - title: a\n      message: '{{\"greeting\": \"a\"}}'\n    - title: b\n      message: '{{\"greeting\": \"b\"}}'\n"
        ),
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    core.send(Command::OpenCollection(dir.path().to_path_buf()));
    let root = wait(&core, "the collection", |s| s.collection.is_some())
        .collection
        .as_ref()
        .unwrap()
        .root
        .clone();
    let key = TabKey::new(root, RequestPath::new("greet.yml"));
    core.send(Command::OpenRequest(key.clone()));
    wait(&core, "the request", |s| s.active_tab().is_some());

    core.send(Command::LoadMethods(key));
    let snapshot = wait(&core, "the methods", |s| {
        let methods = s.active_tab().and_then(|t| t.context.methods.clone());
        methods.is_some_and(|m| m.catalog.is_some())
    });
    let methods = snapshot
        .active_tab()
        .unwrap()
        .context
        .methods
        .clone()
        .unwrap();
    let catalog = methods.catalog.unwrap();
    let method = &catalog.services[0].methods[0];
    assert_eq!(method.path, "/hello.Greeter/LotsOfGreetings");
    assert_eq!(method.method_type, GrpcMethodType::ClientStreaming);

    // The method picked, as on screen.
    let mut request = snapshot.active_tab().unwrap().request().clone();
    if let apifast_domain::Body::Grpc(call) = &mut request.body {
        call.method.clone_from(&method.path);
        call.method_type = Some(method.method_type);
    }
    let key = snapshot.active.clone().unwrap();
    core.send(Command::Edit {
        tab: key,
        request: Box::new(request),
    });
    // Calling sends the message shown; then the other, and the end.
    core.send(Command::Send);
    core.send(Command::SendMessage(0));
    wait(&core, "the call", |s| match response(s) {
        ResponseState::Streaming(stream) => stream.read(|live| live.grpc),
        _ => false,
    });
    core.send(Command::SendMessage(1));
    core.send(Command::EndStream);
    let snapshot = wait(&core, "the end of the call", |s| {
        matches!(response(s), ResponseState::Received(_))
    });
    let ResponseState::Received(received) = response(&snapshot) else {
        unreachable!()
    };
    assert!(apifast_domain::is_grpc_response(&received.head.headers));
    let log = received.body.read(0, 4096).unwrap();
    let events: Vec<WsEvent> = parse_ws_log(&log).into_iter().map(|e| e.event).collect();
    let text = |t: &str| WsData::Text(t.into());
    assert_eq!(
        events,
        [
            WsEvent::Sent(text(r#"{"greeting": "a"}"#)),
            WsEvent::Sent(text(r#"{"greeting": "b"}"#)),
            WsEvent::Received(text(r#"{"reply":"a+b"}"#)),
            WsEvent::Closed {
                by: WsSide::Server,
                code: Some(0),
                reason: "OK".into(),
            },
        ]
    );
    assert!(
        (snapshot.history.iter()).any(|e| e.request.as_str() == "greet.yml"),
        "{:?}",
        snapshot.history
    );
}
