//! The response store on a real folder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;
use std::time::{Duration, SystemTime};

use apifast_app::{BodyWriter, ResponseId, ResponseStore};
use apifast_history::{FsResponseStore, Limits, Pruned};

fn files(store: &FsResponseStore) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.dir())
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn store_body(store: &FsResponseStore, chunks: &[&[u8]]) -> ResponseId {
    let mut writer = store.create().unwrap();
    for chunk in chunks {
        writer.write(chunk).unwrap();
    }
    writer.finish().unwrap().id
}

#[test]
fn writes_in_chunks_and_reads_back_in_windows() {
    let dir = tempfile::tempdir().unwrap();
    // The folder does not exist yet: the first write creates it.
    let store = FsResponseStore::new(dir.path().join("responses/col_1"));
    let mut writer = store.create().unwrap();
    writer.write(b"hello ").unwrap();
    writer.write(b"").unwrap();
    writer.write(b"world").unwrap();
    let stored = writer.finish().unwrap();

    assert_eq!(stored.size, 11);
    assert_eq!(store.size(&stored.id).unwrap(), 11);
    assert_eq!(store.read(&stored.id, 0, 100).unwrap(), b"hello world");
    assert_eq!(store.read(&stored.id, 6, 3).unwrap(), b"wor");
    assert_eq!(store.read(&stored.id, 10, 5).unwrap(), b"d");
    assert_eq!(store.read(&stored.id, 11, 5).unwrap(), b"");
    assert_eq!(store.read(&stored.id, 99, 5).unwrap(), b"");
    assert_eq!(store.read(&stored.id, 0, 0).unwrap(), b"");
    assert_eq!(files(&store), [format!("{}.body", stored.id.0)]);
}

#[test]
fn empty_bodies_are_stored() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let stored = store.create().unwrap().finish().unwrap();
    assert_eq!(stored.size, 0);
    assert_eq!(store.read(&stored.id, 0, 10).unwrap(), b"");
}

#[test]
fn large_bodies_stream_through() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let chunk = vec![7u8; 64 * 1024 + 13];
    let mut writer = store.create().unwrap();
    for _ in 0..320 {
        writer.write(&chunk).unwrap();
    }
    let stored = writer.finish().unwrap();
    let total = chunk.len() as u64 * 320;
    assert_eq!(stored.size, total);
    assert_eq!(
        fs::metadata(dir.path().join(format!("{}.body", stored.id.0)))
            .unwrap()
            .len(),
        total
    );
    assert_eq!(store.read(&stored.id, total - 3, 10).unwrap(), [7, 7, 7]);
}

#[test]
fn an_unfinished_body_is_never_visible_and_is_removed_when_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let mut writer = store.create().unwrap();
    writer.write(b"half").unwrap();
    // While writing there is only a partial file.
    assert!(files(&store).iter().all(|f| f.ends_with(".partial")));
    drop(writer);
    assert_eq!(files(&store), Vec::<String>::new());
}

#[test]
fn concurrent_writers_do_not_mix() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let mut a = store.create().unwrap();
    let mut b = store.create().unwrap();
    for _ in 0..100 {
        a.write(b"a").unwrap();
        b.write(b"bb").unwrap();
    }
    let (a, b) = (a.finish().unwrap(), b.finish().unwrap());
    assert_ne!(a.id, b.id);
    assert_eq!(store.read(&a.id, 0, 1000).unwrap(), vec![b'a'; 100]);
    assert_eq!(store.read(&b.id, 0, 1000).unwrap(), vec![b'b'; 200]);
}

#[test]
fn invalid_or_missing_ids_are_errors_and_never_leave_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("secret.body"), "outside").unwrap();
    let store = FsResponseStore::new(dir.path().join("store"));
    for bad in ["../secret", "..", "a/b", ""] {
        let id = ResponseId(bad.into());
        assert!(store.read(&id, 0, 10).is_err(), "{bad:?}");
        assert!(store.size(&id).is_err(), "{bad:?}");
        assert!(store.remove(&id).is_err(), "{bad:?}");
    }
    assert!(store.read(&ResponseId("1-2-3".into()), 0, 10).is_err());
    assert!(dir.path().join("secret.body").exists());
}

#[test]
fn responses_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let id = store_body(&store, &[b"x"]);
    store.remove(&id).unwrap();
    store.remove(&id).unwrap();
    assert!(store.read(&id, 0, 1).is_err());
}

fn age(store: &FsResponseStore, id: &ResponseId, by: Duration) {
    let file = fs::File::options()
        .write(true)
        .open(store.dir().join(format!("{}.body", id.0)))
        .unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

#[test]
fn pruning_keeps_the_newest_within_the_limits() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let ids: Vec<ResponseId> = (0..5)
        .map(|i| store_body(&store, &[&vec![b'x'; 100 * (i + 1)]]))
        .collect();
    // Oldest first: ids[0] is a day old, ids[4] is new.
    for (i, id) in ids.iter().enumerate() {
        age(&store, id, Duration::from_secs(3600 * (5 - i as u64)));
    }

    let by_count = store
        .prune(
            Limits {
                max_count: Some(4),
                ..Limits::default()
            },
            SystemTime::now(),
        )
        .unwrap();
    assert_eq!(
        by_count,
        Pruned {
            responses: 1,
            partials: 0,
            bytes: 100
        }
    );
    assert!(store.size(&ids[0]).is_err());

    // Newest are 500 + 400 bytes; 300 more would exceed 1000.
    let by_size = store
        .prune(
            Limits {
                max_total_bytes: Some(1000),
                ..Limits::default()
            },
            SystemTime::now(),
        )
        .unwrap();
    assert_eq!(by_size.responses, 2);
    assert!(store.size(&ids[3]).is_ok() && store.size(&ids[4]).is_ok());

    let by_age = store
        .prune(
            Limits {
                max_age: Some(Duration::from_mins(90)),
                ..Limits::default()
            },
            SystemTime::now(),
        )
        .unwrap();
    assert_eq!(by_age.responses, 1);
    assert_eq!(files(&store), [format!("{}.body", ids[4].0)]);

    // No limits: nothing is removed.
    assert_eq!(
        store.prune(Limits::default(), SystemTime::now()).unwrap(),
        Pruned::default()
    );
}

#[test]
fn pruning_removes_stale_partials_but_not_running_ones() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let mut running = store.create().unwrap();
    running.write(b"in progress").unwrap();
    fs::write(dir.path().join("1-1-1.partial"), "crashed").unwrap();
    let old = fs::File::options()
        .write(true)
        .open(dir.path().join("1-1-1.partial"))
        .unwrap();
    old.set_modified(SystemTime::now() - Duration::from_secs(7200))
        .unwrap();

    let pruned = store.prune(Limits::default(), SystemTime::now()).unwrap();
    assert_eq!(pruned.partials, 1);
    let stored = running.finish().unwrap();
    assert_eq!(store.read(&stored.id, 0, 100).unwrap(), b"in progress");
}

#[test]
fn pruning_a_missing_folder_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path().join("never-created"));
    assert_eq!(
        store
            .prune(
                Limits {
                    max_count: Some(0),
                    ..Limits::default()
                },
                SystemTime::now()
            )
            .unwrap(),
        Pruned::default()
    );
}
