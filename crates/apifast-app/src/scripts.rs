//! JavaScript scripts (docs/08, F9): the port of the engine that runs them.
//!
//! A script runs alone, with limits on its time and memory, and without
//! access to the file system or the network: what it can read and change
//! is what the engine is given, and what it asks of the application
//! through a [`ScriptHost`] (send a request, run one of the collection).
//! What it writes to the console is kept, also when it fails.

use std::future::Future;
use std::pin::Pin;

use apifast_domain::{Cookie, CookieSpec, Variable, VariableValue};

use crate::CancelToken;

/// How long a script may run and how much memory it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Wall-clock time, waits included, in milliseconds.
    pub time_ms: u64,
    /// Memory of the script's heap, in bytes.
    pub memory_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            time_ms: 10_000,
            memory_bytes: 64 << 20,
        }
    }
}

/// A script to run.
#[derive(Debug, Clone, Copy)]
pub struct Script<'s> {
    /// Where it comes from, for messages: `collection`, `users/folder.yml`.
    pub name: &'s str,
    /// Its source. It runs as the body of an `async` function, as in
    /// Bruno: it may `await`, and `return` ends it.
    pub code: &'s str,
}

/// The console method a line was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// `console.log`.
    Log,
    /// `console.info`.
    Info,
    /// `console.warn`.
    Warn,
    /// `console.error`.
    Error,
    /// `console.debug`.
    Debug,
}

/// A line a script wrote to the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// How it was written.
    pub level: ConsoleLevel,
    /// Its arguments, joined by spaces: strings as they are, other values
    /// as JSON.
    pub text: String,
}

/// Why a script did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    /// It does not parse.
    #[error("syntax error{}: {message}", at(*line))]
    Syntax {
        /// The parser's message.
        message: String,
        /// The line of the script, from 1, when known.
        line: Option<u32>,
    },
    /// It threw an exception, or a promise it awaited was rejected.
    #[error("{message}{}", at(*line))]
    Thrown {
        /// The error's name and message (`TypeError: …`), or the value
        /// thrown.
        message: String,
        /// The line of the script, from 1, when known.
        line: Option<u32>,
    },
    /// It ran longer than its limit.
    #[error("the script ran longer than {ms} ms")]
    TimedOut {
        /// The limit.
        ms: u64,
    },
    /// It used more memory than its limit.
    #[error("the script used more than {} MB of memory", bytes >> 20)]
    OutOfMemory {
        /// The limit.
        bytes: usize,
    },
    /// It was cancelled.
    #[error("the script was cancelled")]
    Cancelled,
    /// The engine could not run it.
    #[error("the script engine failed: {0}")]
    Engine(String),
}

fn at(line: Option<u32>) -> String {
    line.map(|line| format!(" at line {line}"))
        .unwrap_or_default()
}

/// A request as a script sees it, and changes: its texts unrendered, as
/// in the file, since a script runs before templates are rendered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptRequest {
    /// Its name (`req.getName()`).
    pub name: String,
    /// `GET`, `POST`…
    pub method: String,
    /// Its URL.
    pub url: String,
    /// Its enabled headers, in order.
    pub headers: Vec<(String, String)>,
    /// Its body's text, for a body that is text (JSON, XML, text…);
    /// `None` for none, or for one a script cannot read yet.
    pub body: Option<String>,
    /// Its timeout, in milliseconds, when set.
    pub timeout_ms: Option<u64>,
    /// Its tags (`req.getTags()`).
    pub tags: Vec<String>,
    /// Its enabled path parameters (`:name` in the URL), in order.
    pub path_params: Vec<(String, String)>,
}

/// A response as a script sees it (`res`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptResponse {
    /// Its status code.
    pub status: u16,
    /// Its reason phrase.
    pub status_text: String,
    /// Its headers, in order.
    pub headers: Vec<(String, String)>,
    /// Its body's text; `None` when too large to give to a script.
    pub body: Option<String>,
    /// How long it took, in milliseconds.
    pub time_ms: u64,
    /// Its body's size, in bytes.
    pub size: u64,
    /// The URL it came from.
    pub url: String,
}

