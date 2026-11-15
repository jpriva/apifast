//! The interface session on disk: tabs and drafts survive a restart.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::{RequestPath, Session, SessionStore, SessionTab};
use apifast_domain::{Header, HttpRequest, Method};
use apifast_storage_fs::FsSessionStore;

fn draft() -> HttpRequest {
    let mut request = HttpRequest::new("List", Method::Post, "{{host}}/users?page=2");
    request.seq = Some(3);
    request.headers.push(Header::new("x-trace", "1"));
    request
}

fn session() -> Session {
    Session {
        tabs: vec![
            SessionTab {
                path: RequestPath::new("users/list.yml"),
                draft: Some(draft()),
            },
            SessionTab {
                path: RequestPath::new("health.yml"),
                draft: None,
            },
        ],
        active: Some(RequestPath::new("health.yml")),
        environment: Some("local".into()),
    }
}

#[test]
fn a_missing_session_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsSessionStore::new(dir.path().join("none"));
    assert_eq!(store.load().unwrap(), Session::default());
}

#[test]
fn tabs_drafts_and_environment_read_back_as_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsSessionStore::new(dir.path());
    store.save(&session()).unwrap();
    assert_eq!(FsSessionStore::new(dir.path()).load().unwrap(), session());
}

#[test]
fn saving_removes_drafts_that_are_gone() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsSessionStore::new(dir.path());
    store.save(&session()).unwrap();
    assert!(dir.path().join("drafts/0.yml").exists());

    let mut saved = session();
    saved.tabs[0].draft = None;
    store.save(&saved).unwrap();
    assert!(!dir.path().join("drafts/0.yml").exists());
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn a_broken_draft_loses_only_that_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsSessionStore::new(dir.path());
    store.save(&session()).unwrap();
    std::fs::write(dir.path().join("drafts/0.yml"), "info: [broken").unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.tabs.len(), 2);
    assert_eq!(loaded.tabs[0].draft, None);
    assert_eq!(loaded.active, session().active);
}
