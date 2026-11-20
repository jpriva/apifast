//! WebSocket connections against the in-memory ports (docs/07, F7).

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryOutbox,
    MemoryResponses, MemorySecrets, MemorySockets, MemoryTransport, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, RenderedBody, RequestPath, ResponseStore, SendOptions, SendOutcome,
    SendPorts, connect_request, send_request,
};
use apifast_domain::{
    AuthScheme, BearerAuth, Body, Collection, Environment, Header, HttpRequest, Method, Setting,
    Variable, VariableValue, WsData, WsEvent, WsMessage, WsMessageKind, WsSide, parse_ws_log,
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

struct World {
    collections: MemoryCollections,
    environments: MemoryEnvironments,
    transport: MemoryTransport,
    sockets: MemorySockets,
    responses: MemoryResponses,
    clock: FixedClock,
    random: CountingRandom,
    prompter: ScriptedPrompter,
    secrets: MemorySecrets,
}

impl World {
    /// A collection whose auth is a secret bearer token, with a WebSocket
    /// request `socket.yml` and an HTTP one `plain.yml`.
    fn new(messages: Vec<WsMessage>) -> Self {
        let mut collection = Collection::new("api");
        collection.defaults.headers = vec![Header::new("X-Client", "apifast")];
        collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "{{token}}".into(),
        }));
        let mut socket = HttpRequest::new("socket", Method::Get, "ws://{{host}}/live");
        socket.body = Body::WebSocket(messages);
        let token = Variable {
            value: Some(VariableValue::text("s3cr3t")),
            ..Variable::secret("token")
        };
        Self {
            collections: MemoryCollections::new(collection)
                .with_request("socket.yml", socket)
                .with_request(
                    "plain.yml",
                    HttpRequest::new("plain", Method::Get, "http://localhost"),
                ),
            environments: MemoryEnvironments::new(vec![Environment::new(
                "local",
                vec![
                    Variable::text("host", "localhost:8080"),
                    Variable::text("name", "Ada"),
                    token,
                ],
            )]),
            transport: MemoryTransport::new(200, ""),
            sockets: MemorySockets::default(),
            responses: MemoryResponses::default(),
            clock: FixedClock(1_700_000_000_000),
            random: CountingRandom::new(1),
            prompter: ScriptedPrompter::default(),
            secrets: MemorySecrets::default(),
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

    fn connect(&self, path: &str, outbox: Vec<WsMessage>) -> Result<SendOutcome, AppError> {
        let options = SendOptions {
            environment: Some("local".into()),
            ..SendOptions::default()
        };
        block_on(connect_request(
            self.ports(),
            (&self.sockets, &mut MemoryOutbox(outbox.into())),
            &RequestPath::new(path),
            &options,
            &CancelToken::new(),
        ))
    }

    fn log(&self, outcome: &SendOutcome) -> Vec<WsEvent> {
        let len = usize::try_from(outcome.body.size).unwrap_or(usize::MAX);
        let log = self
            .responses
            .read(&outcome.body.id, 0, len)
            .unwrap_or_default();
        parse_ws_log(&log).into_iter().map(|e| e.event).collect()
    }
}

fn text(data: &str) -> WsMessage {
    WsMessage::new("", WsMessageKind::Text, data)
}

#[test]
fn a_connection_inherits_headers_and_auth_and_logs_its_messages() {
    let world = World::new(vec![text("saved, not sent on its own")]);
    let outcome = world
        .connect(
            "socket.yml",
            vec![
                text("hello {{name}}"),
                text("{{missing}}"),
                text("token {{token}}"),
            ],
        )
        .unwrap();

    let handshake = world.sockets.connected().pop().unwrap();
    assert_eq!(handshake.url(), "ws://localhost:8080/live");
    assert_eq!(handshake.body(), &RenderedBody::None);
    let headers = handshake.headers();
    assert!(headers.contains(&("X-Client".into(), "apifast".into())));
    assert!(headers.contains(&("Authorization".into(), "Bearer s3cr3t".into())));

    assert_eq!(outcome.head.status, 101);
    assert!(outcome.contains_secret);
    let hello = WsData::Text("hello Ada".into());
    let events = world.log(&outcome);
    assert_eq!(
        events[..2],
        [WsEvent::Sent(hello.clone()), WsEvent::Received(hello)]
    );
    // A message that does not render is not sent, and the log says why.
    assert!(
        matches!(&events[2], WsEvent::Failed(why) if why.contains("missing")),
        "{events:?}"
    );
    // The secret went out as it is, and is masked in the log.
    assert_eq!(
        events[3],
        WsEvent::Sent(WsData::Text(format!("token {}", apifast_domain::MASK)))
    );
    assert_eq!(
        events.last(),
        Some(&WsEvent::Closed {
            by: WsSide::Client,
            code: Some(1000),
            reason: String::new(),
        })
    );
}

#[test]
fn only_a_websocket_request_connects_and_it_is_not_sent() {
    let world = World::new(Vec::new());
    let error = world.connect("plain.yml", Vec::new()).unwrap_err();
    assert_eq!(error.to_string(), "`plain.yml` is not a WebSocket request");

    let error = block_on(send_request(
        world.ports(),
        &RequestPath::new("socket.yml"),
        &SendOptions::default(),
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "`socket.yml` is a WebSocket request: it connects, it is not sent"
    );
    assert_eq!(world.transport.sent().len(), 0);
    assert_eq!(world.sockets.connected().len(), 0);
}

#[test]
fn a_grpc_request_is_called_not_sent() {
    let mut world = World::new(Vec::new());
    let mut call = HttpRequest::new("call", Method::Get, "localhost:50051");
    call.body = Body::Grpc(apifast_domain::GrpcCall::default());
    world.collections = world.collections.with_request("call.yml", call);
    let error = block_on(send_request(
        world.ports(),
        &RequestPath::new("call.yml"),
        &SendOptions::default(),
        &CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "`call.yml` is a gRPC request: it is called, not sent"
    );
    assert_eq!(world.transport.sent().len(), 0);
}
