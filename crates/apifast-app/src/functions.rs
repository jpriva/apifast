//! The template functions `response` and `header` (docs/05): a value from
//! the response of another request, which is sent first when needed.
//!
//! Rendering is synchronous and sending is not, so the renderer only asks:
//! a call whose response is not at hand is noted in
//! [`ResponseFunctions::needed`] and fails this round. The send use case
//! then sends what was noted and renders again.

use std::collections::BTreeMap;
use std::sync::Arc;

use apifast_domain::Json;
use apifast_query::{Language, QueryError};
use apifast_template::FunctionArg;

use crate::checks::CHECK_LIMIT;
use crate::{HistoryEntry, HistoryOutcome, HistoryStore, RequestPath, ResponseHead, ResponseId};
use crate::{SendOutcome, StoreError};

/// Reads a stored body: its id and how many bytes.
pub(crate) type ReadBody<'a> =
    &'a (dyn Fn(&ResponseId, usize) -> Result<Vec<u8>, StoreError> + Sync);

/// A response other requests can read from.
#[derive(Debug)]
pub(crate) struct Fetched {
    /// When it was received.
    at_ms: i64,
    head: ResponseHead,
    /// Its text; `None` when it was too large to read.
    body: Option<String>,
    json: Option<Json>,
}

impl Fetched {
    fn new(at_ms: i64, head: ResponseHead, body: Option<&[u8]>) -> Self {
        let body = body.map(|bytes| String::from_utf8_lossy(bytes).into_owned());
        let json = body.as_deref().and_then(|text| Json::parse(text).ok());
        Self {
            at_ms,
            head,
            body,
            json,
        }
    }

    /// The response just sent in `outcome`, its body read from the store.
    pub(crate) fn sent(
        at_ms: i64,
        outcome: &SendOutcome,
        read: ReadBody<'_>,
    ) -> Result<Self, StoreError> {
        let body = readable(outcome.body.size)
            .map(|len| read(&outcome.body.id, len))
            .transpose()?;
        Ok(Self::new(at_ms, outcome.head.clone(), body.as_deref()))
    }

    /// `response(path=…)`: the body, or what a JSONPath or XPath selects
    /// in it (see [`query_body`]).
    fn value(&self, request: &RequestPath, path: Option<&str>) -> Result<String, String> {
        let Some(text) = &self.body else {
            return Err(format!("the response of `{request}` is too large to read"));
        };
        let written = path.unwrap_or("").trim();
        if written.is_empty() || written == "$" {
            return Ok(text.clone());
        }
        let values = query_body(written, text, self.json.as_ref())
            .map_err(|e| format!("{e}, in the response of `{request}`"))?;
        match values.as_slice() {
            [] => Err(format!("`{written}` is not in the response of `{request}`")),
            [one] => Ok(one.to_text()),
            many => Ok(Json::Array(many.to_vec()).to_string()),
        }
    }

    /// `header(name=…)`: its values joined by `, `; names ignore case.
    fn header(&self, request: &RequestPath, name: &str) -> Result<String, String> {
        let values: Vec<&str> = (self.head.headers.iter())
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect();
        if values.is_empty() {
            return Err(format!(
                "the response of `{request}` has no `{name}` header"
            ));
        }
        Ok(values.join(", "))
    }
}

/// What `expression` selects in a body. A JSONPath starts with `$`; on a
/// JSON body, a path without it is the simple form (`data.items[0]`) read as
/// JSONPath; on any other body, the expression is an XPath.
///
/// # Errors
///
/// If the expression is malformed, or not in the body's language.
pub(crate) fn query_body(
    expression: &str,
    text: &str,
    json: Option<&Json>,
) -> Result<Vec<Json>, QueryError> {
    let expression = expression.trim();
    if json.is_some() && Language::of(expression) == Language::XPath {
        let joined = if expression.starts_with('[') {
            format!("${expression}")
        } else {
            format!("$.{expression}")
        };
        return apifast_query::select(&joined, text, json);
    }
    apifast_query::select(expression, text, json)
}

/// How many bytes of a body of `size` to read, or `None` if too large.
fn readable(size: u64) -> Option<usize> {
    (size <= CHECK_LIMIT).then(|| usize::try_from(size).unwrap_or(usize::MAX))
}

