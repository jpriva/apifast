//! The history of sends: one entry per send that finished, kept by a
//! [`HistoryStore`] outside the collection (ADR 0008).
//!
//! History is not encrypted, so an entry never holds a resolved secret: the
//! request is kept as written (`{{token}}`, not its value), and when a send
//! carried a secret the queries of the URLs it reached are hidden. Values of
//! `Set-Cookie` are always hidden; cookies have their own encrypted store.

use std::time::Duration;

use apifast_domain::{Secret, mask};

use crate::{RequestPath, ResponseHead, ResponseId, StoreError, TimelineKind};

/// What replaces a hidden value.
pub const HIDDEN: &str = "[hidden]";

/// One send that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
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
            at_ms,
            request,
            method,
            url,
            outcome: HistoryOutcome::Received { head, size, body },
        }
    }

    /// An entry for a send that failed.
    pub fn failed(
        at_ms: i64,
        request: RequestPath,
        (method, url): (String, String),
        message: String,
    ) -> Self {
        Self {
            at_ms,
            request,
            method,
            url,
            outcome: HistoryOutcome::Failed(message),
        }
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
}

/// The entries of `entries` that `limits` keep, oldest first.
pub fn within_limits(
    mut entries: Vec<HistoryEntry>,
    limits: HistoryLimits,
    now_ms: i64,
) -> Vec<HistoryEntry> {
    if let Some(max_age) = limits.max_age {
        let max_age = i64::try_from(max_age.as_millis()).unwrap_or(i64::MAX);
        entries.retain(|entry| now_ms.saturating_sub(entry.at_ms) <= max_age);
    }
    if let Some(max) = limits.max_entries
        && entries.len() > max
    {
        entries.drain(..entries.len() - max);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimelineEvent;
    use apifast_domain::MASK;

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
        let at = |at_ms| {
            HistoryEntry::failed(
                at_ms,
                RequestPath::new("a.yml"),
                (String::new(), String::new()),
                String::new(),
            )
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
}
