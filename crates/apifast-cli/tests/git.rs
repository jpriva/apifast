//! `apifast git` on a collection in a real repository. Runs only in a build
//! with the `git` feature (the default).

#![cfg(feature = "git")]
#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use apifast_git_contract::{git, git_output, repo, write};
use common::{apifast, text};

#[test]
fn status_diff_commit_and_branches_of_a_collection() {
    let data = tempfile::tempdir().unwrap();
    let dir = repo();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\n",
    );
    write(
        root,
        "users/list.yml",
        "info:\n  name: List\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    let run = |args: &[&str]| apifast(root, data.path(), args);

    let out = run(&["git", "status"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "On main\nNothing changed since the last commit\n"
    );

    write(
        root,
        "users/list.yml",
        "info:\n  name: List\n  type: http\nhttp:\n  method: POST\n  url: \"{{host}}/users\"\n  headers:\n    - name: X-Team\n      value: core\n",
    );
    write(root, "notes.txt", "n\n");
    assert_eq!(
        text(&run(&["git", "status"]).stdout),
        "On main\nA notes.txt\nM users/list.yml\n"
    );
    assert_eq!(
        text(&run(&["git", "diff", "users/list.yml"]).stdout),
        "users/list.yml (modified)\n  method: GET → POST\n  header x-team: (none) → X-Team: core\n"
    );

    let out = run(&["git", "commit", "-m", "Post users", "users/list.yml"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).starts_with("Committed 1 file(s) on main: "));
    assert_eq!(
        git_output(root, &["log", "-1", "--format=%s"]),
        "Post users"
    );
    assert_eq!(
        text(&run(&["git", "status"]).stdout),
        "On main\nA notes.txt\n"
    );

    let out = run(&["git", "commit", "-m", "x", "nope.yml"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("`nope.yml` has no changes"));

    git(root, &["branch", "feature"]);
    assert_eq!(
        text(&run(&["git", "branches"]).stdout),
        "  feature\n* main\n"
    );
    let out = run(&["git", "switch", "feature"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feature"
    );

    let out = run(&["git", "commit", "-m", "All", "--all"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&run(&["git", "status"]).stdout),
        "On feature\nNothing changed since the last commit\n"
    );
}

#[test]
fn a_collection_in_no_repository_says_so() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\n",
    );
    let out = apifast(dir.path(), data.path(), &["git", "status"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("is not in a git repository"));
}
