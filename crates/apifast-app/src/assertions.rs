//! The assertion evaluator (docs/08): what an expression is in a response,
//! and whether an operator holds.
//!
//! The expected value is written as text and converted to the type of the
//! actual value before comparing (`"200"` against the number `200`). When
//! that is not possible, the assertion fails with both types in its message.

use std::cmp::Ordering;
use std::fmt::Write as _;

use apifast_domain::{Assertion, Json, Operator};
use apifast_query::JsonPath;
use regex::RegexBuilder;

use crate::{RenderedBody, RenderedRequest, ResponseHead};

/// How much of a value a message shows.
const SHOWN: usize = 120;
/// The largest compiled regular expression, so a pattern cannot use
/// unbounded memory.
const REGEX_LIMIT: usize = 1 << 20;

/// What expressions read: a response, its body parsed once.
#[derive(Debug)]
pub struct ResponseFacts {
    status: u16,
    headers: Vec<(String, String)>,
    elapsed_ms: u64,
    size: u64,
    body: Body,
    /// The request as it was sent, for `req.*`.
    request: Option<SentRequest>,
}

/// A request as `req.*` expressions read it.
#[derive(Debug)]
struct SentRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Body,
}

#[derive(Debug)]
enum Body {
    /// Too large to read for assertions.
    NotLoaded,
    /// Its text, and its JSON when it is JSON.
    Loaded(String, Option<Json>),
}

/// Why an expression has no value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpressionError {
    /// Not one of the `res.*` or `req.*` expressions.
    #[error(
        "unknown expression `{0}`; use res.status, res.headers.<name>, res.body.<path>, req.body.<path>…"
    )]
    Unknown(String),
    /// A path into a body that is not JSON.
    #[error("the body is not JSON")]
    NotJson,
    /// The body was too large to read.
    #[error("the body is too large to check")]
    BodyTooLarge,
    /// A malformed path.
    #[error("invalid path `{0}`")]
    Path(String),
    /// A JSONPath that cannot be used, with why.
    #[error("{0}")]
    Query(String),
}

impl ResponseFacts {
    /// The facts of a response; `body` is `None` when it was too large to
    /// read. A body that is not UTF-8 is read lossily.
    pub fn new(head: &ResponseHead, size: u64, body: Option<&[u8]>) -> Self {
        Self {
            status: head.status,
            headers: head.headers.clone(),
            elapsed_ms: head.elapsed_ms,
            size,
            body: match body {
                None => Body::NotLoaded,
                Some(bytes) => Body::of(&String::from_utf8_lossy(bytes)),
            },
            request: None,
        }
    }

    /// These facts, and the request `sent` for `req.method`, `req.url`,
    /// `req.headers.<name>` and `req.body.<path>`. A body that is not text
    /// (multipart, a file) reads as empty.
    #[must_use]
    pub fn with_request(mut self, sent: &RenderedRequest) -> Self {
        let body = match sent.body() {
            RenderedBody::Text(text) => text.clone(),
            RenderedBody::Form(fields) => form_text(fields),
            _ => String::new(),
        };
        self.request = Some(SentRequest {
            method: sent.method().as_str().to_owned(),
            url: sent.url().to_owned(),
            headers: sent.headers().to_vec(),
            body: Body::of(&body),
        });
        self
    }

