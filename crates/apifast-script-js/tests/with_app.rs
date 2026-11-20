//! Sending with scripts: those of the collection, the folders and the
//! request run in Bruno's sandwich order, with `QuickJS`, over the in-memory
//! ports.
#![allow(clippy::unwrap_used, reason = "tests")]

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryResponses,
    MemorySecrets, MemoryTransport, RecordedPauses, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, RequestPath, RunOptions, RunReport, ScriptError, ScriptSource,
    SendOptions, SendOutcome, SendPorts, run_collection, send_request,
};
use apifast_domain::{
    Assertion, Collection, Environment, Folder, Header, HttpRequest, Method, Operator, ScriptBlock,
    ScriptKind, Variable, VariableValue,
};
use apifast_script_js::QuickJs;

struct World {
    collections: MemoryCollections,
    environments: MemoryEnvironments,
    transport: MemoryTransport,
    responses: MemoryResponses,
    engine: QuickJs,
    clock: FixedClock,
    random: CountingRandom,
    prompter: ScriptedPrompter,
    secrets: MemorySecrets,
}

fn script(kind: ScriptKind, code: &str) -> ScriptBlock {
    ScriptBlock::new(kind, code)
}

/// A collection whose levels log their name in each phase, and whose
/// folder adds a header.
fn sandwich() -> MemoryCollections {
    let logs = |level: &str| {
        vec![
            script(
                ScriptKind::BeforeRequest,
                &format!("console.log('before {level}')"),
            ),
            script(
                ScriptKind::AfterResponse,
                &format!("console.log('after {level}')"),
            ),
            script(ScriptKind::Tests, &format!("console.log('tests {level}')")),
        ]
    };
    let mut collection = Collection::new("Shop");
    collection.defaults.scripts = logs("collection");
    collection.defaults.headers = vec![Header::new("X-Collection", "1")];
    let mut outer = Folder::new("Users");
    outer.defaults.scripts = logs("users");
    let mut inner = Folder::new("Admin");
    inner.defaults.scripts = logs("admin");
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users");
    request.scripts = logs("request");
    MemoryCollections::new(collection)
        .with_folder("users", outer)
        .with_folder("users/admin", inner)
        .with_request("users/admin/get.yml", request)
}

impl World {
    fn new(collections: MemoryCollections) -> Self {
        Self {
            collections,
            environments: MemoryEnvironments::new(vec![Environment {
                name: "dev".into(),
                color: None,
                extends: None,
                variables: vec![Variable::text("host", "https://api.test")],
            }]),
            transport: MemoryTransport::new(200, r#"{"id": 7, "name": "Ana"}"#),
            responses: MemoryResponses::default(),
            engine: QuickJs::default(),
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
            scripts: Some(&self.engine),
        }
    }

    async fn send(&self, path: &str) -> Result<SendOutcome, AppError> {
        let options = SendOptions {
            environment: Some("dev".into()),
            ..SendOptions::default()
        };
        send_request(
            self.ports(),
            &RequestPath::new(path),
            &options,
            &CancelToken::new(),
        )
        .await
    }

    async fn run(&self) -> RunReport {
        let options = RunOptions {
            send: SendOptions {
                environment: Some("dev".into()),
                ..SendOptions::default()
            },
            ..RunOptions::default()
        };
        let ports = self.ports();
        let pause = RecordedPauses::default();
        run_collection(ports, &pause, &options, &CancelToken::new(), &|_| {})
            .await
            .unwrap()
    }
}

fn logged(outcome: &SendOutcome) -> Vec<String> {
    (outcome.scripts.runs.iter())
        .flat_map(|run| &run.console)
        .map(|line| line.text.clone())
        .collect()
}

#[tokio::test]
async fn scripts_run_in_sandwich_order() {
    let world = World::new(sandwich());
    let outcome = world.send("users/admin/get.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        [
            "before collection",
            "before users",
            "before admin",
            "before request",
            "after request",
            "after admin",
            "after users",
            "after collection",
            "tests request",
            "tests admin",
            "tests users",
            "tests collection",
        ]
    );
    let sources: Vec<ScriptSource> = (outcome.scripts.runs.iter().take(4))
        .map(|run| run.source.clone())
        .collect();
    assert_eq!(
        sources,
        [
            ScriptSource::Collection,
            ScriptSource::Folder("Users".into()),
            ScriptSource::Folder("Admin".into()),
            ScriptSource::Request,
        ]
    );
}

