//! Every Postman export of the corpus imports, and its plan is consistent.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::Path;

use apifast_app::plan_import;
use apifast_import::{import_postman, import_postman_environment};

#[test]
fn every_collection_of_the_corpus_imports() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postman");
    let mut collections = 0;
    let mut requests = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        if name.ends_with("postman_environment.json") {
            import_postman_environment(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            continue;
        }
        let imported = import_postman(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let plan = plan_import(&imported);
        let mut paths: Vec<&str> = plan.entries.iter().map(|e| e.path.as_str()).collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "{name}: two entries share a path");
        collections += 1;
        requests += plan.counts().1;
    }
    assert!(collections >= 30, "the corpus is there: {collections}");
    assert!(requests > 100, "{requests}");
}

/// Each collection of the corpus, imported, exported and imported again,
/// gives the same tree and requests: the export loses nothing the importer
/// reads.
#[test]
fn every_collection_of_the_corpus_survives_an_export() {
    use apifast_app::ImportedItem;
    use apifast_import::export_postman;

    fn flat(items: &[ImportedItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                ImportedItem::Folder { folder, items } => {
                    let mut folder = folder.clone();
                    folder.defaults.scripts.clear();
                    out.push(format!("{folder:?}"));
                    flat(items, out);
                }
                // Scripts are not exported: they use Bruno's API.
                ImportedItem::Request { request, .. } => {
                    let mut request = request.clone();
                    request.scripts.clear();
                    out.push(format!("{request:?}"));
                }
            }
        }
    }

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postman");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().is_none_or(|e| e != "json")
            || name.ends_with("postman_environment.json")
        {
            continue;
        }
        let first = import_postman(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let (export, _) = export_postman(&first);
        let second = import_postman(&export.to_string())
            .unwrap_or_else(|e| panic!("{name}: the export is not read back: {e}"));
        let (mut before, mut after) = (Vec::new(), Vec::new());
        flat(&first.items, &mut before);
        flat(&second.items, &mut after);
        assert_eq!(before.len(), after.len(), "{name}");
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(b, a, "{name}");
        }
        assert_eq!(first.defaults.auth, second.defaults.auth, "{name}");
        checked += 1;
    }
    assert!(checked >= 30, "{checked}");
}