    /// The value of `expression`; `None` when it is not there (a missing
    /// header or field: `undefined`).
    ///
    /// # Errors
    ///
    /// If the expression is unknown or malformed, or reads into a body
    /// that is not JSON or was not read.
    pub fn value_of(&self, expression: &str) -> Result<Option<Json>, ExpressionError> {
        let expression = expression.trim();
        let number = |n: u64| Some(Json::Number(n.to_string()));
        match expression {
            "res.status" => return Ok(number(self.status.into())),
            "res.statusText" => {
                return Ok(Some(Json::String(reason_phrase(self.status).into())));
            }
            "res.responseTime" => return Ok(number(self.elapsed_ms)),
            "res.size" => return Ok(number(self.size)),
            "res.headers" => {
                let mut pairs: Vec<(String, Json)> = Vec::new();
                for (name, _) in &self.headers {
                    let name = name.to_ascii_lowercase();
                    if !pairs.iter().any(|(n, _)| *n == name) {
                        let value = self.header(&name).unwrap_or_default();
                        pairs.push((name, Json::String(value)));
                    }
                }
                return Ok(Some(Json::Object(pairs)));
            }
            _ => {}
        }
        if let Some(name) = expression.strip_prefix("res.headers.") {
            return Ok(self.header(name).map(Json::String));
        }
        if let Some(path) = expression.strip_prefix("res.body") {
            return self.body.value_at(path, expression);
        }
        if let Some(request) = &self.request {
            match expression {
                "req.method" => return Ok(Some(Json::String(request.method.clone()))),
                "req.url" => return Ok(Some(Json::String(request.url.clone()))),
                _ => {}
            }
            if let Some(name) = expression.strip_prefix("req.headers.") {
                return Ok(header_of(&request.headers, name).map(Json::String));
            }
            if let Some(path) = expression.strip_prefix("req.body") {
                return request.body.value_at(path, expression);
            }
        }
        Err(ExpressionError::Unknown(expression.to_owned()))
    }

    /// A header's values, joined by `, `; names ignore case.
    fn header(&self, name: &str) -> Option<String> {
        header_of(&self.headers, name)
    }
}

/// The values of header `name` in `headers`, joined by `, `; names ignore
/// case.
fn header_of(headers: &[(String, String)], name: &str) -> Option<String> {
    let values: Vec<&str> = (headers.iter())
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

impl Body {
    /// `text`, and its JSON when it is JSON.
    fn of(text: &str) -> Self {
        Self::Loaded(text.to_owned(), Json::parse(text).ok())
    }

    /// The value at `path` (after `res.body` or `req.body`) of `expression`.
    fn value_at(&self, path: &str, expression: &str) -> Result<Option<Json>, ExpressionError> {
        let (text, json) = match self {
            Self::NotLoaded => return Err(ExpressionError::BodyTooLarge),
            Self::Loaded(text, json) => (text, json),
        };
        if path.is_empty() {
            return Ok(Some(
                json.clone().unwrap_or_else(|| Json::String(text.clone())),
            ));
        }
        let json = json.as_ref().ok_or(ExpressionError::NotJson)?;
        if is_json_path(path) {
            return json_path(json, path, expression);
        }
        let path = path.strip_prefix('.').unwrap_or(path);
        if path.is_empty() {
            return Err(ExpressionError::Path(expression.to_owned()));
        }
        json.at(path)
            .map(Option::<&Json>::cloned)
            .map_err(|_| ExpressionError::Path(expression.to_owned()))
    }
}

/// A form's fields as they go, URL-encoded.
fn form_text(fields: &[(String, String)]) -> String {
    let encode = |text: &str| -> String {
        let mut out = String::with_capacity(text.len());
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                out.push(char::from(byte));
            } else {
                // Writing to a String cannot fail.
                let _ = write!(out, "%{byte:02X}");
            }
        }
        out
    };
    (fields.iter())
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Whether a path after `res.body` needs JSONPath: wildcards, descendants,
/// filters, slices, unions or negative indexes (`res.body.items[*].id`).
/// Others keep the simple reading, which also takes `items.0`.
fn is_json_path(path: &str) -> bool {
    ["*", "..", "?", ":", ",", "[-"]
        .iter()
        .any(|token| path.contains(token))
}

/// `res.body<path>` read as the JSONPath `$<path>`: undefined when it selects
/// nothing, the value when one, and an array of them when several.
fn json_path(json: &Json, path: &str, expression: &str) -> Result<Option<Json>, ExpressionError> {
    let query = JsonPath::parse(&format!("${path}"))
        .map_err(|_| ExpressionError::Path(expression.to_owned()))?;
    let mut values = query
        .select(json)
        .map_err(|e| ExpressionError::Query(e.to_string()))?;
    Ok(match values.len() {
        0 => None,
        1 => values.pop().cloned(),
        _ => Some(Json::Array(values.into_iter().cloned().collect())),
    })
}

