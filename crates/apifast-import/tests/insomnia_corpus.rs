//! Every Insomnia export of the corpus imports, or is refused if it is
//! broken on purpose.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::Path;

use apifast_app::plan_import;
use apifast_import::import_insomnia;
use apifast_storage_fs::yaml_to_json;

#[test]
fn every_export_of_the_corpus_imports() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/insomnia");
    let mut imported = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let broken = name.contains("malformed") || name.contains("invalid");
        let text = std::fs::read_to_string(&path).unwrap();
        let result =
            yaml_to_json(&text).and_then(|v| import_insomnia(&v).map_err(|e| e.to_string()));
        match result {
            Ok(collection) => {
                assert!(!broken, "{name} is meant to be broken");
                let plan = plan_import(&collection);
                assert!(plan.counts().1 > 0, "{name}");
                imported += 1;
            }
            Err(e) => assert!(broken, "{name}: {e}"),
        }
    }
    assert!(imported >= 5, "{imported}");
}
