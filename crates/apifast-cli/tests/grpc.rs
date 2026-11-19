//! `apifast send` on gRPC requests end to end: the real binary, a
//! collection with a `.proto` and a local gRPC server. No test uses the
//! external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

mod common;

use std::convert::Infallible;
use std::fmt::Write as _;
use std::pin::Pin;
use std::task::{Context, Poll};

use common::{apifast, collection, text};
use futures_util::Stream;
use tonic::body::Body;
use tonic::server::{Grpc, NamedService, ServerStreamingService, UnaryService};
use tonic::{Request, Response, Status};
use tonic_prost::ProstCodec;

const HELLO: &str = "syntax = \"proto3\";\npackage hello;\nmessage HelloRequest { string greeting = 1; }\nmessage HelloReply { string reply = 1; }\nservice Greeter {\n  rpc SayHello(HelloRequest) returns (HelloReply);\n  rpc LotsOfReplies(HelloRequest) returns (stream HelloReply);\n}\n";

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

type Future<T> = Pin<Box<dyn std::future::Future<Output = Result<T, Status>> + Send>>;
type Replies = Pin<Box<dyn Stream<Item = Result<HelloReply, Status>> + Send>>;

/// `hello <greeting>`, with the bearer token seen; `fail` is not found.
struct SayHello;

impl UnaryService<HelloRequest> for SayHello {
    type Response = HelloReply;
    type Future = Future<Response<HelloReply>>;

    fn call(&mut self, request: Request<HelloRequest>) -> Self::Future {
        let token = (request.metadata().get("authorization"))
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let greeting = request.into_inner().greeting;
        Box::pin(async move {
            if greeting == "fail" {
                return Err(Status::not_found("no fail"));
            }
            Ok(Response::new(HelloReply {
                reply: format!("hello {greeting} ({token})"),
            }))
        })
    }
}

/// Two replies.
struct LotsOfReplies;

impl ServerStreamingService<HelloRequest> for LotsOfReplies {
    type Response = HelloReply;
    type ResponseStream = Replies;
    type Future = Future<Response<Replies>>;

    fn call(&mut self, request: Request<HelloRequest>) -> Self::Future {
        let greeting = request.into_inner().greeting;
        let replies = (1..=2).map(move |n| {
            Ok(HelloReply {
                reply: format!("{greeting} {n}"),
            })
        });
        Box::pin(async move {
            Ok(Response::new(
                Box::pin(futures_util::stream::iter(replies)) as Replies
            ))
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

/// A collection with `hello.proto` and a request `call.yml` calling
/// `method` with `messages` (JSON).
fn calling(port: u16, method: &str, messages: &[&str]) -> tempfile::TempDir {
    let dir = collection(port);
    std::fs::write(dir.path().join("hello.proto"), HELLO).unwrap();
    let messages = (messages.iter())
        .enumerate()
        .fold(String::new(), |mut out, (i, m)| {
            let _ = write!(out, "    - title: m{i}\n      message: '{m}'\n");
            out
        });
    std::fs::write(
        dir.path().join("call.yml"),
        format!(
            "info:\n  name: Call\n  type: grpc\ngrpc:\n  url: localhost:{port}\n  method: /hello.Greeter/{method}\n  protoFilePath: hello.proto\n  message:\n{messages}  auth: inherit\n"
        ),
    )
    .unwrap();
    dir
}

#[test]
fn a_grpc_request_is_called_and_prints_its_answers() {
    let port = server();
    let data = tempfile::tempdir().unwrap();

    let dir = calling(port, "SayHello", &[r#"{"greeting": "{{name}}"}"#]);
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "call.yml", "-e", "local", "--var", "name=Ana"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    // The collection's bearer token went as metadata.
    assert_eq!(
        text(&out.stdout),
        "{\"reply\":\"hello Ana (Bearer t0k3n)\"}\n"
    );
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("> {\"greeting\": \"Ana\"}\nOK · "),
        "{stderr}"
    );

    let dir = calling(port, "LotsOfReplies", &[r#"{"greeting": "hi"}"#]);
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "call.yml", "-e", "local", "-q"],
    );
    assert_eq!(
        text(&out.stdout),
        "{\"reply\":\"hi 1\"}\n{\"reply\":\"hi 2\"}\n"
    );
    assert_eq!(text(&out.stderr), "");

    // A status other than OK is said, and fails with `--fail`.
    let dir = calling(port, "SayHello", &[r#"{"greeting": "fail"}"#]);
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "call.yml", "-e", "local", "-q", "--fail"],
    );
    assert_eq!(out.status.code(), Some(4));
    assert!(
        text(&out.stderr).starts_with("NOT_FOUND: no fail · "),
        "{}",
        text(&out.stderr)
    );
}