/// The variables a script reads, by scope; secrets with a value only when
/// it was decrypted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptVariables {
    /// The collection's name (`bru.getCollectionName()`).
    pub collection_name: String,
    /// The active environment's name, if any.
    pub environment_name: Option<String>,
    /// The collection's variables.
    pub collection: Vec<Variable>,
    /// The active environment's.
    pub environment: Vec<Variable>,
    /// The folders', from the outermost: an inner one wins.
    pub folders: Vec<Variable>,
    /// The request's.
    pub request: Vec<Variable>,
    /// Those set while running (`bru.setVar`, captures).
    pub runtime: Vec<Variable>,
    /// The global ones (`bru.getGlobalEnvVar`): those scripts set, for as
    /// long as the run or the open collection lasts.
    pub globals: Vec<Variable>,
}

/// What a script is given: the request, the response once there is one,
/// and the variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptInput {
    /// The request.
    pub request: ScriptRequest,
    /// The response, for scripts that run after it.
    pub response: Option<ScriptResponse>,
    /// The variables.
    pub variables: ScriptVariables,
    /// The cookies in the collection's jar, which scripts read
    /// (`bru.cookies`); they change it through the [`ScriptHost`].
    pub cookies: Vec<Cookie>,
}

/// Where a variable a script changed lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableScope {
    /// Kept while running (`bru.setVar`).
    Runtime,
    /// The active environment (`bru.setEnvVar`).
    Environment,
    /// The collection (`bru.setCollectionVar`), for this run: the
    /// collection file is not changed.
    Collection,
    /// The global ones (`bru.setGlobalEnvVar`), for this run.
    Global,
}

/// A variable a script set or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableChange {
    /// Its scope.
    pub scope: VariableScope,
    /// Its name.
    pub name: String,
    /// Its new value; `None` when deleted.
    pub value: Option<VariableValue>,
    /// Whether to save it to the environment's file
    /// (`bru.setEnvVar(name, value, {persist: true})`).
    pub persist: bool,
}

/// A `test(name, fn)` of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestOutcome {
    /// Its name.
    pub name: String,
    /// Why it failed; `None` if it passed.
    pub error: Option<String>,
}

/// What a script asked of a collection run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerRequest {
    /// Go on with the request of this name (`bru.setNextRequest`).
    Next(String),
    /// Skip this request (`bru.runner.skipRequest()`).
    Skip,
    /// Stop the run (`bru.runner.stopExecution()`).
    Stop,
}

/// What running a script gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptOutcome {
    /// What it wrote to the console, in order.
    pub console: Vec<ConsoleLine>,
    /// Why it did not finish; `None` if it did. What it did before
    /// failing is kept.
    pub error: Option<ScriptError>,
    /// The request, if the script changed it.
    pub request: Option<ScriptRequest>,
    /// The response's body, if the script replaced it (`res.setBody`).
    pub response_body: Option<String>,
    /// The variables it set or deleted, in order.
    pub variables: Vec<VariableChange>,
    /// Its tests, in the order they were declared.
    pub tests: Vec<TestOutcome>,
    /// What it asked of a collection run, the last thing asked.
    pub runner: Option<RunnerRequest>,
}

/// What [`ScriptEngine::run`] gives: boxed, so the engine can be a
/// `dyn ScriptEngine`.
pub type ScriptFuture<'a> = Pin<Box<dyn Future<Output = ScriptOutcome> + Send + 'a>>;

/// A request a script sends on its own (`bru.sendRequest`), as it wrote
/// it: nothing of it is rendered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptHttpRequest {
    /// `GET`, `POST`…
    pub method: String,
    /// Its URL, query included.
    pub url: String,
    /// Its headers, in order.
    pub headers: Vec<(String, String)>,
    /// Its body's text, if it has one.
    pub body: Option<String>,
    /// Its timeout, in milliseconds; the default one when `None`.
    pub timeout_ms: Option<u64>,
}