/// When to send the referenced request again (`refresh`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refresh {
    /// Only the saved response; an error without one.
    Never,
    /// Send only when nothing was saved. The default.
    Missing,
    /// Send when the saved response is older than this many seconds.
    Ttl(u64),
    /// Send every time (once per send, however many times it is used).
    Always,
}

impl Refresh {
    fn parse(refresh: Option<&str>, ttl: Option<&str>) -> Result<Self, String> {
        Ok(match refresh.map_or("missing", str::trim) {
            "never" => Self::Never,
            "" | "missing" => Self::Missing,
            "always" => Self::Always,
            "ttl" => Self::Ttl(
                ttl.and_then(|t| t.trim().parse().ok())
                    .ok_or("`refresh='ttl'` needs `ttl`, in seconds")?,
            ),
            other => {
                return Err(format!(
                    "`refresh` is never, missing, ttl or always, not `{other}`"
                ));
            }
        })
    }
}

/// The request named in `request='auth/login'`: relative to the
/// collection, with or without its extension.
pub(crate) fn request_path(name: &str) -> RequestPath {
    let name = name.trim().trim_start_matches("./");
    let has_extension = std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"));
    if has_extension {
        RequestPath::new(name)
    } else {
        RequestPath::new(format!("{name}.yml"))
    }
}

/// Answers `response` and `header` for one send.
pub(crate) struct ResponseFunctions<'a> {
    history: Option<&'a (dyn HistoryStore + Sync)>,
    read: ReadBody<'a>,
    now_ms: i64,
    /// Responses at hand: sent during this send, or read from the history.
    known: BTreeMap<RequestPath, Arc<Fetched>>,
    /// The history, read once when first needed.
    entries: Option<Vec<HistoryEntry>>,
    /// Requests to send before rendering again, in order.
    pub(crate) needed: Vec<RequestPath>,
}

impl<'a> ResponseFunctions<'a> {
    pub(crate) fn new(
        history: Option<&'a (dyn HistoryStore + Sync)>,
        read: ReadBody<'a>,
        now_ms: i64,
    ) -> Self {
        Self {
            history,
            read,
            now_ms,
            known: BTreeMap::new(),
            entries: None,
            needed: Vec::new(),
        }
    }

    /// Makes the response of `request` available to the next round.
    pub(crate) fn add(&mut self, request: RequestPath, fetched: Arc<Fetched>) {
        self.known.insert(request, fetched);
    }

    /// Calls `response` or `header`.
    pub(crate) fn call(
        &mut self,
        name: &str,
        args: &[(String, FunctionArg)],
    ) -> Result<String, String> {
        let arg = |key: &str| {
            args.iter().find(|(k, _)| k == key).map(|(_, v)| match v {
                FunctionArg::Text(text) => text.clone(),
                FunctionArg::Bool(b) => b.to_string(),
                FunctionArg::Null => String::new(),
            })
        };
        let request = arg("request")
            .filter(|r| !r.trim().is_empty())
            .ok_or("`request` is missing")?;
        let request = request_path(&request);
        let refresh = Refresh::parse(arg("refresh").as_deref(), arg("ttl").as_deref())?;
        let fetched = if let Some(fetched) = self.known.get(&request) {
            fetched.clone()
        } else if let Some(fetched) = self.saved(&request, refresh)? {
            self.known.insert(request.clone(), fetched.clone());
            fetched
        } else {
            if !self.needed.contains(&request) {
                self.needed.push(request.clone());
            }
            return Err(format!("`{request}` has not been sent yet"));
        };
        match name {
            "header" => {
                let header = arg("name").ok_or("`name` is missing")?;
                fetched.header(&request, header.trim())
            }
            _ => fetched.value(&request, arg("path").as_deref()),
        }
    }

    /// The saved response to use under `refresh`; `None` means send it.
    fn saved(
        &mut self,
        request: &RequestPath,
        refresh: Refresh,
    ) -> Result<Option<Arc<Fetched>>, String> {
        if refresh == Refresh::Always {
            return Ok(None);
        }
        let latest = self.latest(request);
        Ok(match refresh {
            Refresh::Never => {
                Some(latest.ok_or_else(|| format!("`{request}` has no saved response"))?)
            }
            Refresh::Ttl(seconds) => latest.filter(|f| {
                let age_ms = self.now_ms.saturating_sub(f.at_ms);
                age_ms <= i64::try_from(seconds.saturating_mul(1000)).unwrap_or(i64::MAX)
            }),
            Refresh::Missing | Refresh::Always => latest,
        })
    }

