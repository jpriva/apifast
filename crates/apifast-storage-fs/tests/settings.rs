//! `settings.yml` keeps scalar values and writes numbers as numbers.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_storage_fs::FsSettingsStore;

fn entry(key: &str, value: &str) -> (String, String) {
    (key.into(), value.into())
}

#[test]
fn values_round_trip_and_numbers_stay_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsSettingsStore::new(dir.path().join("config/settings.yml"));
    assert_eq!(store.load().unwrap(), []);
    let entries = vec![
        entry("theme", "dark"),
        entry("ui_scale", "1.25"),
        entry("history_days", "30"),
    ];
    store.save(&entries).unwrap();
    assert_eq!(store.load().unwrap(), entries);
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert!(text.contains("history_days: 30\n"), "{text}");
    assert!(text.contains("ui_scale: 1.25\n"), "{text}");
}

#[test]
fn lists_and_mappings_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.yml");
    std::fs::write(
        &file,
        "theme: light\nodd: [1, 2]\nnested:\n  a: 1\nflag: true\n",
    )
    .unwrap();
    let store = FsSettingsStore::new(&file);
    assert_eq!(
        store.load().unwrap(),
        [entry("theme", "light"), entry("flag", "true")]
    );
}