/// A response a script gets from [`ScriptHost`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptHttpResponse {
    /// Its status code.
    pub status: u16,
    /// Its reason phrase.
    pub status_text: String,
    /// Its headers, in order.
    pub headers: Vec<(String, String)>,
    /// Its body's text (lossy UTF-8).
    pub body: String,
    /// How long it took, in milliseconds.
    pub time_ms: u64,
    /// The URL that answered, after redirects.
    pub url: String,
}

/// A request of the collection a script ran (`bru.runRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RanRequest {
    /// Its response.
    pub response: ScriptHttpResponse,
    /// The variables its scripts set or deleted, in order: the script that
    /// ran it sees them, as in Bruno.
    pub variables: Vec<VariableChange>,
}

/// What a [`ScriptHost`] gives back: a message when it failed.
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// What a script can ask of the application while it runs.
pub trait ScriptHost: Sync {
    /// Sends `request` as it is (`bru.sendRequest`). Any status is a
    /// response; an error is a request that got none.
    fn send_request(&self, request: ScriptHttpRequest) -> HostFuture<'_, ScriptHttpResponse>;

    /// Sends the request of the collection at `path` (relative to its
    /// root, with or without the file's extension), its scripts and all
    /// (`bru.runRequest`).
    fn run_request<'a>(&'a self, path: &'a str) -> HostFuture<'a, RanRequest>;

    /// The cookies in the jar, for the next script to read.
    fn cookies(&self) -> Vec<Cookie>;

    /// Changes the jar as `change` asks, and gives the cookies in it then.
    ///
    /// # Errors
    ///
    /// A message, when there is no jar or `change` names no valid URL.
    fn change_cookies(&self, change: CookieChange) -> Result<Vec<Cookie>, String>;
}

/// A change a script makes to the cookie jar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieChange {
    /// Sets a cookie for `url` (`jar.setCookie`, `bru.cookies.upsert`). A
    /// cookie a `Set-Cookie` from it could not set is left out.
    Set {
        /// Where it comes from.
        url: String,
        /// The cookie.
        cookie: CookieSpec,
    },
    /// Deletes the cookies that go to `url`: those called `name`, or all.
    /// Of several called `name`, the one at the URL's path goes, or else
    /// the first, as in Bruno.
    Delete {
        /// Where they go.
        url: String,
        /// Which.
        name: Option<String>,
    },
    /// Empties the jar.
    Clear,
}

/// A host for scripts that can ask the application nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoScriptHost;

impl ScriptHost for NoScriptHost {
    fn send_request(&self, _: ScriptHttpRequest) -> HostFuture<'_, ScriptHttpResponse> {
        Box::pin(std::future::ready(Err(
            "scripts cannot send requests here".to_owned()
        )))
    }

    fn run_request<'a>(&'a self, _: &'a str) -> HostFuture<'a, RanRequest> {
        Box::pin(std::future::ready(Err(
            "scripts cannot run requests here".to_owned()
        )))
    }

    fn cookies(&self) -> Vec<Cookie> {
        Vec::new()
    }

    fn change_cookies(&self, _: CookieChange) -> Result<Vec<Cookie>, String> {
        Err("there is no cookie jar here".to_owned())
    }
}

/// Runs JavaScript scripts.
pub trait ScriptEngine {
    /// Runs `script` with `input` until it ends, fails, exceeds its limits
    /// or `cancel` asks it to stop; what it asks of the application goes
    /// to `host`.
    fn run<'a>(
        &'a self,
        script: Script<'a>,
        input: &'a ScriptInput,
        host: &'a dyn ScriptHost,
        cancel: &'a CancelToken,
    ) -> ScriptFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_where_they_happened() {
        let thrown = ScriptError::Thrown {
            message: "Error: boom".into(),
            line: Some(3),
        };
        assert_eq!(thrown.to_string(), "Error: boom at line 3");
        let syntax = ScriptError::Syntax {
            message: "unexpected token".into(),
            line: None,
        };
        assert_eq!(syntax.to_string(), "syntax error: unexpected token");
        let memory = ScriptError::OutOfMemory { bytes: 64 << 20 };
        assert_eq!(
            memory.to_string(),
            "the script used more than 64 MB of memory"
        );
    }
}
