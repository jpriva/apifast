//! A build without the `git` feature: `apifast git` says it has none. Runs
//! only in such a build (`--no-default-features`).

#![cfg(not(feature = "git"))]
#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use common::{apifast, collection, text};

#[test]
fn git_says_this_build_has_no_git() {
    let dir = collection(1);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(dir.path(), data.path(), &["git", "status"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("this build of apifast has no git"),
        "{}",
        text(&out.stderr)
    );
}