#[tokio::test]
async fn scripts_before_the_request_change_it_before_it_is_rendered() {
    let mut request = HttpRequest::new("Create", Method::Get, "{{host}}/users");
    request.headers = vec![Header::new("X-Drop", "1")];
    request.scripts = vec![script(
        ScriptKind::BeforeRequest,
        r"
        console.log(req.getUrl(), req.getHeader('x-collection'));
        req.setMethod('POST');
        req.setUrl(req.getUrl() + '/{{id}}');
        req.setHeader('Authorization', 'Bearer {{token}}');
        req.deleteHeader('x-drop');
        req.setBody({ name: 'Ana' });
        bru.setVar('id', 42);
        bru.setVar('token', 'abc');
        ",
    )];
    let mut collection = Collection::new("Shop");
    collection.defaults.headers = vec![Header::new("X-Collection", "1")];
    let world = World::new(MemoryCollections::new(collection).with_request("create.yml", request));
    let outcome = world.send("create.yml").await.unwrap();
    assert_eq!(logged(&outcome), ["{{host}}/users 1"]);
    let sent = world.transport.sent().pop().unwrap();
    assert_eq!(sent.method(), &Method::Post);
    assert_eq!(sent.url(), "https://api.test/users/42");
    let headers: Vec<(&str, &str)> = (sent.headers().iter())
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    assert!(
        headers.contains(&("Authorization", "Bearer abc")),
        "{headers:?}"
    );
    assert!(headers.contains(&("X-Collection", "1")), "{headers:?}");
    assert!(
        !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("x-drop"))
    );
    let names: Vec<&str> = (outcome.scripts.variables.iter())
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["id", "token"]);
}

#[tokio::test]
async fn a_failing_script_before_the_request_stops_it() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/x");
    request.scripts = vec![script(ScriptKind::BeforeRequest, "\nnull.x")];
    let world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let error = world.send("get.yml").await.unwrap_err();
    assert!(matches!(error, AppError::Script(_)));
    assert_eq!(
        error.to_string(),
        "the request script before the request failed: TypeError: cannot read property 'x' of null at line 2"
    );
    assert_eq!(world.transport.sent().len(), 0);
}

#[tokio::test]
async fn scripts_after_the_response_read_it_and_their_variables_reach_the_assertions() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users/7");
    request.scripts = vec![
        script(
            ScriptKind::AfterResponse,
            "bru.setVar('name', res.body.name)",
        ),
        script(
            ScriptKind::Tests,
            r#"
            test("is 200", () => expect(res.getStatus()).to.equal(200));
            test("is 201", () => expect(res.status).to.equal(201));
            "#,
        ),
    ];
    request.assertions = vec![Assertion {
        expression: "res.body.name".into(),
        operator: Operator::Eq,
        value: Some("{{name}}".into()),
        enabled: true,
    }];
    let world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let outcome = world.send("get.yml").await.unwrap();
    assert!(
        outcome.assertions[0].verdict.passed,
        "{:?}",
        outcome.assertions
    );
    let tests: Vec<(&str, Option<&str>)> = (outcome.scripts.tests())
        .map(|t| (t.name.as_str(), t.error.as_deref()))
        .collect();
    assert_eq!(
        tests,
        [
            ("is 200", None),
            ("is 201", Some("expected 200 to equal 201"))
        ]
    );
    assert!(!outcome.scripts.passed());
}

#[tokio::test]
async fn an_error_after_the_response_is_reported_and_the_send_succeeds() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/x");
    request.scripts = vec![script(ScriptKind::AfterResponse, "throw new Error('late')")];
    let world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let outcome = world.send("get.yml").await.unwrap();
    assert_eq!(outcome.head.status, 200);
    assert_eq!(
        outcome.scripts.runs[0].error,
        Some(ScriptError::Thrown {
            message: "Error: late".into(),
            line: Some(1)
        })
    );
}

