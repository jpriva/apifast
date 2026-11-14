//! Use cases against the in-memory ports.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryResponses,
    MemoryTransport, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, RenderedBody, RequestPath, ResponseStore, SendOptions, SendOutcome,
    SendPorts, TransportError, TreeItem, open_collection, save_request, send_request,
};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, BasicAuth, BearerAuth, Body, Collection, Environment,
    EnvironmentError, Folder, Header, HttpRequest, Method, Param, ParamKind, Setting, Variable,
    VariableValue,
};
use apifast_template::{Problem, RenderError};

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
    responses: MemoryResponses,
    clock: FixedClock,
    random: CountingRandom,
    prompter: ScriptedPrompter,
}

impl World {
    fn new(collections: MemoryCollections) -> Self {
        Self {
            collections,
            environments: MemoryEnvironments::default(),
            transport: MemoryTransport::new(200, r#"{"ok":true}"#),
            responses: MemoryResponses::default(),
            clock: FixedClock(1_700_000_000_000),
            random: CountingRandom::new(7),
            prompter: ScriptedPrompter::default(),
        }
    }

    fn send(&self, path: &str, options: &SendOptions) -> Result<SendOutcome, AppError> {
        self.send_with(path, options, &CancelToken::new())
    }

    fn send_with(
        &self,
        path: &str,
        options: &SendOptions,
        cancel: &CancelToken,
    ) -> Result<SendOutcome, AppError> {
        let ports = SendPorts {
            collections: &self.collections,
            environments: &self.environments,
            transport: &self.transport,
            responses: &self.responses,
            clock: &self.clock,
            random: &self.random,
            prompter: &self.prompter,
            report: &|_| {},
        };
        block_on(send_request(
            ports,
            &RequestPath::new(path),
            options,
            cancel,
        ))
    }

    #[expect(clippy::expect_used, reason = "test helper: fails the test")]
    fn last_sent(&self) -> apifast_app::RenderedRequest {
        self.transport.sent().pop().expect("nothing was sent")
    }
}

fn header<'a>(request: &'a apifast_app::RenderedRequest, name: &str) -> Vec<&'a str> {
    request
        .headers()
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .collect()
}

fn get(url: &str) -> HttpRequest {
    HttpRequest::new("request", Method::Get, url)
}

fn env(name: &str, extends: Option<&str>, variables: Vec<Variable>) -> Environment {
    Environment {
        extends: extends.map(str::to_owned),
        ..Environment::new(name, variables)
    }
}

/// A collection with inheritance at every level, like a real one.
fn users_world() -> World {
    let mut collection = Collection::new("api");
    collection.defaults.headers = vec![Header::new("Accept", "application/json")];
    collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
        token: "{{token}}".into(),
    }));
    collection.defaults.variables = vec![Variable::text("api_version", "v1")];

    let mut users = Folder::new("users");
    users.defaults.variables = vec![Variable::text("resource", "users")];

    let mut request = HttpRequest::new(
        "Create user",
        Method::Post,
        "{{host}}/{{api_version}}/{{resource}}/:id?notify=true",
    );
    request.params = vec![
        Param {
            name: "id".into(),
            value: "{{user_id}}".into(),
            kind: ParamKind::Path,
            enabled: true,
        },
        // Mirrors the URL query, as Bruno writes it; must not be sent twice.
        Param {
            name: "notify".into(),
            value: "true".into(),
            kind: ParamKind::Query,
            enabled: true,
        },
    ];
    request.body = Body::Json(r#"{"name": "{{$randomInt}}"}"#.into());

    let mut world = World::new(
        MemoryCollections::new(collection)
            .with_folder("users", users)
            .with_request("users/create.yml", request),
    );
    let token = Variable {
        value: Some(VariableValue::text("s3cr3t")),
        ..Variable::secret("token")
    };
    world.environments = MemoryEnvironments::new(vec![
        env(
            "base",
            None,
            vec![Variable::text("host", "http://localhost")],
        ),
        env(
            "prod",
            Some("base"),
            vec![Variable::text("host", "https://api.test"), token],
        ),
    ]);
    world
}

fn prod() -> SendOptions {
    SendOptions {
        environment: Some("prod".into()),
        runtime: vec![Variable::text("user_id", "981")],
        ..SendOptions::default()
    }
}

