//! Every OpenAPI spec of the corpus imports (or, if broken, says so), and
//! its plan is consistent.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::Path;

use apifast_app::plan_import;
use apifast_import::import_openapi;
use apifast_storage_fs::yaml_to_json;

#[test]
fn every_spec_of_the_corpus_imports() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openapi");
    let (mut specs, mut requests) = (0, 0);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if !matches!(extension.as_deref(), Some("json" | "yaml")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let broken = name.contains("broken") || name.contains("malformed");
        let value = match yaml_to_json(&text) {
            Ok(value) => value,
            Err(_) if broken => continue,
            Err(e) => panic!("{name}: {e}"),
        };
        assert!(!broken, "{name} reads, but is meant to be broken");
        let imported = import_openapi(&value).unwrap_or_else(|e| panic!("{name}: {e}"));
        let plan = plan_import(&imported);
        let mut paths: Vec<&str> = plan.entries.iter().map(|e| e.path.as_str()).collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "{name}");
        specs += 1;
        requests += plan.counts().1;
    }
    assert!(specs >= 15, "{specs}");
    assert!(requests > 40, "{requests}");
}