#[tokio::test]
async fn scripts_read_variables_by_scope() {
    let mut collection = Collection::new("Shop");
    collection.defaults.variables = vec![Variable::text("level", "collection")];
    let mut folder = Folder::new("F");
    folder.defaults.variables = vec![Variable::text("level", "folder")];
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/x");
    request.variables = vec![Variable {
        value: Some(VariableValue {
            kind: apifast_domain::ValueKind::Number,
            data: "3".into(),
        }),
        ..Variable::text("count", "")
    }];
    request.scripts = vec![script(
        ScriptKind::BeforeRequest,
        "console.log(bru.getEnvName(), bru.getEnvVar('host'), bru.getCollectionVar('level'), bru.getFolderVar('level'), bru.getRequestVar('count') + 1, bru.getCollectionName())",
    )];
    let collections = MemoryCollections::new(collection)
        .with_folder("f", folder)
        .with_request("f/get.yml", request);
    let outcome = World::new(collections).send("f/get.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        ["dev https://api.test collection folder 4 Shop"]
    );
}

#[tokio::test]
async fn a_run_carries_script_variables_and_counts_their_tests() {
    let mut login = HttpRequest::new("Login", Method::Post, "{{host}}/login");
    login.seq = Some(1);
    login.scripts = vec![script(
        ScriptKind::AfterResponse,
        "bru.setVar('userId', res.body.id)",
    )];
    let mut get = HttpRequest::new("Get", Method::Get, "{{host}}/users/{{userId}}");
    get.seq = Some(2);
    get.scripts = vec![script(
        ScriptKind::Tests,
        "test('ok', () => {}); test('bad', () => expect(1).to.equal(2))",
    )];
    let collections = MemoryCollections::new(Collection::new("C"))
        .with_request("login.yml", login)
        .with_request("get.yml", get);
    let world = World::new(collections);
    let report = world.run().await;
    assert_eq!(world.transport.sent()[1].url(), "https://api.test/users/7");
    let summary = report.summary();
    assert_eq!((summary.tests_passed, summary.tests_failed), (1, 1));
    assert_eq!((summary.passed, summary.failed), (1, 1));
    assert!(!report.succeeded());
    assert!(report.runtime.iter().any(|v| v.name == "userId"));
}

#[tokio::test]
async fn scripts_steer_a_run() {
    let request = |name: &str, seq: u32, code: &str| {
        let mut request = HttpRequest::new(name, Method::Get, "{{host}}/x");
        request.seq = Some(seq);
        request.scripts = vec![script(ScriptKind::BeforeRequest, code)];
        request
    };
    let collections = MemoryCollections::new(Collection::new("C"))
        .with_request("a.yml", request("A", 1, "bru.setNextRequest('C')"))
        .with_request("b.yml", request("B", 2, ""))
        .with_request("c.yml", request("C", 3, "bru.runner.skipRequest()"))
        .with_request("d.yml", request("D", 4, "bru.runner.stopExecution()"))
        .with_request("e.yml", request("E", 5, ""));
    let report = World::new(collections).run().await;
    let ran: Vec<&str> = report.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(ran, ["A", "D"]);
    let skipped: Vec<(&str, &str)> = (report.skipped.iter())
        .map(|s| (s.name.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(skipped, [("C", "a script skipped the request")]);
    assert!(report.stopped);
}

#[tokio::test]
async fn scripts_after_the_response_read_the_request_as_sent() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users/:id");
    request.tags = vec!["smoke".into()];
    request.params = vec![apifast_domain::Param {
        name: "id".into(),
        value: "7".into(),
        kind: apifast_domain::ParamKind::Path,
        enabled: true,
    }];
    request.scripts = vec![
        script(
            ScriptKind::BeforeRequest,
            "console.log(req.getUrl(), req.getPath(), req.getTags()); bru.setCollectionVar('seen', 1);",
        ),
        script(
            ScriptKind::Tests,
            "console.log(req.getUrl(), req.getHost(), req.getTags(), bru.getCollectionVar('seen'))",
        ),
    ];
    let world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let outcome = world.send("get.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        [
            r#"{{host}}/users/:id /users/7 ["smoke"]"#,
            r#"https://api.test/users/7 api.test ["smoke"] 1"#,
        ]
    );
}