#[test]
fn sends_a_request_with_inheritance_variables_and_auth() {
    let world = users_world();
    let outcome = world.send("users/create.yml", &prod()).unwrap();

    let sent = world.last_sent();
    assert_eq!(sent.method(), &Method::Post);
    assert_eq!(sent.url(), "https://api.test/v1/users/981?notify=true");
    assert_eq!(header(&sent, "accept"), ["application/json"]);
    assert_eq!(header(&sent, "authorization"), ["Bearer s3cr3t"]);
    assert_eq!(header(&sent, "content-type"), ["application/json"]);
    assert_eq!(sent.body(), &RenderedBody::Text(r#"{"name": "7"}"#.into()));
    assert_eq!(sent.timeout_ms(), 30_000);
    assert!(sent.contains_secret());

    assert_eq!(outcome.head.status, 200);
    assert!(outcome.contains_secret);
    assert_eq!(outcome.body.size, 11);
    assert_eq!(
        world.responses.read(&outcome.body.id, 1, 4).unwrap(),
        b"\"ok\""
    );
}

#[test]
fn a_missing_variable_fails_unless_allowed() {
    let world = users_world();
    let no_env = SendOptions {
        environment: None,
        ..prod()
    };
    assert!(matches!(
        world.send("users/create.yml", &no_env),
        Err(AppError::Render(RenderError(Problem::MissingVariable(name)))) if name == "host"
    ));
    assert_eq!(world.transport.sent().len(), 0, "nothing must be sent");

    let allowed = SendOptions {
        allow_unresolved: true,
        ..no_env
    };
    let outcome = world.send("users/create.yml", &allowed).unwrap();
    assert_eq!(world.last_sent().url(), "/v1/users/981?notify=true");
    assert_eq!(
        outcome.problems,
        [
            Problem::MissingVariable("host".into()),
            Problem::MissingVariable("token".into())
        ]
    );
}

#[test]
fn an_unknown_environment_is_an_error() {
    let world = users_world();
    let options = SendOptions {
        environment: Some("staging".into()),
        ..prod()
    };
    assert_eq!(
        world.send("users/create.yml", &options),
        Err(AppError::Environment(EnvironmentError::NotFound(
            "staging".into()
        )))
    );
}

#[test]
fn a_cancelled_send_reports_cancellation() {
    let world = users_world();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        world.send_with("users/create.yml", &prod(), &cancel),
        Err(AppError::Transport(TransportError::Cancelled))
    );
}

#[test]
fn basic_auth_is_base64_encoded_and_replaces_authorization() {
    let mut request = get("https://api.test");
    request.headers = vec![Header::new("authorization", "old")];
    request.auth = Setting::Set(AuthScheme::Basic(BasicAuth {
        username: "{{user}}".into(),
        password: "pa:ss".into(),
    }));
    let mut world =
        World::new(MemoryCollections::new(Collection::new("c")).with_request("r.yml", request));
    world.environments =
        MemoryEnvironments::new(vec![env("e", None, vec![Variable::text("user", "ana")])]);

    world
        .send(
            "r.yml",
            &SendOptions {
                environment: Some("e".into()),
                ..SendOptions::default()
            },
        )
        .unwrap();
    // "ana:pa:ss"
    assert_eq!(
        header(&world.last_sent(), "authorization"),
        ["Basic YW5hOnBhOnNz"]
    );
}

#[test]
fn api_keys_go_in_a_header_or_in_the_query() {
    let key = |placement| {
        Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
            key: "X-Key".into(),
            value: "a b".into(),
            placement,
        }))
    };
    let mut in_header = get("https://api.test/x");
    in_header.headers = vec![Header::new("x-key", "old")];
    in_header.auth = key(ApiKeyPlacement::Header);
    let mut in_query = get("https://api.test/x?y=1");
    in_query.auth = key(ApiKeyPlacement::Query);
    let world = World::new(
        MemoryCollections::new(Collection::new("c"))
            .with_request("h.yml", in_header)
            .with_request("q.yml", in_query),
    );

    world.send("h.yml", &SendOptions::default()).unwrap();
    assert_eq!(header(&world.last_sent(), "x-key"), ["a b"]);
    world.send("q.yml", &SendOptions::default()).unwrap();
    assert_eq!(
        world.last_sent().url(),
        "https://api.test/x?y=1&X-Key=a%20b"
    );
}

#[test]
fn an_explicit_content_type_is_kept() {
    let mut request = get("https://api.test");
    request.headers = vec![Header::new("content-type", "application/vnd.api+json")];
    request.body = Body::Json("{}".into());
    let world =
        World::new(MemoryCollections::new(Collection::new("c")).with_request("r.yml", request));

    world.send("r.yml", &SendOptions::default()).unwrap();
    assert_eq!(
        header(&world.last_sent(), "content-type"),
        ["application/vnd.api+json"]
    );
}

