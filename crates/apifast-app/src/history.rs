//! The history of sends: one entry per send that finished, kept by a
//! [`HistoryStore`] outside the collection (ADR 0008).
//!
//! History is not encrypted, so an entry never holds a resolved secret: the
//! request is kept as written (`{{token}}`, not its value), and when a send
//! carried a secret the queries of the URLs it reached are hidden. Values of
//! `Set-Cookie` are always hidden; cookies have their own encrypted store.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use apifast_domain::{MultipartValue, Secret, mask};

use crate::{
    RenderedBody, RenderedRequest, RequestPath, ResponseHead, ResponseId, StoreError, TimelineKind,
};

/// What replaces a hidden value.
pub const HIDDEN: &str = "[hidden]";

/// How much of a text body sent the history keeps, in bytes.
pub const SENT_BODY_LIMIT: usize = 64 * 1024;

/// Headers whose values the history never keeps: they carry credentials
/// (tokens kept encrypted, cookies of the encrypted jar) whatever wrote
/// them.
const CREDENTIAL_HEADERS: [&str; 3] = ["authorization", "proxy-authorization", "cookie"];

/// Numbers the entries of failed sends made by this process, so their ids
/// never repeat; a response's id is its body's.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One send that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Names the entry, to rename or remove it: the id of its response's
    /// body, or one made for a send that failed.
    pub id: String,
    /// The name the user gave it. A named entry is kept by
    /// [`within_limits`] whatever its age, and so is its body.
    pub name: Option<String>,
    /// When it was sent, in milliseconds since the Unix epoch.
    pub at_ms: i64,
    /// The request sent.
    pub request: RequestPath,
    /// Its method.
    pub method: String,
    /// Its URL as written, with its `{{variables}}` unresolved.
    pub url: String,
    /// How it ended.
    pub outcome: HistoryOutcome,
    /// The request as it went out, with its secrets masked; none for a send
    /// that failed before it was built, or an entry from before apifast
    /// kept it.
    pub sent: Option<SentRequest>,
}

/// A request as apifast built it to send: the URL, headers and body once
/// rendered. Secrets are masked, and when the request carried one the
/// values of its query are hidden too, as in the timeline. The values of
/// `Authorization`, `Proxy-Authorization` and `Cookie` are always hidden.
/// What the transport adds on the way (cookies of the jar, signatures,
/// `Content-Length`) is not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentRequest {
    /// The method.
    pub method: String,
    /// The URL.
    pub url: String,
    /// The headers, in order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: SentBody,
}

/// The body of a [`SentRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentBody {
    /// No body.
    None,
    /// Text, up to [`SENT_BODY_LIMIT`] bytes; `size` is that of the whole.
    Text {
        /// The text kept.
        text: String,
        /// Bytes of the whole text.
        size: u64,
    },
    /// Form fields.
    Form(Vec<(String, String)>),
    /// Multipart parts.
    Multipart(Vec<SentPart>),
    /// A file sent as is, by its path relative to the collection.
    File(String),
}

/// A part of a multipart body sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentPart {
    /// Its name.
    pub name: String,
    /// Its text, or the paths of its files.
    pub value: String,
    /// Whether `value` are file paths.
    pub files: bool,
    /// Its `Content-Type`, if given.
    pub content_type: Option<String>,
}

