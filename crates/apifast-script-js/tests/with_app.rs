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

#[tokio::test]
async fn scripts_send_requests_and_run_others_of_the_collection() {
    let mut login = HttpRequest::new("Login", Method::Post, "{{host}}/login");
    login.scripts = vec![script(
        ScriptKind::AfterResponse,
        "bru.setVar('token', 't0k')",
    )];
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users");
    request.scripts = vec![
        script(
            ScriptKind::BeforeRequest,
            r"
            const ran = await bru.runRequest('login');
            console.log(ran.status, ran.data.name, bru.getVar('token'));
            const sent = await bru.sendRequest({ url: 'https://other.test/x', method: 'PUT', data: 'hi' });
            console.log(sent.status, sent.data.id);
            try { await bru.runRequest('users/get'); } catch (e) { console.log(e.message); }
            ",
        ),
        script(ScriptKind::Tests, "console.log(bru.getVar('token'))"),
    ];
    let collections = MemoryCollections::new(Collection::new("C"))
        .with_request("login.yml", login)
        .with_request("users/get.yml", request);
    let world = World::new(collections);
    let outcome = world.send("users/get.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        [
            "200 Ana t0k",
            "200 7",
            "requests depend on each other: users/get.yml → users/get.yml",
            "t0k",
        ]
    );
    let sent: Vec<_> = (world.transport.sent().iter())
        .map(|r| format!("{} {}", r.method().as_str(), r.url()))
        .collect();
    assert_eq!(
        sent,
        [
            "POST https://api.test/login",
            "PUT https://other.test/x",
            "GET https://api.test/users",
        ]
    );
    let token = outcome.scripts.variables.iter().find(|c| c.name == "token");
    assert!(
        token.is_some(),
        "the variables of the request it ran reach the run"
    );
}

#[tokio::test]
async fn scripts_read_and_change_the_cookie_jar() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users");
    request.scripts = vec![
        script(
            ScriptKind::BeforeRequest,
            r"
            const jar = bru.cookies.jar();
            await jar.setCookie('{{host}}', { key: 'sid', value: '1', path: '/' });
            await jar.setCookies('https://other.test', [{ key: 'o', value: '2' }]);
            await jar.setCookie('https://api.test', { key: 'bad', value: 'x', domain: 'other.test' });
            const other = await jar.getCookies('https://other.test');
            console.log(bru.cookies.get('sid'), bru.cookies.has('o'), bru.cookies.has('bad'), other.map((c) => c.key).join());
            await bru.cookies.upsert({ key: 'lang', value: 'en' });
            try { await jar.getCookies('nope'); } catch (e) { console.log(e.message); }
            await new Promise((done) => jar.getCookie('https://api.test/users', 'sid', (error, cookie) => {
                console.log(error, cookie.value, cookie.domain, cookie.path, cookie.expires);
                done();
            }));
            ",
        ),
        script(
            ScriptKind::Tests,
            r"
            const jar = bru.cookies.jar();
            console.log(bru.cookies.all().map((c) => c.key).sort().join(), bru.cookies.count());
            await bru.cookies.remove('lang');
            await jar.deleteCookies('https://other.test');
            console.log(bru.cookies.toString(), (await jar.getCookies('https://other.test')).length);
            ",
        ),
    ];
    let mut world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let cookies = std::sync::Arc::new(apifast_app::Cookies::new());
    world.transport = MemoryTransport::new(200, "{}").with_cookies(cookies.clone());
    let outcome = world.send("get.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        [
            "1 false false o",
            "`nope` is not a URL with a scheme and a host",
            "null 1 api.test / Infinity",
            "lang,sid 2",
            "sid=1 0",
        ]
    );
    let left: Vec<_> = (cookies.jar().cookies().iter())
        .map(|c| format!("{}={}", c.name, c.value))
        .collect();
    assert_eq!(left, ["sid=1"]);
}

#[tokio::test]
async fn a_cookie_set_without_await_is_set_before_the_script_ends() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users");
    request.scripts = vec![
        script(
            ScriptKind::BeforeRequest,
            "bru.cookies.jar().setCookie('https://api.test', 'sid', '1')",
        ),
        script(ScriptKind::Tests, "console.log(bru.cookies.get('sid'))"),
    ];
    let mut world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let cookies = std::sync::Arc::new(apifast_app::Cookies::new());
    world.transport = MemoryTransport::new(200, "{}").with_cookies(cookies);
    let outcome = world.send("get.yml").await.unwrap();
    assert_eq!(logged(&outcome), ["1"]);
}