/// The outcome of an assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether it held.
    pub passed: bool,
    /// The actual value, shortened, or `undefined`; empty when the
    /// expression has no value.
    pub actual: String,
    /// Why it failed.
    pub message: Option<String>,
}

impl Verdict {
    fn pass(actual: String) -> Self {
        Self {
            passed: true,
            actual,
            message: None,
        }
    }

    fn fail(actual: String, message: impl Into<String>) -> Self {
        Self {
            passed: false,
            actual,
            message: Some(message.into()),
        }
    }
}

/// Checks `assertion` against a response. `expected` is its value with the
/// variables already rendered.
pub fn check(assertion: &Assertion, expected: Option<&str>, facts: &ResponseFacts) -> Verdict {
    let actual = match facts.value_of(&assertion.expression) {
        Ok(actual) => actual,
        Err(error) => return Verdict::fail(String::new(), error.to_string()),
    };
    let shown = show(actual.as_ref());
    let expected = expected.unwrap_or("");
    if let Operator::Other(name) = &assertion.operator {
        return Verdict::fail(shown, format!("unknown operator `{name}`"));
    }
    if assertion.operator.takes_value() && expected.trim().is_empty() {
        return Verdict::fail(
            shown,
            format!("`{}` needs a value", assertion.operator.as_str()),
        );
    }
    match holds(&assertion.operator, actual.as_ref(), expected) {
        Ok(true) => Verdict::pass(shown),
        Ok(false) => {
            let message = if assertion.operator.takes_value() {
                format!(
                    "expected {} {expected}, got {shown}",
                    assertion.operator.as_str()
                )
            } else {
                format!("expected {}, got {shown}", assertion.operator.as_str())
            };
            Verdict::fail(shown, message)
        }
        Err(message) => Verdict::fail(shown, message),
    }
}

fn show(value: Option<&Json>) -> String {
    let Some(value) = value else {
        return "undefined".into();
    };
    let text = value.to_string();
    if text.chars().count() <= SHOWN {
        text
    } else {
        let mut short: String = text.chars().take(SHOWN).collect();
        short.push('…');
        short
    }
}

/// Whether `operator` holds; an error says why it cannot be decided.
fn holds(operator: &Operator, actual: Option<&Json>, expected: &str) -> Result<bool, String> {
    use Operator as O;
    let Some(value) = actual else {
        return undefined(operator, expected);
    };
    Ok(match operator {
        O::Eq => same(value, &convert(value, expected)?),
        O::Neq => !same(value, &convert(value, expected)?),
        O::Gt | O::Gte | O::Lt | O::Lte => {
            let order = number(value)?.partial_cmp(&expected_number(expected)?);
            match operator {
                O::Gt => order == Some(Ordering::Greater),
                O::Gte => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
                O::Lt => order == Some(Ordering::Less),
                _ => matches!(order, Some(Ordering::Less | Ordering::Equal)),
            }
        }
        O::Between => {
            let (low, high) = expected
                .split_once(',')
                .ok_or("`between` needs two values: `low,high`")?;
            let n = number(value)?;
            expected_number(low)? <= n && n <= expected_number(high)?
        }
        O::In => one_of(value, expected),
        O::NotIn => !one_of(value, expected),
        O::Contains => contains(value, expected)?,
        O::NotContains => !contains(value, expected)?,
        O::StartsWith => text(value)?.starts_with(unquote(expected).0),
        O::EndsWith => text(value)?.ends_with(unquote(expected).0),
        O::Matches => regex(expected)?.is_match(&text(value)?),
        O::NotMatches => !regex(expected)?.is_match(&text(value)?),
        O::Length => {
            let want: usize = expected
                .trim()
                .parse()
                .map_err(|_| format!("`length` needs a whole number, got {expected}"))?;
            length(value)? == want
        }
        O::IsEmpty => length(value)? == 0,
        O::IsNotEmpty => length(value)? != 0,
        O::IsNull => *value == Json::Null,
        O::IsUndefined => false,
        O::IsDefined => true,
        O::IsTruthy => truthy(value),
        O::IsFalsy => !truthy(value),
        O::IsJson => matches!(value, Json::Object(_) | Json::Array(_)),
        O::IsNumber => matches!(value, Json::Number(_)),
        O::IsString => matches!(value, Json::String(_)),
        O::IsBoolean => matches!(value, Json::Bool(_)),
        O::IsArray => matches!(value, Json::Array(_)),
        O::Other(name) => return Err(format!("unknown operator `{name}`")),
    })
}