impl SentRequest {
    /// What the history keeps of `request`.
    pub fn of(request: &RenderedRequest) -> Self {
        let secrets = request.secrets();
        let clean = |text: &str| mask(text, secrets).into_owned();
        let url = if secrets.is_empty() {
            request.url().to_owned()
        } else {
            hide_query(&clean(request.url()))
        };
        let headers = (request.headers().iter())
            .map(|(name, value)| {
                let hidden = CREDENTIAL_HEADERS
                    .iter()
                    .any(|credential| name.eq_ignore_ascii_case(credential));
                let value = if hidden {
                    HIDDEN.to_owned()
                } else {
                    clean(value)
                };
                (name.clone(), value)
            })
            .collect();
        let pairs = |pairs: &[(String, String)]| {
            (pairs.iter())
                .map(|(name, value)| (name.clone(), clean(value)))
                .collect()
        };
        let body = match request.body() {
            RenderedBody::None => SentBody::None,
            RenderedBody::Text(text) => {
                let text = clean(text);
                let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
                SentBody::Text {
                    text: cut(text, SENT_BODY_LIMIT),
                    size,
                }
            }
            RenderedBody::Form(fields) => SentBody::Form(pairs(fields)),
            RenderedBody::Multipart(parts) => SentBody::Multipart(
                (parts.iter())
                    .map(|part| {
                        let (value, files) = match &part.value {
                            MultipartValue::Text(text) => (clean(text), false),
                            MultipartValue::Files(paths) => (paths.join(", "), true),
                        };
                        SentPart {
                            name: part.name.clone(),
                            value,
                            files,
                            content_type: part.content_type.clone(),
                        }
                    })
                    .collect(),
            ),
            RenderedBody::File(path) => SentBody::File(path.clone()),
        };
        Self {
            method: request.method().as_str().to_owned(),
            url,
            headers,
            body,
        }
    }

    /// The request as HTTP text: the request line, the headers, a blank
    /// line and the body, to copy or read. A body kept in part says how
    /// much was left out.
    pub fn to_http(&self) -> String {
        let mut out = format!("{} {}\n", self.method, self.url);
        for (name, value) in &self.headers {
            let _ = writeln!(out, "{name}: {value}");
        }
        let body = match &self.body {
            SentBody::None => return out,
            SentBody::Text { text, size } => {
                let kept = u64::try_from(text.len()).unwrap_or(u64::MAX);
                let mut body = text.clone();
                if kept < *size {
                    let _ = write!(body, "\n[{} more bytes not kept]", size - kept);
                }
                body
            }
            SentBody::Form(fields) => (fields.iter())
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("&"),
            SentBody::Multipart(parts) => (parts.iter())
                .map(|part| {
                    let at = if part.files { "@" } else { "" };
                    format!("{}={at}{}", part.name, part.value)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            SentBody::File(path) => format!("@{path}"),
        };
        out.push('\n');
        out.push_str(&body);
        out
    }
}

/// `text` cut to at most `limit` bytes, on a character's boundary.
fn cut(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// How a send ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryOutcome {
    /// A response arrived.
    Received {
        /// Its status, headers and timeline, with secrets hidden.
        head: ResponseHead,
        /// Size of its body.
        size: u64,
        /// Where its body is stored. It may have been removed since, by
        /// the limits of the response store.
        body: ResponseId,
    },
    /// It failed; the message holds no secrets.
    Failed(String),
}

impl HistoryEntry {
    /// An entry for a response. `secrets` are the values of the secrets
    /// the request carried: they are masked wherever they appear in the
    /// URLs it reached, and the queries of those URLs are hidden too.
    pub fn received(
        at_ms: i64,
        request: RequestPath,
        (method, url): (String, String),
        mut head: ResponseHead,
        (size, body): (u64, ResponseId),
        secrets: &[Secret],
    ) -> Self {
        for (name, value) in &mut head.headers {
            if name.eq_ignore_ascii_case("set-cookie") {
                HIDDEN.clone_into(value);
            }
        }
        if !secrets.is_empty() {
            // Masked anywhere (a secret in the path too), and every query
            // value hidden besides, as some may be secrets in other forms.
            let clean = |url: &str| hide_query(&mask(url, secrets));
            head.url = clean(&head.url);
            for event in &mut head.timeline {
                match &mut event.kind {
                    TimelineKind::Request { url, .. } => *url = clean(url),
                    TimelineKind::Redirect { location, .. } => *location = clean(location),
                    // A host may come from a secret too.
                    TimelineKind::Dns { host, .. } => *host = mask(host, secrets).into_owned(),
                    TimelineKind::Connected { .. }
                    | TimelineKind::Reused { .. }
                    | TimelineKind::Tls { .. }
                    | TimelineKind::Headers { .. }
                    | TimelineKind::Body { .. }
                    | TimelineKind::Decompressed { .. } => {}
                }
            }
        }
        Self {
            id: body.0.clone(),
            name: None,
            at_ms,
            request,
            method,
            url,
            outcome: HistoryOutcome::Received { head, size, body },
            sent: None,
        }
    }

    /// An entry for a send that failed.
    pub fn failed(
        at_ms: i64,
        request: RequestPath,
        (method, url): (String, String),
        message: String,
    ) -> Self {
        let n = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            id: format!("failed-{at_ms}-{}-{n}", std::process::id()),
            name: None,
            at_ms,
            request,
            method,
            url,
            outcome: HistoryOutcome::Failed(message),
            sent: None,
        }
    }

    /// The entry, with what was sent.
    #[must_use]
    pub fn with_sent(mut self, sent: Option<SentRequest>) -> Self {
        self.sent = sent;
        self
    }

    /// The name to show: the one given, or none.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref().filter(|name| !name.is_empty())
    }
}

