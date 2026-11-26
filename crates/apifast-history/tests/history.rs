//! The history file: entries survive a round trip, unreadable lines are
//! skipped and pruning keeps the newest.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::time::Duration;

use apifast_app::{
    HistoryEntry, HistoryLimits, HistoryStore, RequestPath, ResponseHead, ResponseId, SentBody,
    SentPart, SentRequest, TimelineEvent, TimelineKind,
};
use apifast_history::FsHistoryStore;

fn received(at_ms: i64) -> HistoryEntry {
    let head = ResponseHead {
        status: 201,
        version: "HTTP/2.0".into(),
        url: "https://api.test/users?page=1".into(),
        redirects: 1,
        headers: vec![("content-type".into(), "application/json".into())],
        elapsed_ms: 42,
        timeline: vec![
            TimelineEvent {
                at_us: 0,
                kind: TimelineKind::Request {
                    method: "POST".into(),
                    url: "https://api.test/u".into(),
                },
            },
            TimelineEvent {
                at_us: 2,
                kind: TimelineKind::Dns {
                    host: "api.test".into(),
                    addresses: vec!["10.0.0.1".into(), "::1".into()],
                    duration_us: 2,
                    overridden: true,
                },
            },
            TimelineEvent {
                at_us: 4,
                kind: TimelineKind::Connected {
                    address: Some("10.0.0.1:443".into()),
                    duration_us: 2,
                },
            },
            TimelineEvent {
                at_us: 8,
                kind: TimelineKind::Tls {
                    version: Some("TLSv1.3".into()),
                    duration_us: 4,
                },
            },
            TimelineEvent {
                at_us: 9,
                kind: TimelineKind::Reused { address: None },
            },
            TimelineEvent {
                at_us: 10,
                kind: TimelineKind::Redirect {
                    status: 307,
                    location: "/users?page=1".into(),
                },
            },
            TimelineEvent {
                at_us: 30,
                kind: TimelineKind::Headers { status: 201 },
            },
            TimelineEvent {
                at_us: 42,
                kind: TimelineKind::Body { bytes: 7 },
            },
            TimelineEvent {
                at_us: 42,
                kind: TimelineKind::Decompressed {
                    encoding: "gzip".into(),
                    wire_bytes: 5,
                },
            },
        ],
    };
    HistoryEntry::received(
        at_ms,
        RequestPath::new("users/create.yml"),
        ("POST".into(), "{{host}}/u".into()),
        head,
        (7, ResponseId("0001-abc".into())),
        &[],
    )
}

fn failed(at_ms: i64) -> HistoryEntry {
    let mut entry = HistoryEntry::failed(
        at_ms,
        RequestPath::new("health.yml"),
        ("GET".into(), "{{host}}/health".into()),
        "cannot connect to `api.test`".into(),
    );
    // Fixed, so two calls give equal entries.
    entry.id = format!("failed-{at_ms}");
    entry
}

fn sent() -> SentRequest {
    SentRequest {
        method: "POST".into(),
        url: "https://api.test/u".into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("authorization".into(), "[hidden]".into()),
        ],
        body: SentBody::Text {
            text: "{\"name\":\"ana\"}".into(),
            size: 14,
        },
    }
}

#[test]
fn entries_survive_a_round_trip_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("history/c.jsonl"));
    assert_eq!(store.entries().unwrap(), []);
    store.append(&received(1)).unwrap();
    store.append(&failed(2)).unwrap();
    assert_eq!(store.entries().unwrap(), [received(1), failed(2)]);
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert_eq!(text.lines().count(), 2, "one line per entry");
}

#[test]
fn an_event_of_a_newer_version_is_skipped_and_its_entry_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    store.append(&received(1)).unwrap();
    let text = std::fs::read_to_string(store.file()).unwrap();
    let newer = text.replacen(
        "\"timeline\":[",
        "\"timeline\":[{\"type\":\"quic\",\"at_us\":1},",
        1,
    );
    assert_ne!(newer, text);
    std::fs::write(store.file(), newer).unwrap();
    assert_eq!(store.entries().unwrap(), [received(1)]);
}

#[test]
fn unreadable_lines_are_skipped_and_pruning_drops_them() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    store.append(&received(1)).unwrap();
    // A newer format, garbage, and a line cut by a crash.
    let mut text = std::fs::read_to_string(store.file()).unwrap();
    text.push_str("{\"v\":99}\nnot json\n{\"v\":1,\"at\":3,\"requ");
    std::fs::write(store.file(), text).unwrap();
    store.append(&failed(4)).unwrap();
    assert_eq!(store.entries().unwrap(), [received(1), failed(4)]);

    assert_eq!(store.prune(HistoryLimits::default(), 10).unwrap(), 3);
    assert_eq!(store.entries().unwrap(), [received(1), failed(4)]);
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn pruning_keeps_the_newest_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    for at in [10, 20, 30, 40] {
        store.append(&failed(at)).unwrap();
    }
    let limits = HistoryLimits {
        max_entries: Some(3),
        max_age: Some(Duration::from_millis(25)),
    };
    assert_eq!(store.prune(limits, 50).unwrap(), 2);
    assert_eq!(store.entries().unwrap(), [failed(30), failed(40)]);
    assert_eq!(store.prune(limits, 50).unwrap(), 0);
}

