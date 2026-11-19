//! Bruno's own test collection, in `.bru`, imports whole: every file
//! reads, and every request has a URL.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::Path;

use apifast_app::{ImportedItem, plan_import};
use apifast_import::import_bruno;

fn requests(items: &[ImportedItem], out: &mut Vec<String>) {
    for item in items {
        match item {
            ImportedItem::Request(request) => {
                assert!(!request.url.is_empty(), "`{}` has no URL", request.name);
                out.push(request.name.clone());
            }
            ImportedItem::Folder { items, .. } => requests(items, out),
        }
    }
}

#[test]
fn brunos_test_collection_imports_whole() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bruno/bruno-tests");
    let collection = import_bruno(&dir).unwrap();
    assert_eq!(collection.name, "bruno-testbench");
    let unreadable: Vec<_> = (collection.warnings.iter())
        .filter(|w| w.message.contains("cannot be read"))
        .collect();
    assert!(unreadable.is_empty(), "{unreadable:#?}");
    let mut names = Vec::new();
    requests(&collection.items, &mut names);
    assert!(names.len() > 200, "{}", names.len());
    assert_eq!(collection.environments.len(), 2);
    let plan = plan_import(&collection);
    let mut paths: Vec<&str> = plan.entries.iter().map(|e| e.path.as_str()).collect();
    let total = paths.len();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(paths.len(), total);
}