    /// The last response of `request` in the history whose body is still
    /// kept. A history that cannot be read counts as empty.
    fn latest(&mut self, request: &RequestPath) -> Option<Arc<Fetched>> {
        let history = self.history?;
        let entries = self
            .entries
            .get_or_insert_with(|| history.entries().unwrap_or_default());
        entries.iter().rev().find_map(|entry| {
            let HistoryOutcome::Received { head, size, body } = &entry.outcome else {
                return None;
            };
            if entry.request != *request {
                return None;
            }
            let bytes = match readable(*size) {
                Some(len) => Some((self.read)(body, len).ok()?),
                None => None,
            };
            Some(Arc::new(Fetched::new(
                entry.at_ms,
                head.clone(),
                bytes.as_deref(),
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_names_take_the_extension_when_missing() {
        assert_eq!(
            request_path("auth/login"),
            RequestPath::new("auth/login.yml")
        );
        assert_eq!(
            request_path("./auth/login.yml"),
            RequestPath::new("auth/login.yml")
        );
        assert_eq!(request_path("a.yaml"), RequestPath::new("a.yaml"));
    }

    #[test]
    fn refresh_policies_are_read_with_their_ttl() {
        assert_eq!(Refresh::parse(None, None), Ok(Refresh::Missing));
        assert_eq!(
            Refresh::parse(Some("ttl"), Some("300")),
            Ok(Refresh::Ttl(300))
        );
        assert!(Refresh::parse(Some("ttl"), None).is_err());
        assert!(Refresh::parse(Some("sometimes"), None).is_err());
        assert_eq!(Refresh::parse(Some("always"), None), Ok(Refresh::Always));
        assert_eq!(Refresh::parse(Some("never"), None), Ok(Refresh::Never));
    }

    #[test]
    fn values_come_from_json_paths_and_headers() {
        let head = ResponseHead {
            status: 200,
            version: String::new(),
            url: String::new(),
            redirects: 0,
            headers: vec![("X-Id".into(), "1".into()), ("x-id".into(), "2".into())],
            elapsed_ms: 0,
            timeline: Vec::new(),
        };
        let r = RequestPath::new("a.yml");
        let fetched = Fetched::new(
            0,
            head.clone(),
            Some(br#"{"data":{"token":"t"},"list":[5]}"#),
        );
        assert_eq!(fetched.value(&r, Some("$.data.token")), Ok("t".into()));
        assert_eq!(fetched.value(&r, Some("list[0]")), Ok("5".into()));
        assert_eq!(fetched.value(&r, Some("$..token")), Ok("t".into()));
        assert_eq!(
            fetched.value(&r, Some("$.*")),
            Ok(r#"[{"token":"t"},[5]]"#.into())
        );
        assert_eq!(fetched.value(&r, Some("$.list[?@ > 1]")), Ok("5".into()));
        let xml = Fetched::new(
            0,
            head.clone(),
            Some(b"<a><b id='1'>x</b><b id='2'>y</b></a>"),
        );
        assert_eq!(xml.value(&r, Some("/a/b[2]")), Ok("y".into()));
        assert_eq!(xml.value(&r, Some("//b/@id")), Ok(r#"["1","2"]"#.into()));
        assert_eq!(xml.value(&r, Some("count(//b)")), Ok("2".into()));
        assert_eq!(fetched.value(&r, None).unwrap().len(), 33);
        assert_eq!(
            fetched.value(&r, Some("$.nope")),
            Err("`$.nope` is not in the response of `a.yml`".into())
        );
        assert_eq!(fetched.header(&r, "x-ID"), Ok("1, 2".into()));
        assert!(fetched.header(&r, "x-other").is_err());
        let text = Fetched::new(0, head.clone(), Some(b"plain"));
        assert_eq!(text.value(&r, Some("$")), Ok("plain".into()));
        assert!(
            text.value(&r, Some("$.a"))
                .unwrap_err()
                .contains("not JSON")
        );
        assert!(text.value(&r, Some("/a")).unwrap_err().contains("not XML"));
        let big = Fetched::new(0, head, None);
        assert!(big.value(&r, None).unwrap_err().contains("too large"));
    }
}
