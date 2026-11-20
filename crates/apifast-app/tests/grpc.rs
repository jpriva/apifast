//! gRPC requests against the in-memory ports (docs/07, F8).

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryGrpc, MemoryOutbox,
    MemoryResponses, MemorySecrets, MemoryTransport, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, RequestPath, ResponseStore, SendOptions, SendOutcome, SendPorts,
    call_grpc, describe_grpc,
};
use apifast_domain::{
    AuthScheme, BearerAuth, Body, Collection, Environment, GrpcCall, GrpcMethod, GrpcMethodType,
    GrpcService, Header, HttpRequest, Method, Setting, Variable, VariableValue, WsData, WsEvent,
    WsMessage, WsMessageKind, WsSide, parse_ws_log,
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

struct World {
    collections: MemoryCollections,
    environments: MemoryEnvironments,
    transport: MemoryTransport,
    responses: MemoryResponses,
    clock: FixedClock,
    random: CountingRandom,
    prompter: ScriptedPrompter,
    secrets: MemorySecrets,
    grpc: MemoryGrpc,
}

impl World {
    /// A collection with metadata and a secret bearer token, and a gRPC
    /// request `call.yml` of `method_type`, and an HTTP one `plain.yml`.
    fn new(method_type: Option<GrpcMethodType>) -> Self {
        let mut collection = Collection::new("api");
        collection.defaults.headers = vec![Header::new("x-team", "core")];
        collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "{{token}}".into(),
        }));
        let mut call = HttpRequest::new("call", Method::Get, "{{host}}");
        call.body = Body::Grpc(GrpcCall {
            method: "/hello.Greeter/SayHello".into(),
            method_type,
            proto_path: Some("protos/hello.proto".into()),
            messages: Vec::new(),
        });
        let token = Variable {
            value: Some(VariableValue::text("t0k3n")),
            ..Variable::secret("token")
        };
        Self {
            collections: MemoryCollections::new(collection)
                .with_request("call.yml", call)
                .with_request(
                    "plain.yml",
                    HttpRequest::new("plain", Method::Get, "http://h"),
                ),
            environments: MemoryEnvironments::new(vec![Environment::new(
                "local",
                vec![
                    Variable::text("host", "localhost:50051"),
                    Variable::text("name", "Ada"),
                    token,
                ],
            )]),
            transport: MemoryTransport::new(200, ""),
            responses: MemoryResponses::default(),
            clock: FixedClock(0),
            random: CountingRandom::new(1),
            prompter: ScriptedPrompter::default(),
            secrets: MemorySecrets::default(),
            grpc: MemoryGrpc::new(services()),
        }
    }

    fn ports(
        &self,
    ) -> SendPorts<'_, MemoryCollections, MemoryEnvironments, MemoryTransport, MemoryResponses>
    {
        SendPorts {
            collections: &self.collections,
            environments: &self.environments,
            transport: &self.transport,
            responses: &self.responses,
            clock: &self.clock,
            random: &self.random,
            prompter: &self.prompter,
            secrets: &self.secrets,
            report: &|_| {},
            history: None,
            tokens: None,
            authorizer: None,
            scripts: None,
        }
    }

    fn options() -> SendOptions {
        SendOptions {
            environment: Some("local".into()),
            ..SendOptions::default()
        }
    }

    fn call(&self, path: &str, messages: &[&str]) -> Result<SendOutcome, AppError> {
        let outbox = (messages.iter())
            .map(|data| WsMessage::new("", WsMessageKind::Json, *data))
            .collect();
        block_on(call_grpc(
            self.ports(),
            (&self.grpc, &mut MemoryOutbox(outbox)),
            &RequestPath::new(path),
            &Self::options(),
            &CancelToken::new(),
        ))
    }

    fn log(&self, outcome: &SendOutcome) -> Vec<WsEvent> {
        let len = usize::try_from(outcome.body.size).unwrap();
        let log = self.responses.read(&outcome.body.id, 0, len).unwrap();
        parse_ws_log(&log).into_iter().map(|e| e.event).collect()
    }
}

#[test]
fn a_grpc_request_is_described_with_its_metadata_and_proto() {
    let world = World::new(None);
    let described = block_on(describe_grpc(
        world.ports(),
        &world.grpc,
        &RequestPath::new("call.yml"),
        &World::options(),
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(described.services, services());
    let (request, proto) = world.grpc.asked().pop().unwrap();
    assert_eq!(request.url(), "localhost:50051");
    assert_eq!(proto.as_deref(), Some("protos/hello.proto"));
    let headers = request.headers();
    assert!(headers.contains(&("x-team".into(), "core".into())));
    assert!(headers.contains(&("Authorization".into(), "Bearer t0k3n".into())));

    let error = block_on(describe_grpc(
        world.ports(),
        &world.grpc,
        &RequestPath::new("plain.yml"),
        &World::options(),
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.to_string(), "`plain.yml` is not a gRPC request");
    assert_eq!(world.grpc.asked().len(), 1);
}

#[test]
fn a_build_without_grpc_says_so() {
    let world = World::new(None);
    let error = block_on(describe_grpc(
        world.ports(),
        &apifast_app::NoGrpc,
        &RequestPath::new("call.yml"),
        &World::options(),
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.to_string(), apifast_app::NO_GRPC);
    let error = block_on(call_grpc(
        world.ports(),
        (&apifast_app::NoGrpc, &mut MemoryOutbox::default()),
        &RequestPath::new("call.yml"),
        &World::options(),
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.to_string(), apifast_app::NO_GRPC);
}

#[test]
fn a_call_renders_its_messages_masks_secrets_and_logs_how_it_ended() {
    let world = World::new(Some(GrpcMethodType::ClientStreaming));
    let outcome = world
        .call(
            "call.yml",
            &[
                r#"{"greeting": "hi {{name}}"}"#,
                r#"{"greeting": "{{missing}}"}"#,
                r#"{"token": "{{token}}"}"#,
            ],
        )
        .unwrap();
    assert_eq!(outcome.head.status, 200);
    assert!(outcome.contains_secret);
    let hello = WsData::Text(r#"{"greeting": "hi Ada"}"#.into());
    let events = world.log(&outcome);
    assert_eq!(
        events[..2],
        [WsEvent::Sent(hello.clone()), WsEvent::Received(hello)]
    );
    assert!(matches!(&events[2], WsEvent::Failed(why) if why.contains("missing")));
    let masked = format!(r#"{{"token": "{}"}}"#, apifast_domain::MASK);
    assert_eq!(events[3], WsEvent::Sent(WsData::Text(masked)));
    assert_eq!(
        events.last(),
        Some(&WsEvent::Closed {
            by: WsSide::Server,
            code: Some(0),
            reason: "OK".into(),
        })
    );
    let (request, _) = world.grpc.asked().pop().unwrap();
    assert!((request.headers()).contains(&("Authorization".into(), "Bearer t0k3n".into())));

    // A unary method sends only the first message.
    let world = World::new(Some(GrpcMethodType::Unary));
    let outcome = world.call("call.yml", &["{}", "{\"b\": 1}"]).unwrap();
    let sent = (world.log(&outcome).iter())
        .filter(|e| matches!(e, WsEvent::Sent(_)))
        .count();
    assert_eq!(sent, 1);

    let error = world.call("plain.yml", &[]).unwrap_err();
    assert_eq!(error.to_string(), "`plain.yml` is not a gRPC request");
}
