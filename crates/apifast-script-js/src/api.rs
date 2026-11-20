//! The API scripts see (`req`, `res`, `bru`, `test`, `expect`), written in
//! JavaScript (`api.js`, `expect.js`, `schema.js`, `header_list.js`,
//! `cookies.js`): it is given the input as JSON, and
//! what the script changed is read back as JSON.

use std::time::Duration;

use apifast_app::{
    RunnerRequest, ScriptInput, ScriptOutcome, ScriptRequest, TestOutcome, VariableChange,
    VariableScope,
};
use apifast_domain::{Cookie, SameSite, ValueKind, Variable, VariableValue};
use rquickjs::prelude::Async;
use rquickjs::{Ctx, Function, Promise};
use serde_json::{Value as Json, json};

use crate::host::{self, Calls};

const SCHEMA: &str = include_str!("schema.js");
const EXPECT: &str = include_str!("expect.js");
const HEADER_LIST: &str = include_str!("header_list.js");
const COOKIES: &str = include_str!("cookies.js");
const API: &str = include_str!("api.js");

/// Gives the script of `ctx` the API, over `input`.
pub(crate) fn install(ctx: &Ctx<'_>, input: &ScriptInput, calls: Calls) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    globals.set("__apifast_input", to_json(input).to_string())?;
    let sleep = Function::new(
        ctx.clone(),
        Async(|ms: u64| async move { tokio::time::sleep(Duration::from_millis(ms)).await }),
    )?;
    globals.set("__apifast_sleep", sleep)?;
    let process_env = Function::new(ctx.clone(), |name: String| std::env::var(name).ok())?;
    globals.set("__apifast_process_env", process_env)?;
    globals.set(
        "__apifast_dynamic",
        Function::new(ctx.clone(), |name: String| dynamic(&name))?,
    )?;
    let host = Function::new(
        ctx.clone(),
        Async(move |kind: String, payload: String| {
            let calls = calls.clone();
            async move { host::ask(&calls, &kind, &payload).await }
        }),
    )?;
    globals.set("__apifast_host", host)?;
    ctx.eval::<(), _>(SCHEMA)?;
    ctx.eval::<(), _>(EXPECT)?;
    ctx.eval::<(), _>(HEADER_LIST)?;
    ctx.eval::<(), _>(COOKIES)?;
    ctx.eval::<(), _>(API)
}

/// The value of dynamic variable `name` (`$randomInt`…), generated as
/// templates generate it; `None` when there is no such variable.
fn dynamic(name: &str) -> Option<String> {
    let bare = name.strip_prefix('$')?;
    if !apifast_template::DYNAMIC_VARIABLES.contains(&bare) {
        return None;
    }
    let variables = apifast_domain::ResolvedVariables::default();
    let template = format!("{{{{{name}}}}}");
    let mode = apifast_template::Mode::Lenient;
    let rendered = apifast_template::render(&template, &variables, &mut SystemHost, mode).ok()?;
    Some(rendered.text.into_owned())
}

/// Time and randomness for dynamic variables: the system's.
struct SystemHost;

impl apifast_template::Host for SystemHost {
    fn unix_millis(&mut self) -> i64 {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }

    fn random_u64(&mut self) -> u64 {
        // Each `RandomState` is seeded anew: enough for test data, not
        // for secrets.
        use std::hash::{BuildHasher, Hasher};
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    }
}

/// The promise of the tests the script started, all settled.
pub(crate) fn settle<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Promise<'js>> {
    let settle: Function = ctx.globals().get("__apifast_settle")?;
    settle.call(())
}

/// What the script changed, into `outcome`.
pub(crate) fn read_output(ctx: &Ctx<'_>, outcome: &mut ScriptOutcome) -> rquickjs::Result<()> {
    let output: Function = ctx.globals().get("__apifast_output")?;
    let text: String = output.call(())?;
    let Ok(json) = serde_json::from_str::<Json>(&text) else {
        return Ok(());
    };
    outcome.request = json.get("request").and_then(request);
    outcome.response_body = json
        .get("responseBody")
        .and_then(Json::as_str)
        .map(str::to_owned);
    outcome.variables = items(&json, "variables").filter_map(change).collect();
    outcome.tests = items(&json, "tests").filter_map(test).collect();
    outcome.runner = json.get("runner").and_then(runner);
    Ok(())
}

fn items<'j>(json: &'j Json, key: &str) -> impl Iterator<Item = &'j Json> {
    json.get(key).and_then(Json::as_array).into_iter().flatten()
}

fn to_json(input: &ScriptInput) -> Json {
    let variables = &input.variables;
    json!({
        "request": request_json(&input.request),
        "response": input.response.as_ref().map(|response| json!({
            "status": response.status,
            "status_text": response.status_text,
            "headers": response.headers,
            "body": response.body,
            "time_ms": response.time_ms,
            "size": response.size,
            "url": response.url,
        })),
        "variables": {
            "collection_name": variables.collection_name,
            "environment_name": variables.environment_name,
            "collection": variables_json(&variables.collection),
            "environment": variables_json(&variables.environment),
            "folders": variables_json(&variables.folders),
            "request": variables_json(&variables.request),
            "runtime": variables_json(&variables.runtime),
            "globals": variables_json(&variables.globals),
        },
        "cookies": input.cookies.iter().map(cookie_json).collect::<Vec<_>>(),
    })
}