#[test]
fn pruning_a_missing_history_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("none.jsonl"));
    assert_eq!(store.prune(HistoryLimits::default(), 0).unwrap(), 0);
    assert!(!store.file().exists());
}

#[test]
fn what_was_sent_and_the_name_survive_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    let mut named = received(1).with_sent(Some(sent()));
    named.name = Some("Before the fix".into());
    let bodies = [
        SentBody::None,
        SentBody::Form(vec![("a".into(), "1".into())]),
        SentBody::Multipart(vec![SentPart {
            name: "file".into(),
            value: "a.png, b.png".into(),
            files: true,
            content_type: Some("image/png".into()),
        }]),
        SentBody::File("data/body.bin".into()),
    ];
    let others: Vec<HistoryEntry> = (bodies.into_iter().enumerate())
        .map(|(i, body)| {
            let mut entry = failed(i64::try_from(i).unwrap() + 10);
            entry.sent = Some(SentRequest { body, ..sent() });
            entry
        })
        .collect();
    store.append(&named).unwrap();
    for entry in &others {
        store.append(entry).unwrap();
    }
    let mut all = vec![named];
    all.extend(others);
    assert_eq!(store.entries().unwrap(), all);
}

#[test]
fn lines_of_the_first_format_are_still_read_with_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    let v1 = concat!(
        r#"{"v":1,"at":5,"request":"a.yml","method":"GET","url":"/a","error":"refused"}"#,
        "\n",
        r#"{"v":1,"at":6,"request":"a.yml","method":"GET","url":"/a","response":{"status":200,"version":"HTTP/1.1","url":"http://h/a","redirects":0,"headers":[],"elapsed_ms":1,"timeline":[],"size":2,"body":"0002-x"}}"#,
        "\n",
    );
    std::fs::write(store.file(), v1).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "failed-5-line0");
    assert_eq!(entries[1].id, "0002-x");
    assert!(entries.iter().all(|e| e.sent.is_none() && e.name.is_none()));

    // Renaming rewrites them in the new format, with the same ids.
    assert!(store.rename("failed-5-line0", Some("refused")).unwrap());
    let entries = store.entries().unwrap();
    assert_eq!(entries[0].id, "failed-5-line0");
    assert_eq!(entries[0].name.as_deref(), Some("refused"));
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert!(
        text.lines()
            .all(|line| line.starts_with(r#"{"v":2"#) || line.contains(r#""v":2"#))
    );
}

#[test]
fn entries_are_renamed_removed_and_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    store.append(&received(1)).unwrap();
    store.append(&failed(2)).unwrap();

    assert!(store.rename("failed-2", Some("  down  ")).unwrap());
    assert_eq!(store.entries().unwrap()[1].name.as_deref(), Some("down"));
    // An empty name takes it away.
    assert!(store.rename("failed-2", Some("")).unwrap());
    assert_eq!(store.entries().unwrap()[1].name, None);
    assert!(!store.rename("nope", Some("x")).unwrap());

    let removed = store.remove("0001-abc").unwrap().unwrap();
    assert_eq!(removed, received(1));
    assert_eq!(store.entries().unwrap(), [failed(2)]);
    assert_eq!(store.remove("0001-abc").unwrap(), None);

    assert_eq!(store.clear().unwrap(), [failed(2)]);
    assert_eq!(store.entries().unwrap(), []);
    assert!(!store.file().exists());
    assert_eq!(store.clear().unwrap(), []);
}

#[test]
fn pruning_keeps_named_entries_whatever_their_age() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsHistoryStore::new(dir.path().join("c.jsonl"));
    for at in [10, 20, 30, 40] {
        store.append(&failed(at)).unwrap();
    }
    store.rename("failed-10", Some("first")).unwrap();
    let limits = HistoryLimits {
        max_entries: Some(2),
        max_age: None,
    };
    assert_eq!(store.prune(limits, 50).unwrap(), 2);
    let ids: Vec<String> = store.entries().unwrap().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, ["failed-10", "failed-40"]);
    let limits = HistoryLimits {
        max_entries: None,
        max_age: Some(Duration::from_millis(1)),
    };
    store.prune(limits, 1000).unwrap();
    let ids: Vec<String> = store.entries().unwrap().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, ["failed-10"]);
}