#[tokio::test]
async fn the_next_requests_scripts_read_what_earlier_ones_changed_by_scope() {
    let mut first = HttpRequest::new("First", Method::Get, "{{host}}/a");
    first.seq = Some(1);
    first.scripts = vec![script(
        ScriptKind::AfterResponse,
        r"
        bru.setEnvVar('token', 'new');
        bru.deleteEnvVar('host');
        bru.setCollectionVar('level', 'changed');
        bru.setGlobalEnvVar('region', 'eu');
        ",
    )];
    let mut second = HttpRequest::new("Second", Method::Get, "https://api.test/b");
    second.seq = Some(2);
    second.scripts = vec![script(
        ScriptKind::BeforeRequest,
        r"console.log(bru.getEnvVar('token'), bru.hasEnvVar('host'), bru.getCollectionVar('level'), bru.getGlobalEnvVar('region'), bru.getAllGlobalEnvVars().region)",
    )];
    let mut collection = Collection::new("C");
    collection.defaults.variables = vec![Variable::text("level", "file")];
    let collections = MemoryCollections::new(collection)
        .with_request("first.yml", first)
        .with_request("second.yml", second);
    let report = World::new(collections).run().await;
    let second = report.steps[1].outcome.as_ref().unwrap();
    assert_eq!(logged(second), ["new false changed eu eu"]);
    let kept: Vec<_> = (report.script_changes.iter())
        .map(|c| (c.name.as_str(), c.value.is_some()))
        .collect();
    assert_eq!(
        kept,
        [
            ("token", true),
            ("host", false),
            ("level", true),
            ("region", true)
        ]
    );
}

#[tokio::test]
async fn a_form_set_by_a_script_goes_as_written_and_headers_off_are_listed() {
    use apifast_domain::{Body, FormField};
    let field = |name: &str, value: &str| FormField {
        name: name.into(),
        value: value.into(),
        enabled: true,
    };
    let mut request = HttpRequest::new("Post", Method::Post, "{{host}}/form");
    request.body = Body::FormUrlEncoded(vec![field("a b", "{{host}}"), field("c", "1")]);
    request.headers = vec![Header {
        enabled: false,
        ..Header::new("x-off", "request")
    }];
    request.scripts = vec![script(
        ScriptKind::BeforeRequest,
        r"
        console.log(req.getBody(), JSON.stringify(req.headerList.filter((h) => h.disabled)));
        req.setBody({ key: 'value with spaces', list: ['x', 'y'] });
        ",
    )];
    let mut folder = Folder::new("F");
    folder.defaults.headers = vec![
        Header {
            enabled: false,
            ..Header::new("X-Off", "folder")
        },
        Header {
            enabled: false,
            ..Header::new("x-folder", "only")
        },
    ];
    let collections = MemoryCollections::new(Collection::new("C"))
        .with_folder("f", folder)
        .with_request("f/post.yml", request);
    let world = World::new(collections);
    let outcome = world.send("f/post.yml").await.unwrap();
    assert_eq!(
        logged(&outcome),
        [
            r#"a%20b={{host}}&c=1 [{"key":"x-folder","value":"only","disabled":true},{"key":"x-off","value":"request","disabled":true}]"#
        ]
    );
    let sent = world.transport.sent().pop().unwrap();
    assert_eq!(
        sent.body(),
        &apifast_app::RenderedBody::Text("key=value%20with%20spaces&list=x&list=y".into())
    );
    let content_type: Vec<_> = (sent.headers().iter())
        .filter(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(content_type, ["application/x-www-form-urlencoded"]);
}

#[tokio::test]
async fn scripts_know_whether_a_run_sends_their_request() {
    let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/a");
    request.scripts = vec![script(
        ScriptKind::BeforeRequest,
        "console.log(req.getExecutionMode())",
    )];
    let world =
        World::new(MemoryCollections::new(Collection::new("C")).with_request("get.yml", request));
    let sent = world.send("get.yml").await.unwrap();
    let report = world.run().await;
    let ran = report.steps[0].outcome.as_ref().unwrap();
    assert_eq!(
        (logged(&sent), logged(ran)),
        (vec!["standalone".to_owned()], vec!["runner".to_owned()])
    );
}
