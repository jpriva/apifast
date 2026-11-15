//! The watcher on a real folder: changes arrive in batches of relative
//! paths, ignored folders stay out, and dropping it stops it.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use apifast_watch::{Changes, FsWatcher};

/// Waits for changes that hold `path`, collecting the batches seen.
fn wait_for(receiver: &mpsc::Receiver<Changes>, path: &str) -> Vec<Changes> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(changes) = receiver.recv_timeout(Duration::from_millis(100)) {
            let found = changes.paths.contains(path);
            seen.push(changes);
            if found {
                return seen;
            }
        }
    }
    panic!("no change for `{path}` in {seen:?}");
}

fn watch(dir: &std::path::Path) -> (FsWatcher, mpsc::Receiver<Changes>) {
    let (sender, receiver) = mpsc::channel();
    let watcher = FsWatcher::start(dir, move |changes| {
        let _ = sender.send(changes);
    })
    .unwrap();
    (watcher, receiver)
}

#[test]
fn a_burst_of_writes_arrives_as_one_batch_of_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("users")).unwrap();
    let (_watcher, receiver) = watch(dir.path());
    for name in ["a.yml", "b.yml", "c.yml"] {
        std::fs::write(dir.path().join("users").join(name), "x").unwrap();
    }
    let seen = wait_for(&receiver, "users/c.yml");
    let all: Vec<String> = seen.iter().flat_map(|c| c.paths.clone()).collect();
    for path in ["users/a.yml", "users/b.yml", "users/c.yml"] {
        assert!(all.contains(&path.to_owned()), "{path} in {all:?}");
    }
    // Written together, they come in one batch, not three.
    assert!(seen.len() <= 2, "{seen:?}");
}

#[test]
fn removing_a_file_is_a_change_and_ignored_folders_are_not() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("health.yml"), "x").unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let (_watcher, receiver) = watch(dir.path());
    std::fs::write(dir.path().join(".git/index"), "x").unwrap();
    std::fs::remove_file(dir.path().join("health.yml")).unwrap();
    let seen = wait_for(&receiver, "health.yml");
    assert!(
        seen.iter()
            .all(|c| c.paths.iter().all(|p| !p.starts_with(".git"))),
        "{seen:?}"
    );
}

#[test]
fn dropping_the_watcher_stops_it() {
    let dir = tempfile::tempdir().unwrap();
    let (watcher, receiver) = watch(dir.path());
    drop(watcher);
    std::fs::write(dir.path().join("late.yml"), "x").unwrap();
    // The thread ended, so the sender was dropped.
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn a_missing_folder_cannot_be_watched() {
    let dir = tempfile::tempdir().unwrap();
    let error = FsWatcher::start(&dir.path().join("nope"), |_| {}).unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");
}