/// Operators on a value that is not there.
fn undefined(operator: &Operator, expected: &str) -> Result<bool, String> {
    use Operator as O;
    let is_undefined = expected.trim() == "undefined";
    match operator {
        O::IsUndefined | O::IsFalsy | O::NotIn | O::NotContains | O::NotMatches => Ok(true),
        O::Eq => Ok(is_undefined),
        O::Neq => Ok(!is_undefined),
        O::Other(name) => Err(format!("unknown operator `{name}`")),
        _ => Ok(false),
    }
}

/// A quoted expected value is a string as it is: `"'200'"` is the text
/// `200`, never the number.
fn unquote(text: &str) -> (&str, bool) {
    let trimmed = text.trim();
    for quote in ['"', '\''] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return (&trimmed[1..trimmed.len() - 1], true);
        }
    }
    (text, false)
}

/// The expected text as a value of the actual value's type.
fn convert(actual: &Json, expected: &str) -> Result<Json, String> {
    let (inner, quoted) = unquote(expected);
    let trimmed = inner.trim();
    let mismatch = || {
        format!(
            "cannot compare {} {} with {expected}",
            actual.type_name(),
            show(Some(actual))
        )
    };
    if quoted {
        return Ok(Json::String(inner.to_owned()));
    }
    Ok(match actual {
        Json::String(_) => Json::String(inner.to_owned()),
        Json::Number(_) => {
            trimmed.parse::<f64>().map_err(|_| mismatch())?;
            Json::Number(trimmed.to_owned())
        }
        Json::Bool(_) => match trimmed {
            "true" => Json::Bool(true),
            "false" => Json::Bool(false),
            _ => return Err(mismatch()),
        },
        Json::Null => match trimmed {
            "null" => Json::Null,
            _ => return Err(mismatch()),
        },
        Json::Array(_) | Json::Object(_) => Json::parse(trimmed).map_err(|_| mismatch())?,
    })
}

/// Equal values: numbers by value, objects whatever the order of keys.
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(_), Json::Number(_)) => a.as_f64() == b.as_f64(),
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|other| same(v, other)))
        }
        _ => a == b,
    }
}

fn number(value: &Json) -> Result<f64, String> {
    match value {
        Json::Number(_) => value.as_f64(),
        Json::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .ok_or_else(|| {
        format!(
            "expected a number, got {} {}",
            value.type_name(),
            show(Some(value))
        )
    })
}

fn expected_number(text: &str) -> Result<f64, String> {
    unquote(text)
        .0
        .trim()
        .parse()
        .map_err(|_| format!("expected value {text} is not a number"))
}

/// A scalar as text, for the text operators.
fn text(value: &Json) -> Result<String, String> {
    match value {
        Json::Array(_) | Json::Object(_) => Err(format!(
            "expected text, got {} {}",
            value.type_name(),
            show(Some(value))
        )),
        scalar => Ok(scalar.to_text()),
    }
}

fn length(value: &Json) -> Result<usize, String> {
    match value {
        Json::String(text) => Ok(text.chars().count()),
        Json::Array(items) => Ok(items.len()),
        Json::Object(pairs) => Ok(pairs.len()),
        other => Err(format!(
            "{} {} has no length",
            other.type_name(),
            show(Some(other))
        )),
    }
}

/// Truthy as in JavaScript.
fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Number(_) => value.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Json::String(text) => !text.is_empty(),
        Json::Array(_) | Json::Object(_) => true,
    }
}

