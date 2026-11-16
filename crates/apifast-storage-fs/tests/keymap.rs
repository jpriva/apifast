//! `keymap.yml` round trips and tolerates what it does not understand.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_storage_fs::FsKeymapStore;

fn entry(action: &str, keys: &[&str]) -> (String, Vec<String>) {
    (action.into(), keys.iter().map(|k| (*k).into()).collect())
}

#[test]
fn entries_round_trip_with_one_many_or_no_shortcut() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsKeymapStore::new(dir.path().join("config/keymap.yml"));
    assert_eq!(store.load().unwrap(), []);
    let entries = vec![
        entry("send", &["Ctrl+Enter"]),
        entry("palette", &["Ctrl+K", "Ctrl+Shift+P"]),
        entry("reopen_tab", &[]),
    ];
    store.save(&entries).unwrap();
    assert_eq!(store.load().unwrap(), entries);
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert!(text.contains("send: Ctrl+Enter"), "{text}");
}

#[test]
fn hand_written_files_are_read_and_odd_entries_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keymap.yml");
    std::fs::write(
        &file,
        "# my keys\nsend: Ctrl+R\nsave: [Ctrl+S]\nweird: 3\nnone:\n",
    )
    .unwrap();
    let store = FsKeymapStore::new(&file);
    assert_eq!(
        store.load().unwrap(),
        [
            entry("send", &["Ctrl+R"]),
            entry("save", &["Ctrl+S"]),
            entry("none", &[])
        ]
    );
    std::fs::write(&file, "send: [unclosed").unwrap();
    assert!(store.load().is_err());
}
