//! What runs after a response (docs/08): captures first, then assertions,
//! so an assertion can compare against a value just captured.

use apifast_domain::{
    Assertion, CaptureScope, HttpRequest, Json, ResolvedVariables, Secret, ValueKind, Variable,
    VariableValue, mask,
};
use apifast_template::{Host, Mode, render};

use crate::{ResponseFacts, Verdict, check};

/// The largest body read for captures and assertions. A larger one is
/// stored whole, but expressions on `res.body` fail with their reason.
pub const CHECK_LIMIT: u64 = 16 << 20;

/// A capture that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOutcome {
    /// The variable it sets.
    pub variable: String,
    /// Where the variable lives.
    pub scope: CaptureScope,
    /// The value, or why there is none.
    pub value: Result<VariableValue, String>,
    /// Whether the variable is a secret: its value is never shown.
    pub secret: bool,
}

/// An assertion that was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionOutcome {
    /// As written in the request.
    pub assertion: Assertion,
    /// Its expected value with the variables rendered, secrets masked.
    pub expected: Option<String>,
    /// The result, secrets masked.
    pub verdict: Verdict,
}

/// Whether a request has anything to run after its response.
pub(crate) fn has_checks(request: &HttpRequest) -> bool {
    request.assertions.iter().any(|a| a.enabled) || request.captures.iter().any(|c| c.enabled)
}

/// Runs the captures and then the assertions of `request`. Captured values
/// are added to `variables`, above every scope, for the assertions.
/// Everything shown is masked with `secrets` and the secrets it meets.
pub(crate) fn run_checks(
    request: &HttpRequest,
    facts: &ResponseFacts,
    variables: &mut ResolvedVariables,
    host: &mut dyn Host,
    secrets: &[Secret],
) -> (Vec<CaptureOutcome>, Vec<AssertionOutcome>) {
    let mut secrets = secrets.to_vec();
    let mut captures = Vec::new();
    // Bruno writes only `after-response` captures; other phases wait.
    for capture in request
        .captures
        .iter()
        .filter(|c| c.enabled && (c.phase.is_empty() || c.phase == "after-response"))
    {
        let value = match facts.value_of(&capture.expression) {
            Ok(Some(json)) => Ok(variable_value(&json)),
            Ok(None) => Err(format!("{} is undefined", capture.expression.trim())),
            Err(error) => Err(error.to_string()),
        };
        let secret = variables.get(&capture.variable).is_some_and(|v| v.secret);
        if let Ok(value) = &value {
            if secret {
                secrets.push(Secret::new(value.data.clone()));
            }
            variables.capture(&capture.variable, value.clone());
        }
        captures.push(CaptureOutcome {
            variable: capture.variable.clone(),
            scope: capture.scope.clone(),
            value,
            secret,
        });
    }

    let mut assertions = Vec::new();
    for assertion in request.assertions.iter().filter(|a| a.enabled) {
        let expected = match assertion.value.as_deref().map(|value| {
            render(value, variables, host, Mode::Strict).map(|rendered| {
                secrets.extend(rendered.secrets);
                rendered.text.into_owned()
            })
        }) {
            None => Ok(None),
            Some(Ok(text)) => Ok(Some(text)),
            Some(Err(error)) => Err(error.to_string()),
        };
        let (expected, mut verdict) = match expected {
            Ok(expected) => {
                let verdict = check(assertion, expected.as_deref(), facts);
                (expected, verdict)
            }
            Err(message) => (
                assertion.value.clone(),
                Verdict {
                    passed: false,
                    actual: String::new(),
                    message: Some(message),
                },
            ),
        };
        verdict.actual = mask(&verdict.actual, &secrets).into_owned();
        verdict.message = verdict.message.map(|m| mask(&m, &secrets).into_owned());
        assertions.push(AssertionOutcome {
            assertion: assertion.clone(),
            expected: expected.map(|e| mask(&e, &secrets).into_owned()),
            verdict,
        });
    }
    (captures, assertions)
}

/// A captured JSON value as a variable value: text as it is, numbers and
/// booleans with their kind, anything else as JSON (an object variable).
pub fn variable_value(json: &Json) -> VariableValue {
    let kind = match json {
        Json::String(_) => ValueKind::String,
        Json::Number(_) => ValueKind::Number,
        Json::Bool(_) => ValueKind::Bool,
        Json::Null | Json::Array(_) | Json::Object(_) => ValueKind::Object,
    };
    VariableValue {
        kind,
        data: json.to_text(),
    }
}

/// Keeps the captured values for the next requests: those of scope
/// `runtime`, `environment` and `collection` (the caller writes the last
/// two to their files when it wants to). Values of scope `request` end with their
/// send.
pub fn apply_captures(runtime: &mut Vec<Variable>, captures: &[CaptureOutcome]) {
    for capture in captures {
        let Ok(value) = &capture.value else { continue };
        if !matches!(
            capture.scope,
            CaptureScope::Runtime | CaptureScope::Environment | CaptureScope::Collection
        ) {
            continue;
        }
        runtime.retain(|v| v.name != capture.variable);
        runtime.push(Variable {
            name: capture.variable.clone(),
            value: Some(value.clone()),
            enabled: true,
            secret: capture.secret,
        });
    }
}
