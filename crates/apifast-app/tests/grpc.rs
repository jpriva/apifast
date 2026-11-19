//! gRPC requests against the in-memory ports (docs/07, F8).

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryGrpc, MemoryResponses,
    MemorySecrets, MemoryTransport, ScriptedPrompter,
};
use apifast_app::{CancelToken, RequestPath, SendOptions, SendPorts, describe_grpc};
use apifast_domain::{
    AuthScheme, BearerAuth, Body, Collection, Environment, GrpcCall, GrpcMethod, GrpcMethodType,
    GrpcService, Header, HttpRequest, Method, Setting, Variable,
};

/// Runs a future that never waits, which is true of every in-memory port.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory futures complete on the first poll"),
    }
}

fn services() -> Vec<GrpcService> {
    vec![GrpcService {
        name: "hello.Greeter".into(),
        methods: vec![GrpcMethod {
            name: "SayHello".into(),
            path: "/hello.Greeter/SayHello".into(),
            method_type: GrpcMethodType::Unary,
            input: "hello.HelloRequest".into(),
            output: "hello.HelloReply".into(),
        }],
    }]
}

#[test]
fn a_grpc_request_is_described_with_its_metadata_and_proto() {
    let mut collection = Collection::new("api");
    collection.defaults.headers = vec![Header::new("x-team", "core")];
    collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
        token: "{{token}}".into(),
    }));
    let mut call = HttpRequest::new("call", Method::Get, "{{host}}");
    call.body = Body::Grpc(GrpcCall {
        method: "/hello.Greeter/SayHello".into(),
        proto_path: Some("protos/hello.proto".into()),
        ..GrpcCall::default()
    });
    let collections = MemoryCollections::new(collection)
        .with_request("call.yml", call)
        .with_request(
            "plain.yml",
            HttpRequest::new("plain", Method::Get, "http://h"),
        );
    let environments = MemoryEnvironments::new(vec![Environment::new(
        "local",
        vec![
            Variable::text("host", "localhost:50051"),
            Variable::text("token", "t0k3n"),
        ],
    )]);
    let (transport, responses) = (MemoryTransport::new(200, ""), MemoryResponses::default());
    let (clock, random) = (FixedClock(0), CountingRandom::new(1));
    let (prompter, secrets) = (ScriptedPrompter::default(), MemorySecrets::default());
    let ports = || SendPorts {
        collections: &collections,
        environments: &environments,
        transport: &transport,
        responses: &responses,
        clock: &clock,
        random: &random,
        prompter: &prompter,
        secrets: &secrets,
        report: &|_| {},
        history: None,
        tokens: None,
        authorizer: None,
    };
    let grpc = MemoryGrpc::new(services());
    let options = SendOptions {
        environment: Some("local".into()),
        ..SendOptions::default()
    };
    let described = block_on(describe_grpc(
        ports(),
        &grpc,
        &RequestPath::new("call.yml"),
        &options,
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(described, services());
    let (request, proto) = grpc.asked().pop().unwrap();
    assert_eq!(request.url(), "localhost:50051");
    assert_eq!(proto.as_deref(), Some("protos/hello.proto"));
    let headers = request.headers();
    assert!(headers.contains(&("x-team".into(), "core".into())));
    assert!(headers.contains(&("Authorization".into(), "Bearer t0k3n".into())));

    let error = block_on(describe_grpc(
        ports(),
        &grpc,
        &RequestPath::new("plain.yml"),
        &options,
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.to_string(), "`plain.yml` is not a gRPC request");
    assert_eq!(grpc.asked().len(), 1);
}