fn request_json(request: &ScriptRequest) -> Json {
    json!({
        "name": request.name,
        "method": request.method,
        "url": request.url,
        "headers": request.headers,
        "body": request.body,
        "timeout_ms": request.timeout_ms,
        "tags": request.tags,
        "path_params": request.path_params,
        "disabled_headers": request.disabled_headers,
        "form": request.form,
        "in_run": request.in_run,
    })
}

/// The enabled variables that have a value, in order: a later one wins.
fn variables_json(variables: &[Variable]) -> Json {
    let values = variables.iter().filter(|v| v.enabled).filter_map(|v| {
        let value = v.value.as_ref()?;
        Some(json!({"name": v.name, "kind": kind_name(value.kind), "data": value.data}))
    });
    Json::Array(values.collect())
}

fn kind_name(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::String => "string",
        ValueKind::Number => "number",
        ValueKind::Bool => "bool",
        ValueKind::Object => "object",
    }
}

fn kind(name: &str) -> ValueKind {
    match name {
        "number" => ValueKind::Number,
        "bool" => ValueKind::Bool,
        "object" => ValueKind::Object,
        _ => ValueKind::String,
    }
}

fn text(json: &Json, key: &str) -> Option<String> {
    json.get(key).and_then(Json::as_str).map(str::to_owned)
}

fn request(json: &Json) -> Option<ScriptRequest> {
    let pairs = |key| {
        items(json, key)
            .filter_map(|pair| {
                Some((
                    pair.get(0)?.as_str()?.to_owned(),
                    pair.get(1)?.as_str()?.to_owned(),
                ))
            })
            .collect()
    };
    let tags = (items(json, "tags").filter_map(Json::as_str))
        .map(str::to_owned)
        .collect();
    Some(ScriptRequest {
        name: text(json, "name")?,
        method: text(json, "method")?,
        url: text(json, "url")?,
        headers: pairs("headers"),
        body: text(json, "body"),
        timeout_ms: json.get("timeout_ms").and_then(Json::as_u64),
        tags,
        path_params: pairs("path_params"),
        disabled_headers: pairs("disabled_headers"),
        form: json.get("form").and_then(Json::as_bool).unwrap_or(false),
        in_run: json.get("in_run").and_then(Json::as_bool).unwrap_or(false),
    })
}

/// A cookie of the jar, for a script to read.
pub(crate) fn cookie_json(cookie: &Cookie) -> Json {
    json!({
        "name": cookie.name,
        "value": cookie.value,
        "domain": cookie.domain,
        "host_only": cookie.host_only,
        "path": cookie.path,
        "secure": cookie.secure,
        "http_only": cookie.http_only,
        "same_site": cookie.same_site.map(SameSite::as_str),
        "expires_ms": cookie.expires_ms,
        "created_ms": cookie.created_ms,
    })
}

/// `change` as the script's output gives it, for a script to apply.
pub(crate) fn change_json(change: &VariableChange) -> Json {
    let scope = match change.scope {
        VariableScope::Runtime => "runtime",
        VariableScope::Environment => "environment",
        VariableScope::Collection => "collection",
        VariableScope::Global => "global",
    };
    let value = (change.value.as_ref())
        .map(|value| json!({"kind": kind_name(value.kind), "data": value.data}));
    json!({"scope": scope, "name": change.name, "value": value, "persist": change.persist})
}

fn change(json: &Json) -> Option<VariableChange> {
    let scope = match json.get("scope")?.as_str()? {
        "environment" => VariableScope::Environment,
        "collection" => VariableScope::Collection,
        "global" => VariableScope::Global,
        _ => VariableScope::Runtime,
    };
    let value = json
        .get("value")
        .filter(|v| !v.is_null())
        .and_then(|value| {
            Some(VariableValue {
                kind: kind(value.get("kind")?.as_str()?),
                data: text(value, "data")?,
            })
        });
    Some(VariableChange {
        scope,
        name: text(json, "name")?,
        value,
        persist: json.get("persist").and_then(Json::as_bool).unwrap_or(false),
    })
}

fn test(json: &Json) -> Option<TestOutcome> {
    Some(TestOutcome {
        name: text(json, "name")?,
        error: text(json, "error"),
    })
}

fn runner(json: &Json) -> Option<RunnerRequest> {
    match json {
        Json::String(what) if what == "skip" => Some(RunnerRequest::Skip),
        Json::String(what) if what == "stop" => Some(RunnerRequest::Stop),
        _ => text(json, "next").map(RunnerRequest::Next),
    }
}
