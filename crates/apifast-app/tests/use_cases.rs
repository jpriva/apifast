//! Use cases against the in-memory ports.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryHistory,
    MemoryResponses, MemorySecrets, MemoryTransport, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, HistoryStore, RenderedBody, RequestPath, ResponseStore, SendOptions,
    SendOutcome, SendPorts, TransportError, TreeItem, open_collection, save_request, send_request,
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
    secrets: MemorySecrets,
    history: Option<MemoryHistory>,
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
            secrets: MemorySecrets::default(),
            history: None,
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
            secrets: &self.secrets,
            report: &|_| {},
            history: self
                .history
                .as_ref()
                .map(|h| h as &(dyn HistoryStore + Sync)),
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
    let reported = std::sync::Mutex::new(Vec::new());
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
        secrets: &world.secrets,
        report: &|problem: &Problem| reported.lock().unwrap().push(problem.clone()),
        history: None,
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
        reported.into_inner().unwrap(),
        [
            Problem::MissingVariable("host".into()),
            Problem::MissingVariable("user_id".into()),
            Problem::MissingVariable("token".into()),
        ]
    );
}

/// Use cases run as tasks on multi-threaded runtimes, so their futures must
/// be `Send`. This does not compile otherwise.
#[test]
fn send_request_can_run_on_any_thread() {
    fn assert_send<T: Send>(_: &T) {}
    let world = users_world();
    let ports = SendPorts {
        collections: &world.collections,
        environments: &world.environments,
        transport: &world.transport,
        responses: &world.responses,
        clock: &world.clock,
        random: &world.random,
        prompter: &world.prompter,
        secrets: &world.secrets,
        report: &|_| {},
        history: None,
    };
    let options = SendOptions::default();
    let cancel = CancelToken::new();
    let path = RequestPath::new("users/create.yml");
    assert_send(&send_request(ports, &path, &options, &cancel));
}

#[test]
fn a_draft_is_sent_instead_of_the_saved_request_with_the_same_inheritance() {
    let world = users_world();
    let draft = HttpRequest::new("Create user", Method::Put, "{{host}}/{{resource}}/draft");
    let options = SendOptions {
        draft: Some(draft),
        ..prod()
    };
    world.send("users/create.yml", &options).unwrap();
    let sent = world.last_sent();
    assert_eq!(sent.method(), &Method::Put);
    // Folder variables and collection auth still apply.
    assert_eq!(sent.url(), "https://api.test/users/draft");
    assert_eq!(header(&sent, "authorization"), ["Bearer s3cr3t"]);
    // The saved request did not change.
    assert_eq!(
        world.collections.get("users/create.yml").unwrap().method,
        Method::Post
    );
}

/// A secret declared without a value takes it from the vault for the send.
#[test]
fn a_secret_takes_its_value_from_the_vault_for_the_send() {
    let mut world = World::new(
        MemoryCollections::new(Collection::new("api"))
            .with_request("me.yml", get("{{host}}/me?key={{api_key}}")),
    );
    world.environments = MemoryEnvironments::new(vec![env(
        "prod",
        None,
        vec![
            Variable::text("host", "https://api.test"),
            Variable::secret("api_key"),
        ],
    )]);
    let options = SendOptions {
        environment: Some("prod".into()),
        ..SendOptions::default()
    };

    // Not in the vault: the secret has no value, and nothing is sent.
    let error = world.send("me.yml", &options).unwrap_err();
    assert_eq!(error.to_string(), "secret `api_key` has no value available");
    assert_eq!(world.transport.sent().len(), 0);

    world.secrets = MemorySecrets::default().with("prod", "api_key", "k3y");
    let outcome = world.send("me.yml", &options).unwrap();
    assert!(outcome.contains_secret);
    assert_eq!(
        world.transport.sent()[0].url(),
        "https://api.test/me?key=k3y"
    );
    // What is shown or stored can mask it; printing the request never
    // shows it.
    assert_eq!(outcome.secrets, [apifast_domain::Secret::new("k3y")]);
    let printed = format!("{:?}", world.transport.sent()[0]);
    assert!(!printed.contains("k3y"), "{printed}");
    assert!(printed.contains(apifast_domain::MASK), "{printed}");
}