/// `url` with the values of its query hidden: `/a?key=[hidden]`. Names stay,
/// so the entry still tells which parameters were sent.
pub fn hide_query(url: &str) -> String {
    let Some((base, rest)) = url.split_once('?') else {
        return url.to_owned();
    };
    let (query, fragment) = match rest.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (rest, None),
    };
    let hidden: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) => format!("{name}={HIDDEN}"),
            None => pair.to_owned(),
        })
        .collect();
    let mut out = format!("{base}?{}", hidden.join("&"));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// How much history [`HistoryStore::prune`] keeps: the newest entries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryLimits {
    /// Maximum number of entries.
    pub max_entries: Option<usize>,
    /// Entries older than this are removed.
    pub max_age: Option<Duration>,
}

/// Keeps the history of one collection.
pub trait HistoryStore {
    /// Adds an entry at the end.
    ///
    /// # Errors
    ///
    /// If it cannot be written.
    fn append(&self, entry: &HistoryEntry) -> Result<(), StoreError>;

    /// Every entry, oldest first. Entries that cannot be read are skipped.
    ///
    /// # Errors
    ///
    /// If the history exists but cannot be read.
    fn entries(&self) -> Result<Vec<HistoryEntry>, StoreError>;

    /// Removes the oldest entries until `limits` hold, `now_ms` being the
    /// current time. Returns how many were removed.
    ///
    /// # Errors
    ///
    /// If it cannot be read or written.
    fn prune(&self, limits: HistoryLimits, now_ms: i64) -> Result<usize, StoreError>;

    /// Names the entry `id`, or takes its name away with `None` or an
    /// empty name. Returns whether there was such an entry.
    ///
    /// # Errors
    ///
    /// If it cannot be read or written.
    fn rename(&self, id: &str, name: Option<&str>) -> Result<bool, StoreError>;

    /// Removes the entry `id`. Returns it, if there was one, so its body
    /// can be removed too.
    ///
    /// # Errors
    ///
    /// If it cannot be read or written.
    fn remove(&self, id: &str) -> Result<Option<HistoryEntry>, StoreError>;

    /// Removes every entry. Returns them, so their bodies can be removed
    /// too.
    ///
    /// # Errors
    ///
    /// If it cannot be read or written.
    fn clear(&self) -> Result<Vec<HistoryEntry>, StoreError>;
}

/// The entries of `entries` that `limits` keep, oldest first. Named
/// entries are always kept; they count toward the number allowed, and the
/// oldest unnamed ones go first.
pub fn within_limits(
    mut entries: Vec<HistoryEntry>,
    limits: HistoryLimits,
    now_ms: i64,
) -> Vec<HistoryEntry> {
    if let Some(max_age) = limits.max_age {
        let max_age = i64::try_from(max_age.as_millis()).unwrap_or(i64::MAX);
        entries.retain(|entry| {
            entry.name().is_some() || now_ms.saturating_sub(entry.at_ms) <= max_age
        });
    }
    if let Some(max) = limits.max_entries {
        let mut excess = entries.len().saturating_sub(max);
        entries.retain(|entry| {
            let goes = excess > 0 && entry.name().is_none();
            if goes {
                excess -= 1;
            }
            !goes
        });
    }
    entries
}

