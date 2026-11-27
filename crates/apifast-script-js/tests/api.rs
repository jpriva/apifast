//! The API scripts see, as in Bruno: `req`, `res`, `bru`, `test` and
//! `expect`.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use VariableScope::{Environment, Runtime};
use apifast_app::{
    CancelToken, ConsoleLine, HostFuture, RanRequest, RunnerRequest, Script, ScriptEngine,
    ScriptError, ScriptHost, ScriptHttpRequest, ScriptHttpResponse, ScriptInput, ScriptLimits,
    ScriptOutcome, ScriptRequest, ScriptResponse, ScriptVariables, TestOutcome, VariableChange,
    VariableScope,
};
use apifast_domain::{ValueKind, Variable, VariableValue};
use apifast_script_js::QuickJs;

fn input() -> ScriptInput {
    let typed = |name: &str, kind, data: &str| Variable {
        value: Some(VariableValue {
            kind,
            data: data.into(),
        }),
        ..Variable::text(name, "")
    };
    ScriptInput {
        request: ScriptRequest {
            name: "Get user".into(),
            method: "POST".into(),
            url: "{{host}}/users".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(r#"{"name": "Ana"}"#.into()),
            timeout_ms: None,
            tags: vec!["smoke".into()],
            path_params: vec![("id".into(), "7".into())],
            disabled_headers: vec![("X-Off".into(), "hidden".into())],
            form: false,
            in_run: false,
        },
        response: Some(ScriptResponse {
            status: 201,
            status_text: "Created".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(r#"{"data": {"items": [{"id": 7}]}, "ok": true}"#.into()),
            time_ms: 42,
            size: 44,
            url: "https://api.test/users".into(),
        }),
        variables: ScriptVariables {
            collection_name: "Shop".into(),
            environment_name: Some("dev".into()),
            collection: vec![
                Variable::text("host", "https://api.test"),
                Variable::text("who", "collection"),
            ],
            environment: vec![
                Variable::text("token", "abc"),
                typed("retries", ValueKind::Number, "3"),
            ],
            folders: vec![Variable::text("who", "folder")],
            request: vec![typed("user", ValueKind::Object, r#"{"id": 1}"#)],
            runtime: vec![
                typed("seen", ValueKind::Bool, "true"),
                Variable::secret("hidden"),
            ],
            globals: vec![Variable::text("region", "eu")],
        },
        cookies: Vec::new(),
    }
}

async fn run_with(code: &str, input: &ScriptInput) -> ScriptOutcome {
    let script = Script {
        name: "request",
        code,
    };
    let limits = ScriptLimits {
        time_ms: 1_000,
        ..ScriptLimits::default()
    };
    QuickJs::new(limits)
        .run(script, input, &FakeHost, &CancelToken::new())
        .await
}

/// Answers `bru.sendRequest` with what it was sent (`/missing` is a 404,
/// `/down` no answer), and `bru.runRequest('login')` with a token it sets.
struct FakeHost;

impl ScriptHost for FakeHost {
    fn send_request(&self, request: ScriptHttpRequest) -> HostFuture<'_, ScriptHttpResponse> {
        Box::pin(async move {
            if request.url.ends_with("/down") {
                return Err("connection refused".to_owned());
            }
            let status = if request.url.ends_with("/missing") {
                404
            } else {
                200
            };
            let echo = serde_json::json!({
                "method": request.method,
                "url": request.url,
                "headers": request.headers,
                "body": request.body,
                "timeout": request.timeout_ms,
            });
            Ok(ScriptHttpResponse {
                status,
                status_text: if status == 200 { "OK" } else { "Not Found" }.into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: echo.to_string(),
                time_ms: 3,
                url: request.url,
            })
        })
    }

    fn run_request<'a>(&'a self, path: &'a str) -> HostFuture<'a, RanRequest> {
        Box::pin(async move {
            if path != "login" {
                return Err(format!("there is no request `{path}` in the collection"));
            }
            Ok(RanRequest {
                response: ScriptHttpResponse {
                    status: 200,
                    status_text: "OK".into(),
                    body: "pong".into(),
                    ..ScriptHttpResponse::default()
                },
                variables: vec![VariableChange {
                    scope: Runtime,
                    name: "token".into(),
                    value: Some(VariableValue::text("t0k")),
                    persist: false,
                }],
            })
        })
    }

    fn cookies(&self) -> Vec<apifast_domain::Cookie> {
        Vec::new()
    }

    fn change_cookies(
        &self,
        _: apifast_app::CookieChange,
    ) -> Result<Vec<apifast_domain::Cookie>, String> {
        Err("there is no cookie jar here".to_owned())
    }
}

async fn run(code: &str) -> ScriptOutcome {
    let outcome = run_with(code, &input()).await;
    assert_eq!(outcome.error, None, "{:?}", outcome.console);
    outcome
}

fn logged(outcome: &ScriptOutcome) -> Vec<&str> {
    outcome
        .console
        .iter()
        .map(|ConsoleLine { text, .. }| text.as_str())
        .collect()
}

#[tokio::test]
async fn req_reads_the_request_as_written() {
    let outcome = run(r"
        console.log(req.getName(), req.getMethod(), req.getUrl(), req.url);
        console.log(req.getHeader('content-type'), req.getHeaders());
        console.log(req.getBody().name, req.body.name, req.getTimeout());
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "Get user POST {{host}}/users {{host}}/users",
            r#"application/json {"Content-Type":"application/json"}"#,
            "Ana Ana undefined",
        ]
    );
    assert_eq!(outcome.request, None);
}

#[tokio::test]
async fn req_changes_the_request() {
    let outcome = run(r"
        req.setUrl(req.getUrl() + '?page=2');
        req.setMethod('put');
        req.setHeader('X-Trace', 1);
        req.setHeader('content-type', 'application/vnd+json');
        req.deleteHeader('nope');
        req.setBody({ ...req.getBody(), age: 30 });
        req.setTimeout(5000);
    ")
    .await;
    assert_eq!(
        outcome.request,
        Some(ScriptRequest {
            name: "Get user".into(),
            method: "PUT".into(),
            url: "{{host}}/users?page=2".into(),
            headers: vec![
                ("content-type".into(), "application/vnd+json".into()),
                ("X-Trace".into(), "1".into()),
            ],
            body: Some(r#"{"name":"Ana","age":30}"#.into()),
            timeout_ms: Some(5000),
            tags: vec!["smoke".into()],
            path_params: vec![("id".into(), "7".into())],
            disabled_headers: vec![("X-Off".into(), "hidden".into())],
            form: false,
            in_run: false,
        })
    );
}

#[tokio::test]
async fn res_reads_the_response() {
    let outcome = run(r"
        console.log(res.getStatus(), res.status, res.getStatusText(), res.statusText);
        console.log(res.getHeader('CONTENT-TYPE'), res.headers['content-type']);
        console.log(res.getBody().ok, res.body.data.items[0].id, res('data.items[0].id'), res('missing.deep'));
        console.log(res.getResponseTime(), res.responseTime, res.getUrl(), res.getSize().body);
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "201 201 Created Created",
            "application/json application/json",
            "true 7 7 undefined",
            "42 42 https://api.test/users 44",
        ]
    );
}

#[tokio::test]
async fn res_can_replace_the_body() {
    let outcome = run("res.setBody({ redacted: true }); console.log(res.body.redacted)").await;
    assert_eq!(logged(&outcome), ["true"]);
    assert_eq!(
        outcome.response_body.as_deref(),
        Some(r#"{"redacted":true}"#)
    );
}

#[tokio::test]
async fn a_script_before_the_request_has_no_res() {
    let before = ScriptInput {
        response: None,
        ..input()
    };
    let outcome = run_with("console.log(typeof res)", &before).await;
    assert_eq!(logged(&outcome), ["undefined"]);
}

#[tokio::test]
async fn bru_reads_variables_by_scope_with_their_types() {
    let outcome = run(r"
        console.log(bru.getCollectionName(), bru.getEnvName());
        console.log(bru.getEnvVar('token'), bru.getEnvVar('retries') + 1, bru.hasEnvVar('nope'));
        console.log(bru.getVar('seen') === true, bru.getVar('hidden'), bru.getRequestVar('user').id);
        console.log(bru.getFolderVar('who'), bru.getCollectionVar('who'), bru.getCollectionVar('host'));
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "Shop dev",
            "abc 4 false",
            "true undefined 1",
            "folder collection https://api.test",
        ]
    );
}

#[tokio::test]
async fn bru_sets_and_deletes_variables() {
    let outcome = run(r"
        bru.setVar('userId', 7);
        bru.setVar('user', { name: 'Ana' });
        bru.setVar('label', 'x');
        bru.deleteVar('seen');
        bru.setEnvVar('token', 'new', { persist: true });
        bru.setEnvVar('flag', false);
        console.log(bru.getVar('userId') + 1, bru.getVar('user').name, bru.hasVar('seen'), bru.getEnvVar('token'));
    ")
    .await;
    assert_eq!(logged(&outcome), ["8 Ana false new"]);
    let typed = |kind, data: &str| {
        Some(VariableValue {
            kind,
            data: data.into(),
        })
    };
    let change = |scope, name: &str, value, persist| VariableChange {
        scope,
        name: name.into(),
        value,
        persist,
    };
    assert_eq!(
        outcome.variables,
        [
            change(Runtime, "userId", typed(ValueKind::Number, "7"), false),
            change(
                Runtime,
                "user",
                typed(ValueKind::Object, r#"{"name":"Ana"}"#),
                false
            ),
            change(Runtime, "label", typed(ValueKind::String, "x"), false),
            change(Runtime, "seen", None, false),
            change(Environment, "token", typed(ValueKind::String, "new"), true),
            change(Environment, "flag", typed(ValueKind::Bool, "false"), false),
        ]
    );
}

#[tokio::test]
async fn a_variable_name_must_be_valid() {
    let outcome = run_with("bru.setVar('bad name', 1)", &input()).await;
    let Some(ScriptError::Thrown { message, .. }) = outcome.error else {
        panic!("{:?}", outcome.error);
    };
    assert!(message.contains("invalid variable name"), "{message}");
    assert_eq!(outcome.variables, []);
}

#[tokio::test]
async fn bru_interpolates_with_the_templates_precedence() {
    let outcome = run(r"
        bru.setVar('id', 9);
        console.log(bru.interpolate('{{host}}/users/{{ id }}?by={{who}}&u={{user}}&x={{unknown}}'));
        console.log(bru.interpolate({ auth: 'Bearer {{token}}' }).auth);
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            r#"https://api.test/users/9?by=folder&u={"id":1}&x={{unknown}}"#,
            "Bearer abc"
        ]
    );
}

#[tokio::test]
async fn bru_sleeps_and_counts_against_the_time_limit() {
    let started = Instant::now();
    let outcome = run("await bru.sleep(50); console.log('awake')").await;
    assert_eq!(logged(&outcome), ["awake"]);
    assert!(started.elapsed() >= Duration::from_millis(50));
    let slept = run_with("bru.setVar('a', 1); await bru.sleep(5000)", &input()).await;
    assert_eq!(slept.error, Some(ScriptError::TimedOut { ms: 1_000 }));
    // Stopped from outside: nothing it changed is kept.
    assert_eq!(slept.variables, []);
}

#[tokio::test]
async fn bru_steers_a_collection_run() {
    let next = run("bru.setNextRequest('Login')").await;
    assert_eq!(next.runner, Some(RunnerRequest::Next("Login".into())));
    let next = run("bru.runner.setNextRequest('Logout')").await;
    assert_eq!(next.runner, Some(RunnerRequest::Next("Logout".into())));
    assert_eq!(
        run("bru.runner.skipRequest()").await.runner,
        Some(RunnerRequest::Skip)
    );
    assert_eq!(
        run("bru.runner.stopExecution()").await.runner,
        Some(RunnerRequest::Stop)
    );
    assert_eq!(run("").await.runner, None);
}

#[tokio::test]
async fn bru_reads_the_process_environment() {
    let path = std::env::var("PATH").unwrap();
    let outcome = run("console.log(bru.getProcessEnv('PATH')); console.log(bru.getProcessEnv('APIFAST_NOT_SET_X'))").await;
    assert_eq!(logged(&outcome), [path.as_str(), "undefined"]);
}

fn passed(name: &str) -> TestOutcome {
    TestOutcome {
        name: name.into(),
        error: None,
    }
}

fn failed(name: &str, error: &str) -> TestOutcome {
    TestOutcome {
        name: name.into(),
        error: Some(error.into()),
    }
}

#[tokio::test]
async fn tests_pass_or_fail_with_chais_messages() {
    let outcome = run(r#"
        test("status is 201", () => expect(res.getStatus()).to.equal(201));
        test("status is 200", function () { expect(res.status).to.equal(200); });
        test("has items", () => expect(res.body.data.items).to.be.an('array').that.is.not.empty);
        test("waits", async () => { await bru.sleep(10); expect(res('ok')).to.be.true; });
        test("rejects", async () => { await bru.sleep(10); expect(res.body).to.have.property('id'); });
        test("throws a string", () => { throw 'plain'; });
    "#)
    .await;
    assert_eq!(
        outcome.tests,
        [
            passed("status is 201"),
            failed("status is 200", "expected 201 to equal 200"),
            passed("has items"),
            passed("waits"),
            failed(
                "rejects",
                r#"expected {"data":{"items":[{"id":7}]},"ok":true} to have property 'id'"#
            ),
            failed("throws a string", "plain"),
        ]
    );
}

#[tokio::test]
async fn what_a_failing_script_did_is_kept() {
    let outcome = run_with(
        "test('first', () => {}); bru.setVar('a', 1); undefined.x; test('never', () => {})",
        &input(),
    )
    .await;
    assert!(matches!(outcome.error, Some(ScriptError::Thrown { .. })));
    assert_eq!(outcome.tests, [passed("first")]);
    assert_eq!(outcome.variables.len(), 1);
}

/// Runs `expect` checks, one per line: `ok` for those that pass, the
/// message for those that fail.
async fn expectations(checks: &[&str]) -> Vec<String> {
    let code = checks.iter().fold(String::new(), |mut code, check| {
        let _ = writeln!(
            code,
            "try {{ {check}; console.log('ok') }} catch (e) {{ console.log(e.message) }}"
        );
        code
    });
    let outcome = run(&code).await;
    logged(&outcome).into_iter().map(str::to_owned).collect()
}

#[tokio::test]
async fn expect_covers_what_scripts_use() {
    let results = expectations(&[
        "expect(1).to.equal(1)",
        "expect({a: [1]}).to.eql({a: [1]})",
        "expect({a: 1}).to.deep.equal({a: 1})",
        "expect('abc').to.be.a('string')",
        "expect([1, 2]).to.be.an('array').with.lengthOf(2)",
        "expect('hello').to.include('ell')",
        "expect([1, 2, 3]).to.contain(2)",
        "expect([{a: 1}]).to.deep.include({a: 1})",
        "expect({a: 1, b: 2}).to.include({a: 1})",
        "expect(5).to.be.above(3).and.below(10)",
        "expect(5).to.be.at.least(5)",
        "expect(5).to.be.at.most(5)",
        "expect(5).to.be.within(1, 10)",
        "expect(1.5).to.be.closeTo(1.4, 0.2)",
        "expect({a: {b: 1}}).to.have.nested.property('a.b', 1)",
        "expect({a: 1}).to.have.property('a').that.equals(1)",
        "expect({a: 1}).to.have.own.property('a')",
        "expect('abc').to.match(/b/)",
        "expect({a: 1, b: 2}).to.have.all.keys('a', 'b')",
        "expect({a: 1, b: 2}).to.have.any.keys('a', 'z')",
        "expect({a: 1, b: 2}).to.contain.keys('a')",
        "expect([1, 2]).to.have.members([2, 1])",
        "expect([1, 2, 3]).to.include.members([2])",
        "expect(2).to.be.oneOf([1, 2])",
        "expect(new Date()).to.be.instanceOf(Date)",
        "expect(() => { throw new TypeError('bad') }).to.throw(TypeError, 'bad')",
        "expect(null).to.be.null",
        "expect(undefined).to.be.undefined",
        "expect(0).to.not.be.ok",
        "expect('x').to.exist",
        "expect([]).to.be.empty",
        "expect(4).to.satisfy(n => n % 2 === 0)",
        "expect(1).to.not.equal(2)",
    ])
    .await;
    let failed: Vec<&String> = results.iter().filter(|r| *r != "ok").collect();
    assert_eq!(failed, Vec::<&String>::new());
}

#[tokio::test]
async fn expect_fails_with_chais_messages() {
    let results = expectations(&[
        "expect(404).to.equal(200)",
        "expect(200).to.not.equal(200)",
        "expect({a: 1}).to.eql({a: 2})",
        "expect('x').to.be.a('number')",
        "expect([1]).to.be.an('object')",
        "expect('abc').to.include('z')",
        "expect(1).to.be.above(2)",
        "expect([1]).to.have.lengthOf(2)",
        "expect({a: 1}).to.have.property('a', 2)",
        "expect(undefined).to.exist",
        "expect(1, 'status').to.equal(2)",
        "expect.fail('nope')",
        "expect(() => {}).to.throw()",
    ])
    .await;
    assert_eq!(
        results,
        [
            "expected 404 to equal 200",
            "expected 200 to not equal 200",
            r#"expected {"a":1} to deeply equal {"a":2}"#,
            "expected 'x' to be a number",
            "expected [1] to be an object",
            "expected 'abc' to include 'z'",
            "expected 1 to be above 2",
            "expected [1] to have a length of 2 but got 1",
            r#"expected {"a":1} to have property 'a' of 2, but got 1"#,
            "expected undefined to exist",
            "status: expected 1 to equal 2",
            "nope",
            "expected [Function] to throw",
        ]
    );
}

#[tokio::test]
async fn a_bruno_script_runs_as_written() {
    let outcome = run(r#"
        const body = res.getBody();
        bru.setVar("userId", body.data.items[0].id);
        test("created", function () {
            expect(res.getStatus()).to.equal(201);
            expect(res.getHeader("content-type")).to.contain("json");
            expect(body).to.have.property("ok", true);
        });
        test("fast", () => expect(res.getResponseTime()).to.be.below(1000));
    "#)
    .await;
    assert_eq!(outcome.tests, [passed("created"), passed("fast")]);
    assert_eq!(outcome.variables[0].name, "userId");
}

#[tokio::test]
async fn req_reads_the_parts_of_its_url_tags_and_path_params() {
    let mut input = input();
    input.request.url = "{{host}}/users/:id?page=2#top".into();
    let outcome = run_with(
        r"
        console.log(req.getHost(), req.getPath(), req.getQueryString());
        console.log(JSON.stringify(req.getPathParams()), req.getTags());
        req.setUrl('https://User:pw@API.Test:443');
        console.log(req.getHost(), req.getPath(), req.getQueryString() === '');
    ",
        &input,
    )
    .await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        logged(&outcome),
        [
            "{{host}} /users/7 page=2",
            r#"[{"name":"id","value":"7","type":"path"}] ["smoke"]"#,
            "api.test / true",
        ]
    );
}

#[tokio::test]
async fn bru_sets_collection_variables_and_reads_every_environment_variable() {
    let outcome = run(r"
        bru.setCollectionVar('id', 9);
        console.log(bru.getCollectionVar('id'), bru.hasCollectionVar('id'), bru.hasCollectionVar('nope'));
        const all = bru.getAllCollectionVars();
        all.id = 'mutated';
        console.log(bru.getAllCollectionVars().id, Object.keys(bru.getAllEnvVars()).join());
        bru.deleteAllEnvVars();
        bru.deleteCollectionVar('id');
        console.log(bru.getEnvVar('token'), bru.getCollectionVar('id'), bru.isSafeMode());
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "9 true false",
            "9 token,retries",
            "undefined undefined true"
        ]
    );
    let collection: Vec<_> = (outcome.variables.iter())
        .filter(|c| c.scope == VariableScope::Collection)
        .map(|c| (c.name.as_str(), c.value.is_some()))
        .collect();
    assert_eq!(collection, [("id", true), ("id", false)]);
    let deleted: Vec<_> = (outcome.variables.iter())
        .filter(|c| c.scope == Environment && c.value.is_none())
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(deleted, ["token", "retries"]);
}

#[tokio::test]
async fn collection_and_global_variables_ask_to_be_kept_only_with_persist() {
    let outcome = run(r"
        bru.setCollectionVar('kept', 1, { persist: true });
        bru.setCollectionVar('for_now', 2);
        bru.setGlobalEnvVar('shared', 3, { persist: true });
        bru.setGlobalEnvVar('passing', 4);
    ")
    .await;
    assert_eq!(outcome.error, None);
    let persist: Vec<_> = (outcome.variables.iter())
        .map(|c| (c.scope, c.name.as_str(), c.persist))
        .collect();
    assert_eq!(
        persist,
        [
            (VariableScope::Collection, "kept", true),
            (VariableScope::Collection, "for_now", false),
            (VariableScope::Global, "shared", true),
            (VariableScope::Global, "passing", false),
        ]
    );
}

#[tokio::test]
async fn timers_fire_and_a_pending_timeout_is_waited_for() {
    let outcome = run(r"
        await new Promise((resolve) => setTimeout(() => { console.log('awaited'); resolve(); }, 10));
        // Long enough for the interval's three ticks of 1 ms, which take
        // about 47 ms where the clock ticks every 15.6 ms (Windows).
        setTimeout((what) => bru.setVar('late', what), 200, 'fired');
        const cleared = setTimeout(() => console.log('never'), 5);
        clearTimeout(cleared);
        let ticks = 0;
        const interval = setInterval(() => { ticks += 1; if (ticks === 3) { clearInterval(interval); console.log('ticked', ticks); } }, 1);
    ")
    .await;
    assert_eq!(logged(&outcome), ["awaited", "ticked 3"]);
    let late = outcome.variables.iter().find(|c| c.name == "late");
    assert_eq!(
        late.and_then(|c| c.value.as_ref()).map(|v| v.data.as_str()),
        Some("fired")
    );
}

#[tokio::test]
async fn a_failing_timeout_is_the_scripts_error_and_keeps_its_changes() {
    let outcome = run_with(
        r"
        bru.setVar('kept', 1);
        setTimeout(() => { throw new Error('boom'); }, 1);
    ",
        &input(),
    )
    .await;
    assert!(
        matches!(&outcome.error, Some(ScriptError::Thrown { message, .. }) if message.contains("boom")),
        "{:?}",
        outcome.error
    );
    assert_eq!(outcome.variables.len(), 1);
}

#[tokio::test]
async fn bru_interpolate_generates_dynamic_variables() {
    let outcome = run(r"
        const a = bru.interpolate('{{$randomInt}}');
        const b = bru.interpolate({ at: '{{$timestamp}}', id: '{{$randomUUID}}' });
        console.log(/^\d+$/.test(a), /^\d{10}$/.test(b.at), /^[0-9a-f-]{36}$/.test(b.id));
        console.log(bru.interpolate('{{$nope}}'));
    ")
    .await;
    assert_eq!(logged(&outcome), ["true true true", "{{$nope}}"]);
}

#[tokio::test]
async fn the_header_lists_read_and_write_headers_in_any_case() {
    let outcome = run(r"
        const list = req.headerList;
        console.log(list.get('content-type'), list.count(), list.has('CONTENT-TYPE', 'application/json'));
        console.log(JSON.stringify(list.one('Content-Type')), list.indexOf('content-type'));
        console.log(list.upsert('X-Trace', '1'), list.upsert('x-trace', '2'), list.get('X-TRACE'));
        list.add('Accept: */*');
        list.remove('content-type');
        console.log(JSON.stringify(list.toObject()), JSON.stringify(req.getHeaders()));
        console.log(JSON.stringify(res.headerList.all()));
        try { res.headerList.clear(); } catch (e) { console.log(e.message); }
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            // The header turned off counts, first.
            "application/json 2 true",
            r#"{"key":"Content-Type","value":"application/json"} 1"#,
            "true false 2",
            r#"{"X-Off":"hidden","x-trace":"2","Accept":"*/*"} {"x-trace":"2","Accept":"*/*"}"#,
            r#"[{"key":"Content-Type","value":"application/json"}]"#,
            "HeaderList is read-only (response headers cannot be modified)",
        ]
    );
    let headers = outcome.request.unwrap().headers;
    assert_eq!(
        headers,
        [
            ("x-trace".to_owned(), "2".to_owned()),
            ("Accept".to_owned(), "*/*".to_owned())
        ]
    );
}

#[tokio::test]
async fn json_body_checks_paths_and_values() {
    let outcome = run(r#"
        test('ok', () => {
            expect(res.getBody()).to.have.jsonBody();
            expect(res.getBody()).to.have.jsonBody('data.items[0].id', 7);
            expect(res.getBody()).to.have.jsonBody('["ok"]');
            expect(res.getBody()).to.not.have.jsonBody('data.items[1]');
            expect({ 'a.b': { c: 1 } }).to.have.jsonBody('["a.b"].c', 1);
        });
        test('fails', () => expect(res.getBody()).to.have.jsonBody('data.nope'));
    "#)
    .await;
    assert_eq!(
        outcome.tests,
        [
            TestOutcome {
                name: "ok".into(),
                error: None
            },
            TestOutcome {
                name: "fails".into(),
                error: Some("expected body to have nested property 'data.nope'".into())
            },
        ]
    );
}

#[tokio::test]
async fn json_schema_validates_draft_7_as_ajv() {
    let outcome = run("
        const user = { name: 'Ana', age: 30, zip: '62701', email: 'ana@x.io', at: '2024-01-15T10:30:00Z',
            ip: '::1', id: '550e8400-e29b-41d4-a716-446655440000', tags: ['a', 'b'] };
        const schema = {
            type: 'object',
            required: ['name', 'age'],
            properties: {
                name: { type: 'string', minLength: 2, pattern: '^[A-Z]' },
                age: { type: 'integer', minimum: 18, format: 'int32' },
                email: { type: 'string', format: 'email' },
                at: { type: 'string', format: 'date-time' },
                ip: { type: 'string', format: 'ipv6' },
                id: { type: 'string', format: 'uuid' },
                tags: { type: 'array', items: { enum: ['a', 'b'] }, uniqueItems: true },
            },
            anyOf: [{ required: ['email'] }, { required: ['phone'] }],
        };
        test('valid', () => expect(user).to.have.jsonSchema(schema));
        test('wrong', () => expect({ name: 'ana', age: 'x' }).to.not.have.jsonSchema(schema));
        test('extra', () => expect(user).to.not.have.jsonSchema({ type: 'object', additionalProperties: false }));
        test('coerce', () => {
            const zip = { type: 'object', properties: { zip: { type: 'integer' } } };
            expect(user).to.not.have.jsonSchema(zip);
            expect(user).to.have.jsonSchema(zip, { coerceTypes: true });
        });
        test('strict', () => {
            const custom = { type: 'object', properties: { name: { type: 'string', custom: true } } };
            expect(() => expect(user).to.have.jsonSchema(custom)).to.throw('JSON schema compile error');
            expect(user).to.have.jsonSchema(custom, { strict: false });
            expect(() => expect(user).to.have.jsonSchema({ type: 'nope' })).to.throw('JSON schema compile error');
        });
        test('ref', () => expect([1, 2]).to.have.jsonSchema({
            definitions: { n: { type: 'number' } }, type: 'array', items: { $ref: '#/definitions/n' },
        }));
        test('message', () => expect({}).to.have.jsonSchema({ required: ['id'] }));
    ")
    .await;
    let failed: Vec<_> = (outcome.tests.iter())
        .filter_map(|t| Some((t.name.as_str(), t.error.as_deref()?)))
        .collect();
    assert_eq!(
        failed,
        [(
            "message",
            r##"expected {} to match JSON schema, validation errors: [{"instancePath":"","schemaPath":"#/required","keyword":"required","params":{"missingProperty":"id"},"message":"must have required property 'id'"}]"##
        )]
    );
}

#[tokio::test]
async fn json_schema_formats_reject_what_ajv_rejects() {
    let outcome = run(r"
        const valid = (format, value) => {
            const errors = [];
            try { expect(value).to.have.jsonSchema({ type: 'string', format }); } catch (e) { errors.push(e); }
            return errors.length === 0;
        };
        console.log(valid('uri', 'https://a.test/x?y=1#z'), valid('uri', '/relative'), valid('uri-reference', '/relative/path'));
        console.log(valid('uri-reference', '[invalid'), valid('uri-template', 'https://a.test/{user}'), valid('uri-template', 'https://a.test/{x'));
        console.log(valid('date', '2024-02-30'), valid('date', '2024-02-29'), valid('time', '10:30:00Z'), valid('duration', 'P3Y6M4DT12H30M5S'));
        console.log(valid('ipv6', '::1'), valid('ipv6', '1::2::3'), valid('ipv4', '256.1.1.1'), valid('regex', '[invalid'));
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "true false true",
            "false true false",
            "false true true true",
            "true false false false",
        ]
    );
}

#[tokio::test]
async fn bru_send_request_answers_as_axios() {
    let outcome = run(r"
        const res = await bru.sendRequest({
            url: 'https://api.test/echo', method: 'post', params: { q: 'a b' },
            headers: { 'X-Id': 7 }, data: { name: 'Ana' }, timeout: 500,
        });
        console.log(res.status, res.statusText, res.headers['content-type'], res.data.method, res.data.url);
        console.log(JSON.stringify(res.data.headers), res.data.body, res.data.timeout);
        console.log((await bru.sendRequest('https://api.test/plain')).data.method);
        try { await bru.sendRequest('https://api.test/missing'); } catch (e) { console.log(e.message, e.status, e.response.status); }
        await bru.sendRequest('https://api.test/missing', (error, response) => console.log(error.status, response));
        await bru.sendRequest('https://api.test/ok', (error, response) => console.log(error, response.status));
        try { await bru.sendRequest('https://api.test/down'); } catch (e) { console.log(e.message); }
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "200 OK application/json POST https://api.test/echo?q=a%20b",
            r#"[["X-Id","7"],["Content-Type","application/json"]] {"name":"Ana"} 500"#,
            "GET",
            "Request failed with status code 404 404 404",
            "404 null",
            "null 200",
            "connection refused",
        ]
    );
}

#[tokio::test]
async fn bru_run_request_gives_its_response_and_the_variables_it_set() {
    let outcome = run(r"
        const res = await bru.runRequest('login');
        console.log(res.data, res.status, res.statusText, bru.getVar('token'));
        try { await bru.runRequest('nope'); } catch (e) { console.log(e.message); }
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            "pong 200 OK t0k",
            "there is no request `nope` in the collection"
        ]
    );
    assert_eq!(outcome.variables.len(), 1, "it reaches the caller too");
}

#[tokio::test]
async fn bru_reads_and_sets_global_variables() {
    let outcome = run(r"
        console.log(bru.getGlobalEnvVar('region'), bru.hasGlobalEnvVar('nope'), bru.interpolate('{{region}}'));
        bru.setGlobalEnvVar('zone', 2);
        const all = bru.getAllGlobalEnvVars();
        all.zone = 'mutated';
        console.log(bru.getAllGlobalEnvVars().zone, bru.getVar('zone'));
        bru.deleteAllGlobalEnvVars();
        console.log(Object.keys(bru.getAllGlobalEnvVars()).length);
    ")
    .await;
    assert_eq!(logged(&outcome), ["eu false eu", "2 undefined", "0"]);
    let scopes: Vec<_> = (outcome.variables.iter())
        .map(|c| (c.scope, c.name.as_str(), c.value.is_some()))
        .collect();
    assert_eq!(
        scopes,
        [
            (VariableScope::Global, "zone", true),
            (VariableScope::Global, "region", false),
            (VariableScope::Global, "zone", false),
        ]
    );
}

#[tokio::test]
async fn awaiting_a_test_waits_for_it_to_end() {
    let outcome = run(r"
        const order = [];
        await test('first', async () => { await bru.sleep(20); order.push('first'); });
        await test('second', () => { order.push('second'); });
        const failed = await test('failed', async () => { throw new Error('no'); });
        console.log(order.join(), failed, req.tags, req.getExecutionMode());
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [r#"first,second undefined ["smoke"] standalone"#]
    );
    assert_eq!(outcome.tests[2].error.as_deref(), Some("no"));
}

#[tokio::test]
async fn the_header_list_shows_the_headers_turned_off() {
    let outcome = run(r"
        const list = req.headerList;
        console.log(list.count(), JSON.stringify(list.find((h) => h.disabled)), list.get('x-off'));
        console.log(JSON.stringify(list.toObject(true)), list.filter((h) => !h.disabled).length);
        console.log(JSON.stringify(req.getHeaders()));
        list.remove('X-OFF');
        console.log(list.has('x-off'));
    ")
    .await;
    assert_eq!(
        logged(&outcome),
        [
            r#"2 {"key":"X-Off","value":"hidden","disabled":true} hidden"#,
            r#"{"Content-Type":"application/json"} 1"#,
            r#"{"Content-Type":"application/json"}"#,
            "false",
        ]
    );
}

#[tokio::test]
async fn a_form_body_is_set_encoded_as_in_bruno() {
    let mut input = input();
    input.request.form = true;
    input.request.body = Some("a=1".into());
    let outcome = run_with(
        r"
        console.log(req.getBody());
        req.setBody({ key: 'value with spaces', name: 'bruno', array: ['test', 'value'] });
        console.log(req.getBody());
        req.setBody([{ name: 'empty', value: '' }, { name: 'null', value: null }, { name: 'undefined' }, { name: '', value: 'k' }, { name: 'zero', value: 0 }]);
        console.log(req.getBody());
        req.setBody('raw=1');
        console.log(req.getBody());
    ",
        &input,
    )
    .await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        logged(&outcome),
        [
            "a=1",
            "key=value%20with%20spaces&name=bruno&array=test&array=value",
            "empty=&null=&undefined=&=k&zero=0",
            "raw=1",
        ]
    );
}