#[test]
fn captures_run_before_assertions_and_feed_the_next_send() {
    use apifast_app::apply_captures;
    use apifast_domain::{Assertion, Capture, CaptureScope, Operator, ValueKind};

    let mut login = HttpRequest::new("login", Method::Post, "{{host}}/login");
    login.captures = vec![
        Capture::new("res.body.token", "token"),
        Capture::new("res.body.user", "user"),
        Capture {
            scope: CaptureScope::Request,
            ..Capture::new("res.body.user.id", "user_id")
        },
        Capture::new("res.body.missing", "nothing"),
    ];
    login.assertions = vec![
        Assertion::new("res.status", Operator::Eq, Some("200")),
        // A value captured just now is there for the assertions.
        Assertion::new("res.body.user.id", Operator::Eq, Some("{{user_id}}")),
        Assertion::new("res.body.token", Operator::Eq, Some("wrong")),
        Assertion {
            enabled: false,
            ..Assertion::new("res.status", Operator::Eq, Some("500"))
        },
    ];
    let me = get("{{host}}/users/{{user.id}}?t={{token}}");
    let mut world = World::new(
        MemoryCollections::new(Collection::new("api"))
            .with_request("login.yml", login)
            .with_request("me.yml", me),
    );
    world.transport = MemoryTransport::new(200, r#"{"token":"t0k","user":{"id":5}}"#);
    let mut options = SendOptions {
        runtime: vec![Variable::text("host", "http://h")],
        ..SendOptions::default()
    };

    let outcome = world.send("login.yml", &options).unwrap();
    let passed: Vec<bool> = outcome
        .assertions
        .iter()
        .map(|a| a.verdict.passed)
        .collect();
    assert_eq!(passed, [true, true, false], "the disabled one is skipped");
    assert_eq!(outcome.assertions[1].expected.as_deref(), Some("5"));
    assert_eq!(
        outcome.assertions[2].verdict.message.as_deref(),
        Some("expected eq wrong, got \"t0k\"")
    );
    let values: Vec<_> = outcome.captures.iter().map(|c| c.value.clone()).collect();
    assert_eq!(values[0], Ok(VariableValue::text("t0k")));
    assert_eq!(values[1].as_ref().unwrap().kind, ValueKind::Object);
    assert_eq!(values[3], Err("res.body.missing is undefined".into()));

    apply_captures(&mut options.runtime, &outcome.captures);
    let names: Vec<&str> = options.runtime.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(
        names,
        ["host", "token", "user"],
        "request scope ends with its send"
    );
    world.send("me.yml", &options).unwrap();
    assert!(
        world.last_sent().url().ends_with("t=t0k"),
        "{}",
        world.last_sent().url()
    );
}

#[test]
fn a_captured_secret_and_secrets_in_results_are_masked() {
    use apifast_domain::{Assertion, Capture, MASK, Operator};

    let mut request = get("{{host}}/echo?key={{api_key}}");
    request.captures = vec![Capture::new("res.body.new_key", "api_key")];
    request.assertions = vec![Assertion::new("res.body.sent", Operator::Eq, Some("x"))];
    let mut world = World::new(
        MemoryCollections::new(Collection::new("api")).with_request("echo.yml", request),
    );
    world.environments = MemoryEnvironments::new(vec![env(
        "prod",
        None,
        vec![
            Variable::text("host", "https://api.test"),
            Variable::secret("api_key"),
        ],
    )]);
    world.secrets = MemorySecrets::default().with("prod", "api_key", "k3y");
    // A server that echoes the secret and hands out a new one.
    world.transport = MemoryTransport::new(200, r#"{"sent":"k3y","new_key":"n3w"}"#);
    let options = SendOptions {
        environment: Some("prod".into()),
        ..SendOptions::default()
    };
    let outcome = world.send("echo.yml", &options).unwrap();
    let verdict = &outcome.assertions[0].verdict;
    assert_eq!(verdict.actual, format!("\"{MASK}\""));
    assert!(!verdict.message.as_deref().unwrap().contains("k3y"));
    assert!(outcome.captures[0].secret);
    let mut runtime = Vec::new();
    apifast_app::apply_captures(&mut runtime, &outcome.captures);
    assert!(runtime[0].secret, "it stays a secret for the next requests");
}

#[test]
fn a_request_without_checks_does_not_read_its_body_back() {
    let world = World::new(
        MemoryCollections::new(Collection::new("api")).with_request("a.yml", get("http://h/")),
    );
    let outcome = world.send("a.yml", &SendOptions::default()).unwrap();
    assert!(outcome.assertions.is_empty() && outcome.captures.is_empty());
}

/// `me.yml` takes its token from the response of `auth/login.yml`.
fn chained(refresh: &str) -> World {
    let mut me = get("http://h/me");
    me.headers = vec![Header::new(
        "Authorization",
        format!("Bearer {{{{ response(request='auth/login', path='$.token'{refresh}) }}}}"),
    )];
    let mut world = World::new(
        MemoryCollections::new(Collection::new("api"))
            .with_request(
                "auth/login.yml",
                HttpRequest::new("login", Method::Post, "http://h/login"),
            )
            .with_request("me.yml", me),
    );
    world.transport = MemoryTransport::new(200, r#"{"token":"t0k"}"#);
    world.history = Some(MemoryHistory::default());
    world
}

fn urls(world: &World) -> Vec<String> {
    world
        .transport
        .sent()
        .iter()
        .map(|r| r.url().to_owned())
        .collect()
}

#[test]
fn a_dependency_is_sent_first_then_its_saved_response_is_used() {
    let world = chained("");
    world.send("me.yml", &SendOptions::default()).unwrap();
    assert_eq!(urls(&world), ["http://h/login", "http://h/me"]);
    assert_eq!(header(&world.last_sent(), "authorization"), ["Bearer t0k"]);
    // The dependency is in the history, so `missing` does not send it again.
    let history = world.history.as_ref().unwrap().entries().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].request, RequestPath::new("auth/login.yml"));
    world.send("me.yml", &SendOptions::default()).unwrap();
    assert_eq!(
        urls(&world),
        ["http://h/login", "http://h/me", "http://h/me"]
    );
}

#[test]
fn refresh_policies_decide_when_to_send_again() {
    let mut world = chained(", refresh='ttl', ttl='300'");
    world.send("me.yml", &SendOptions::default()).unwrap();
    world.send("me.yml", &SendOptions::default()).unwrap();
    assert_eq!(urls(&world).len(), 3, "fresh enough: login once");
    world.clock = FixedClock(world.clock.0 + 301_000);
    world.send("me.yml", &SendOptions::default()).unwrap();
    assert_eq!(urls(&world).len(), 5, "too old: login again");

    let mut world = chained(", refresh='always'");
    // Used twice in one request: sent once for it.
    let mut me =
        get("http://h/me?a={{ response(request='auth/login', path='token', refresh='always') }}");
    me.headers = vec![Header::new(
        "x",
        "{{ response(request='auth/login', refresh='always') }}",
    )];
    world.collections = world.collections.with_request("me.yml", me);
    world.send("me.yml", &SendOptions::default()).unwrap();
    world.send("me.yml", &SendOptions::default()).unwrap();
    assert_eq!(
        urls(&world),
        [
            "http://h/login",
            "http://h/me?a=t0k",
            "http://h/login",
            "http://h/me?a=t0k"
        ]
    );
    assert_eq!(header(&world.last_sent(), "x"), [r#"{"token":"t0k"}"#]);

    let world = chained(", refresh='never'");
    let error = world.send("me.yml", &SendOptions::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`auth/login.yml` has no saved response"),
        "{error}"
    );
    assert_eq!(urls(&world), Vec::<String>::new());
}

#[test]
fn a_cycle_between_requests_is_refused() {
    let a = get("http://h/a?b={{ response(request='b') }}");
    let b = get("http://h/b?c={{ response(request='c') }}");
    let c = get("http://h/c?a={{ response(request='a') }}");
    let world = World::new(
        MemoryCollections::new(Collection::new("api"))
            .with_request("a.yml", a)
            .with_request("b.yml", b)
            .with_request("c.yml", c),
    );
    let error = world.send("a.yml", &SendOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "requests depend on each other: a.yml → b.yml → c.yml → a.yml"
    );
    assert_eq!(urls(&world), Vec::<String>::new());
}

#[test]
fn a_failing_dependency_names_itself() {
    let world = World::new(
        MemoryCollections::new(Collection::new("api"))
            .with_request("login.yml", get("{{host}}/login"))
            .with_request(
                "me.yml",
                get("http://h/me?t={{ response(request='login', path='$.ok') }}"),
            ),
    );
    let error = world.send("me.yml", &SendOptions::default()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "`login.yml`, used by response(): variable `host` is not defined"
    );
    // The path must be in the response.
    let mut world = chained("");
    world.collections = world.collections.with_request(
        "me.yml",
        get("http://h/me?t={{ response(request='auth/login', path='$.missing') }}"),
    );
    let error = world.send("me.yml", &SendOptions::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`$.missing` is not in the response of `auth/login.yml`"),
        "{error}"
    );
}