#[test]
fn each_prompt_is_asked_once_per_send() {
    let mut request = get("https://api.test/{{?Code}}");
    request.headers = vec![Header::new("X-Code", "{{?Code}}")];
    let mut world =
        World::new(MemoryCollections::new(Collection::new("c")).with_request("r.yml", request));
    world.prompter = ScriptedPrompter::default().answer("Code", "123");

    world.send("r.yml", &SendOptions::default()).unwrap();
    assert_eq!(world.last_sent().url(), "https://api.test/123");
    assert_eq!(header(&world.last_sent(), "x-code"), ["123"]);
    assert_eq!(world.prompter.asked(), ["Code"]);
}

#[test]
fn process_env_comes_from_the_environment_repository() {
    let request = get("https://api.test/{{process.env.TENANT}}");
    let mut world =
        World::new(MemoryCollections::new(Collection::new("c")).with_request("r.yml", request));
    world.environments = MemoryEnvironments::default().with_process_env("TENANT", "acme");

    world.send("r.yml", &SendOptions::default()).unwrap();
    assert_eq!(world.last_sent().url(), "https://api.test/acme");
}

#[test]
fn saving_validates_and_stores_the_request() {
    let world = World::new(MemoryCollections::new(Collection::new("c")));
    let path = RequestPath::new("new.yml");

    let mut unnamed = get("/");
    unnamed.name = "  ".into();
    assert!(matches!(
        save_request(&world.collections, &path, &unnamed),
        Err(AppError::Validation(_))
    ));

    save_request(&world.collections, &path, &get("/x")).unwrap();
    assert_eq!(world.collections.get("new.yml").unwrap().url, "/x");
}

#[test]
fn opening_a_collection_returns_a_sorted_tree() {
    let seq = |name: &str, seq| HttpRequest {
        seq,
        ..HttpRequest::new(name, Method::Get, "/")
    };
    let mut users = Folder::new("users");
    users.seq = Some(1);
    let world = World::new(
        MemoryCollections::new(Collection::new("c"))
            .with_folder("users", users)
            .with_request("users/b.yml", seq("B", Some(2)))
            .with_request("users/a.yml", seq("A", Some(2)))
            .with_request("health.yml", seq("Health", None)),
    );

    let tree = open_collection(&world.collections).unwrap();
    let top: Vec<_> = tree
        .items
        .iter()
        .map(|item| match item {
            TreeItem::Folder {
                folder, children, ..
            } => {
                let names: Vec<_> = children
                    .iter()
                    .map(|c| match c {
                        TreeItem::Request { name, .. } | TreeItem::Other { name, .. } => {
                            name.as_str()
                        }
                        TreeItem::Folder { folder, .. } => folder.name.as_str(),
                        TreeItem::Invalid { path, .. } => path.as_str(),
                    })
                    .collect();
                format!("{}{names:?}", folder.name)
            }
            TreeItem::Request { name, .. } | TreeItem::Other { name, .. } => name.clone(),
            TreeItem::Invalid { path, .. } => path.to_string(),
        })
        .collect();
    assert_eq!(top, ["users[\"A\", \"B\"]", "Health"]);
}

#[test]
fn problems_are_reported_before_sending_even_if_the_send_fails() {
    let world = users_world();
    let reported = std::cell::RefCell::new(Vec::new());
    let cancel = CancelToken::new();
    cancel.cancel();
    let ports = SendPorts {
        collections: &world.collections,
        environments: &world.environments,
        transport: &world.transport,
        responses: &world.responses,
        clock: &world.clock,
        random: &world.random,
        prompter: &world.prompter,
        report: &|problem: &Problem| reported.borrow_mut().push(problem.clone()),
    };
    let options = SendOptions {
        allow_unresolved: true,
        ..SendOptions::default()
    };
    let result = block_on(send_request(
        ports,
        &RequestPath::new("users/create.yml"),
        &options,
        &cancel,
    ));
    assert_eq!(result, Err(AppError::Transport(TransportError::Cancelled)));
    assert_eq!(
        reported.into_inner(),
        [
            Problem::MissingVariable("host".into()),
            Problem::MissingVariable("user_id".into()),
            Problem::MissingVariable("token".into()),
        ]
    );
}
