//! The API scripts see, as in Bruno: `req`, `res`, `bru`, `test` and
//! `expect`.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use VariableScope::{Environment, Runtime};
use apifast_app::{
    CancelToken, ConsoleLine, RunnerRequest, Script, ScriptEngine, ScriptError, ScriptInput,
    ScriptLimits, ScriptOutcome, ScriptRequest, ScriptResponse, ScriptVariables, TestOutcome,
    VariableChange, VariableScope,
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
        },
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
        .run(script, input, &CancelToken::new())
        .await
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