/// Applies a rename to `entries`, as [`HistoryStore::rename`] does: whether
/// the entry was there.
pub fn rename_in(entries: &mut [HistoryEntry], id: &str, name: Option<&str>) -> bool {
    let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
        return false;
    };
    entry.name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimelineEvent;
    use apifast_domain::MASK;
    use apifast_domain::Method;

    fn head(url: &str) -> ResponseHead {
        ResponseHead {
            status: 200,
            version: "HTTP/1.1".into(),
            url: url.into(),
            redirects: 0,
            headers: vec![
                ("Set-Cookie".into(), "session=abc".into()),
                ("content-type".into(), "text/plain".into()),
            ],
            elapsed_ms: 3,
            timeline: vec![TimelineEvent {
                at_us: 0,
                kind: TimelineKind::Request {
                    method: "GET".into(),
                    url: url.into(),
                },
            }],
        }
    }

    fn entry(secrets: &[Secret]) -> HistoryEntry {
        HistoryEntry::received(
            1,
            RequestPath::new("a.yml"),
            ("GET".into(), "{{host}}/a?key={{key}}".into()),
            head("http://h/a?key=s3cret&page=2#top"),
            (5, ResponseId("r".into())),
            secrets,
        )
    }

    #[test]
    fn a_send_with_a_secret_hides_the_values_of_its_queries() {
        let HistoryOutcome::Received { head, .. } = entry(&[Secret::new("s3cret")]).outcome else {
            unreachable!()
        };
        assert_eq!(head.url, "http://h/a?key=[hidden]&page=[hidden]#top");
        let TimelineKind::Request { url, .. } = &head.timeline[0].kind else {
            unreachable!()
        };
        assert!(!url.contains("s3cret"));
    }

    #[test]
    fn without_a_secret_urls_are_kept_but_cookies_never() {
        let entry = entry(&[]);
        assert_eq!(entry.url, "{{host}}/a?key={{key}}");
        let HistoryOutcome::Received { head, .. } = entry.outcome else {
            unreachable!()
        };
        assert_eq!(head.url, "http://h/a?key=s3cret&page=2#top");
        assert_eq!(head.headers[0].1, HIDDEN);
        assert_eq!(head.headers[1].1, "text/plain");
    }

    #[test]
    fn a_secret_in_the_path_is_masked_too() {
        let entry = HistoryEntry::received(
            1,
            RequestPath::new("a.yml"),
            ("GET".into(), "{{host}}/users/{{token}}".into()),
            head("http://h/users/t0k3n"),
            (5, ResponseId("r".into())),
            &[Secret::new("t0k3n")],
        );
        let HistoryOutcome::Received { head, .. } = entry.outcome else {
            unreachable!()
        };
        assert_eq!(head.url, format!("http://h/users/{MASK}"));
    }

    #[test]
    fn urls_without_a_query_are_unchanged() {
        assert_eq!(hide_query("http://h/a"), "http://h/a");
        assert_eq!(hide_query("http://h/a?flag"), "http://h/a?flag");
    }

    #[test]
    fn limits_keep_the_newest_entries() {
        let at = |at_ms: i64| {
            let mut entry = HistoryEntry::failed(
                at_ms,
                RequestPath::new("a.yml"),
                (String::new(), String::new()),
                String::new(),
            );
            entry.id = at_ms.to_string();
            entry
        };
        let entries = vec![at(0), at(50), at(90), at(100)];
        let kept = within_limits(
            entries.clone(),
            HistoryLimits {
                max_entries: Some(2),
                max_age: None,
            },
            100,
        );
        assert_eq!(kept, [at(90), at(100)]);
        let kept = within_limits(
            entries,
            HistoryLimits {
                max_entries: None,
                max_age: Some(Duration::from_millis(50)),
            },
            100,
        );
        assert_eq!(kept, [at(50), at(90), at(100)]);
    }

    #[test]
    fn named_entries_outlive_the_limits_and_count_toward_them() {
        let at = |at_ms: i64, name: Option<&str>| {
            let mut entry = HistoryEntry::failed(
                at_ms,
                RequestPath::new("a.yml"),
                (String::new(), String::new()),
                String::new(),
            );
            entry.id = at_ms.to_string();
            entry.name = name.map(str::to_owned);
            entry
        };
        let entries = vec![
            at(0, Some("keep")),
            at(50, None),
            at(90, None),
            at(100, None),
        ];
        let ids = |kept: Vec<HistoryEntry>| kept.into_iter().map(|e| e.id).collect::<Vec<_>>();
        let limits = HistoryLimits {
            max_entries: Some(2),
            max_age: None,
        };
        assert_eq!(
            ids(within_limits(entries.clone(), limits, 100)),
            ["0", "100"]
        );
        let limits = HistoryLimits {
            max_entries: None,
            max_age: Some(Duration::from_millis(20)),
        };
        assert_eq!(ids(within_limits(entries, limits, 100)), ["0", "90", "100"]);
    }

    #[test]
    fn a_rename_trims_and_an_empty_name_takes_it_away() {
        let mut entries = vec![entry(&[])];
        assert!(rename_in(&mut entries, "r", Some("  ok  ")));
        assert_eq!(entries[0].name.as_deref(), Some("ok"));
        assert!(rename_in(&mut entries, "r", Some(" ")));
        assert_eq!(entries[0].name, None);
        assert!(!rename_in(&mut entries, "other", Some("x")));
    }

    fn rendered(secret: bool) -> RenderedRequest {
        let token = if secret {
            Secret::new("s3cret")
        } else {
            Secret::new("unused")
        };
        RenderedRequest::own(
            Method::Post,
            "https://api.test/u?key=s3cret".into(),
            vec![
                ("Authorization".into(), "Bearer abc".into()),
                ("X-Key".into(), "s3cret".into()),
                ("Cookie".into(), "a=1".into()),
            ],
            RenderedBody::Text(format!(
                "{{\"key\":\"s3cret\",\"pad\":\"{}\"}}",
                "é".repeat(SENT_BODY_LIMIT)
            )),
            if secret { vec![token] } else { Vec::new() },
        )
    }

    #[test]
    fn what_was_sent_masks_secrets_and_never_keeps_credentials() {
        let sent = SentRequest::of(&rendered(true));
        assert_eq!(sent.method, "POST");
        assert_eq!(sent.url, "https://api.test/u?key=[hidden]");
        assert_eq!(sent.headers[0].1, HIDDEN);
        assert_eq!(sent.headers[1].1, MASK);
        assert_eq!(sent.headers[2].1, HIDDEN);
        let SentBody::Text { text, size } = &sent.body else {
            unreachable!()
        };
        assert!(!text.contains("s3cret"));
        // Cut on a character's boundary, and its whole size kept.
        assert!(text.len() <= SENT_BODY_LIMIT && text.len() > SENT_BODY_LIMIT - 2);
        assert!(*size > SENT_BODY_LIMIT as u64);
        assert!(sent.to_http().contains("more bytes not kept"));

        // Without secrets the URL and headers stay, but credentials never.
        let sent = SentRequest::of(&rendered(false));
        assert_eq!(sent.url, "https://api.test/u?key=s3cret");
        assert_eq!(sent.headers[0].1, HIDDEN);
        assert_eq!(sent.headers[1].1, "s3cret");
    }

    #[test]
    fn what_was_sent_reads_as_http() {
        let sent = SentRequest {
            method: "GET".into(),
            url: "http://h/a".into(),
            headers: vec![("accept".into(), "*/*".into())],
            body: SentBody::Form(vec![("a".into(), "1".into()), ("b".into(), "2".into())]),
        };
        assert_eq!(sent.to_http(), "GET http://h/a\naccept: */*\n\na=1&b=2");
        let none = SentRequest {
            body: SentBody::None,
            ..sent
        };
        assert_eq!(none.to_http(), "GET http://h/a\naccept: */*\n");
    }
}