/// The items of an `in` list: a JSON array, or values separated by commas.
fn one_of(value: &Json, expected: &str) -> bool {
    let trimmed = expected.trim();
    if trimmed.starts_with('[')
        && let Ok(Json::Array(items)) = Json::parse(trimmed)
    {
        return items.iter().any(|item| match item {
            Json::String(text) => convert(value, text).is_ok_and(|v| same(value, &v)),
            other => same(value, other),
        });
    }
    expected
        .split(',')
        .any(|item| convert(value, item.trim()).is_ok_and(|v| same(value, &v)))
}

fn contains(value: &Json, expected: &str) -> Result<bool, String> {
    Ok(match value {
        Json::Array(items) => items
            .iter()
            .any(|item| convert(item, expected).is_ok_and(|v| same(item, &v))),
        Json::Object(_) => value.get(unquote(expected).0).is_some(),
        scalar => text(scalar)?.contains(unquote(expected).0),
    })
}

fn regex(pattern: &str) -> Result<regex::Regex, String> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_LIMIT)
        .build()
        .map_err(|e| format!("invalid regular expression: {e}"))
}

/// The standard reason phrase of a status code; empty for an unknown one.
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        422 => "Unprocessable Content",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(body: &str) -> ResponseFacts {
        let head = ResponseHead {
            status: 200,
            version: "HTTP/1.1".into(),
            url: "http://x/".into(),
            redirects: 0,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("Set-Cookie".into(), "a=1".into()),
                ("set-cookie".into(), "b=2".into()),
            ],
            elapsed_ms: 42,
            timeline: Vec::new(),
        };
        ResponseFacts::new(&head, body.len() as u64, Some(body.as_bytes()))
    }

    #[test]
    fn req_expressions_read_the_request_as_sent() {
        let sent = |body| {
            RenderedRequest::own(
                apifast_domain::Method::Post,
                "https://api.test/users".into(),
                vec![("X-Trace".into(), "1".into())],
                body,
                Vec::new(),
            )
        };
        let json = facts("{}").with_request(&sent(RenderedBody::Text(
            r#"{"flag":false,"n":0,"none":null,"list":[1,2]}"#.into(),
        )));
        let value = |f: &ResponseFacts, e: &str| f.value_of(e).unwrap().map(|j| j.to_string());
        assert_eq!(value(&json, "req.method").as_deref(), Some(r#""POST""#));
        assert_eq!(
            value(&json, "req.url").as_deref(),
            Some(r#""https://api.test/users""#)
        );
        assert_eq!(
            value(&json, "req.headers.x-trace").as_deref(),
            Some(r#""1""#)
        );
        assert_eq!(value(&json, "req.body.flag").as_deref(), Some("false"));
        assert_eq!(value(&json, "req.body.none").as_deref(), Some("null"));
        assert_eq!(value(&json, "req.body.list[1]").as_deref(), Some("2"));
        assert_eq!(value(&json, "req.body.nope"), None);
        let form = facts("{}").with_request(&sent(RenderedBody::Form(vec![(
            "a b".into(),
            "1&2".into(),
        )])));
        assert_eq!(
            value(&form, "req.body").as_deref(),
            Some(r#""a%20b=1%262""#)
        );
        assert!(matches!(
            facts("{}").value_of("req.body.x"),
            Err(ExpressionError::Unknown(_))
        ));
    }

    const BODY: &str = r#"{"id":7,"name":"Ana","ok":true,"none":null,"price":"9.5","items":[1,2,3],"tags":["a","b"],"user":{"id":1,"role":"admin"},"empty":"","zero":0}"#;

    fn passes(expression: &str, operator: &str, value: Option<&str>) -> Verdict {
        let assertion = Assertion::new(expression, Operator::parse(operator), value);
        check(&assertion, value, &facts(BODY))
    }

    #[test]
    fn expressions_read_the_response() {
        let f = facts(BODY);
        let text = |e: &str| f.value_of(e).unwrap().map(|v| v.to_text());
        assert_eq!(text("res.status").as_deref(), Some("200"));
        assert_eq!(text("res.statusText").as_deref(), Some("OK"));
        assert_eq!(text("res.responseTime").as_deref(), Some("42"));
        assert_eq!(
            text("res.size").as_deref(),
            Some(BODY.len().to_string().as_str())
        );
        assert_eq!(
            text(" res.headers.content-type ").as_deref(),
            Some("application/json")
        );
        assert_eq!(text("res.headers.SET-COOKIE").as_deref(), Some("a=1, b=2"));
        assert_eq!(text("res.headers.x-missing"), None);
        assert_eq!(text("res.body.user.role").as_deref(), Some("admin"));
        assert_eq!(text("res.body.items[1]").as_deref(), Some("2"));
        assert_eq!(text("res.body.nope"), None);
        assert_eq!(
            text("res.body").as_deref(),
            Some(Json::parse(BODY).unwrap().to_string().as_str())
        );
        assert!(
            text("res.headers")
                .unwrap()
                .contains(r#""set-cookie":"a=1, b=2""#)
        );
        assert!(matches!(
            f.value_of("req.url"),
            Err(ExpressionError::Unknown(_))
        ));
        assert!(matches!(
            f.value_of("res.body.items[1"),
            Err(ExpressionError::Path(_))
        ));
        // JSONPath when the path needs it.
        assert_eq!(text("res.body..x"), None);
        assert_eq!(text("res.body..role").as_deref(), Some("admin"));
        assert_eq!(text("res.body.items[*]").as_deref(), Some("[1,2,3]"));
        assert_eq!(text("res.body.items[-1]").as_deref(), Some("3"));
        assert_eq!(text("res.body.items[?@ > 1]").as_deref(), Some("[2,3]"));
        assert_eq!(text("res.body.items[0:1]").as_deref(), Some("1"));
        assert!(matches!(
            f.value_of("res.body.items[?@ >]"),
            Err(ExpressionError::Path(_))
        ));

        let plain = facts("hello");
        assert_eq!(
            plain.value_of("res.body").unwrap(),
            Some(Json::String("hello".into()))
        );
        assert_eq!(plain.value_of("res.body.a"), Err(ExpressionError::NotJson));
        let head = ResponseHead {
            status: 599,
            version: String::new(),
            url: String::new(),
            redirects: 0,
            headers: Vec::new(),
            elapsed_ms: 0,
            timeline: Vec::new(),
        };
        let big = ResponseFacts::new(&head, 1 << 40, None);
        assert_eq!(big.value_of("res.body"), Err(ExpressionError::BodyTooLarge));
        assert_eq!(
            big.value_of("res.size").unwrap(),
            Some(Json::Number("1099511627776".into()))
        );
        assert_eq!(
            big.value_of("res.statusText").unwrap(),
            Some(Json::String(String::new()))
        );
    }

    #[test]
    fn every_operator_passes_and_fails() {
        let cases: &[(&str, &str, Option<&str>, bool)] = &[
            ("res.status", "eq", Some("200"), true),
            ("res.status", "eq", Some("200.0"), true),
            ("res.status", "eq", Some("201"), false),
            ("res.status", "neq", Some("201"), true),
            ("res.body.name", "eq", Some("Ana"), true),
            ("res.body.name", "eq", Some("'Ana'"), true),
            ("res.body.ok", "eq", Some("true"), true),
            ("res.body.none", "eq", Some("null"), true),
            ("res.body.tags", "eq", Some(r#"["a","b"]"#), true),
            (
                "res.body.user",
                "eq",
                Some(r#"{"role":"admin","id":1.0}"#),
                true,
            ),
            ("res.body.missing", "eq", Some("undefined"), true),
            ("res.body.missing", "neq", Some("1"), true),
            ("res.status", "gt", Some("199"), true),
            ("res.status", "gt", Some("200"), false),
            ("res.status", "gte", Some("200"), true),
            ("res.responseTime", "lt", Some("500"), true),
            ("res.status", "lte", Some("199"), false),
            ("res.body.price", "gt", Some("9"), true),
            ("res.status", "between", Some("200, 299"), true),
            ("res.status", "between", Some("300,399"), false),
            ("res.status", "in", Some("200,201,204"), true),
            ("res.status", "in", Some("[200, 201]"), true),
            ("res.body.name", "in", Some(r#"["Ana", "Bo"]"#), true),
            ("res.status", "notIn", Some("400,500"), true),
            ("res.headers.content-type", "contains", Some("json"), true),
            ("res.body.items", "contains", Some("2"), true),
            ("res.body.tags", "contains", Some("c"), false),
            ("res.body.user", "contains", Some("role"), true),
            ("res.body.name", "notContains", Some("x"), true),
            ("res.body.name", "startsWith", Some("A"), true),
            ("res.body.name", "endsWith", Some("na"), true),
            ("res.body.name", "matches", Some("^A[a-z]+$"), true),
            ("res.body.name", "notMatches", Some("^\\d+$"), true),
            ("res.body.items", "length", Some("3"), true),
            ("res.body.name", "length", Some("3"), true),
            ("res.body.user", "length", Some("2"), true),
            ("res.body.empty", "isEmpty", None, true),
            ("res.body.items", "isEmpty", None, false),
            ("res.body.items", "isNotEmpty", None, true),
            ("res.body.none", "isNull", None, true),
            ("res.body.id", "isNull", None, false),
            ("res.body.missing", "isUndefined", None, true),
            ("res.body.none", "isUndefined", None, false),
            ("res.body.none", "isDefined", None, true),
            ("res.body.missing", "isDefined", None, false),
            ("res.body.id", "isTruthy", None, true),
            ("res.body.zero", "isTruthy", None, false),
            ("res.body.empty", "isFalsy", None, true),
            ("res.body.missing", "isFalsy", None, true),
            ("res.body.user", "isJson", None, true),
            ("res.body.name", "isJson", None, false),
            ("res.body.id", "isNumber", None, true),
            ("res.body.price", "isNumber", None, false),
            ("res.body.name", "isString", None, true),
            ("res.body.ok", "isBoolean", None, true),
            ("res.body.tags", "isArray", None, true),
            ("res.body.user", "isArray", None, false),
        ];
        for (expression, operator, value, expected) in cases {
            let verdict = passes(expression, operator, *value);
            assert_eq!(
                verdict.passed, *expected,
                "{expression} {operator} {value:?}: {verdict:?}"
            );
            assert_eq!(verdict.message.is_none(), *expected);
        }
    }

    #[test]
    fn failures_say_why_with_both_values() {
        let verdict = passes("res.status", "eq", Some("201"));
        assert_eq!(verdict.actual, "200");
        assert_eq!(verdict.message.as_deref(), Some("expected eq 201, got 200"));
        let verdict = passes("res.status", "eq", Some("abc"));
        assert_eq!(
            verdict.message.as_deref(),
            Some("cannot compare number 200 with abc")
        );
        let verdict = passes("res.body.name", "gt", Some("3"));
        assert_eq!(
            verdict.message.as_deref(),
            Some("expected a number, got string \"Ana\"")
        );
        let verdict = passes("res.body.id", "length", Some("1"));
        assert_eq!(verdict.message.as_deref(), Some("number 7 has no length"));
        let verdict = passes("res.body.name", "matches", Some("("));
        assert!(
            verdict
                .message
                .unwrap()
                .starts_with("invalid regular expression")
        );
        let verdict = passes("res.status", "isFuture", None);
        assert_eq!(
            verdict.message.as_deref(),
            Some("unknown operator `isFuture`")
        );
        let verdict = passes("res.status", "eq", None);
        assert_eq!(verdict.message.as_deref(), Some("`eq` needs a value"));
        let verdict = passes("res.nope", "isDefined", None);
        assert!(verdict.message.unwrap().starts_with("unknown expression"));
        let verdict = passes("res.body.missing", "isNull", None);
        assert_eq!(verdict.actual, "undefined");
        let long = facts(&format!("\"{}\"", "x".repeat(500)));
        let verdict = check(
            &Assertion::new("res.body", Operator::IsNumber, None),
            None,
            &long,
        );
        assert_eq!(verdict.actual.chars().count(), SHOWN + 1);
    }
}
