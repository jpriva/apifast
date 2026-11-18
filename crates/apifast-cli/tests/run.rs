//! `apifast run`, end to end.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use std::path::Path;

use common::{Server, apifast, text};

/// A small collection: `login` captures a value, `users/list` is tagged
/// `smoke` and reads `{{id}}`, `users/missing` fails its assertion.
fn shop(port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: Shop\n".to_owned()),
        (
            "environments/local.yml",
            format!("name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:{port}\n  - name: id\n    value: \"0\"\n"),
        ),
        (
            "login.yml",
            "info:\n  name: Login\n  type: http\n  seq: 1\nhttp:\n  method: GET\n  url: \"{{host}}/login\"\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n  actions:\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.body.path\n        method: jsonq\n      variable:\n        name: from\n        scope: runtime\n".to_owned(),
        ),
        ("users/folder.yml", "info:\n  name: Users\n  type: folder\n".to_owned()),
        (
            "users/list.yml",
            "info:\n  name: List\n  type: http\n  seq: 1\n  tags: [smoke]\nhttp:\n  method: GET\n  url: \"{{host}}/users/{{id}}\"\nruntime:\n  assertions:\n    - expression: res.body.path\n      operator: eq\n      value: \"/users/{{id}}\"\n".to_owned(),
        ),
        (
            "users/missing.yml",
            "info:\n  name: Missing\n  type: http\n  seq: 2\nhttp:\n  method: GET\n  url: \"{{host}}/missing?from={{from}}\"\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n".to_owned(),
        ),
    ];
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    dir
}

fn run(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let data = tempfile::tempdir().unwrap();
    let mut all = vec!["run"];
    all.extend(args);
    let out = apifast(dir, data.path(), &all);
    (out.status.code(), text(&out.stdout), text(&out.stderr))
}

#[test]
fn a_failing_assertion_exits_with_1_and_reports_are_written() {
    let server = Server::start();
    let dir = shop(server.port);
    let (code, stdout, stderr) = run(
        dir.path(),
        &[
            "-r",
            "-e",
            "local",
            "--reporter",
            "junit=out/report.xml",
            "--reporter",
            "json=out/report.json",
            "--reporter",
            "html=out/report.html",
        ],
    );
    assert_eq!(code, Some(1), "{stdout}{stderr}");
    assert!(
        stdout.starts_with("Running Shop with environment local\n"),
        "{stdout}"
    );
    assert!(stdout.contains("PASS  Login login.yml"), "{stdout}");
    assert!(
        stdout.contains("FAIL  Missing users/missing.yml"),
        "{stdout}"
    );
    assert!(
        stdout.contains("✗ res.status eq 200: expected eq 200, got 404"),
        "{stdout}"
    );
    assert!(
        stdout.contains("3 requests: 2 passed, 1 failed · 2/3 assertions"),
        "{stdout}"
    );
    // No color when not on a terminal.
    assert!(!stdout.contains('\x1b'), "{stdout}");
    // The capture of login reached the next requests.
    assert!(
        server
            .seen()
            .iter()
            // Encoded as Bruno does with `encodeUrl` on, the default.
            .any(|r| r.starts_with("GET /missing?from=%2Flogin ")),
        "{:?}",
        server.seen()
    );

    let junit = std::fs::read_to_string(dir.path().join("out/report.xml")).unwrap();
    assert!(
        junit.contains("<testsuites name=\"Shop\" tests=\"3\" failures=\"1\" errors=\"0\""),
        "{junit}"
    );
    let json = std::fs::read_to_string(dir.path().join("out/report.json")).unwrap();
    assert!(json.contains("\"succeeded\":false"), "{json}");
    let html = std::fs::read_to_string(dir.path().join("out/report.html")).unwrap();
    assert!(html.contains("<title>Shop run</title>"));
}

#[test]
fn a_folder_runs_with_tags_and_data_rows() {
    let server = Server::start();
    let dir = shop(server.port);
    std::fs::write(dir.path().join("ids.csv"), "id\n1\n2\n").unwrap();
    let (code, stdout, stderr) = run(
        dir.path(),
        &[
            "users", "-e", "local", "--tags", "smoke", "--data", "ids.csv", "-q",
        ],
    );
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let paths: Vec<String> = server
        .seen()
        .iter()
        .map(|r| r.split_whitespace().nth(1).unwrap().to_owned())
        .collect();
    assert_eq!(paths, ["/users/1", "/users/2"]);
    // Quiet: only the summary.
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(
        stdout.starts_with("2 requests: 2 passed, 1 skipped · 2/2 assertions"),
        "{stdout}"
    );
}

#[test]
fn bail_stops_and_errors_exit_with_2() {
    let server = Server::start();
    let dir = shop(server.port);
    std::fs::write(dir.path().join("users/zzz.yml"), "info:\n  name: Later\n  type: http\n  seq: 3\nhttp:\n  method: GET\n  url: \"{{host}}/later\"\n").unwrap();
    let (code, stdout, _) = run(
        dir.path(),
        &["users", "-e", "local", "--var", "from=x", "--bail"],
    );
    assert_eq!(code, Some(1), "{stdout}");
    assert!(stdout.contains("stopped at the first failure"), "{stdout}");
    assert!(!server.seen().iter().any(|r| r.contains("/later")));

    // Without an environment, `host` is missing: the requests cannot be sent.
    let (code, stdout, _) = run(dir.path(), &[]);
    assert_eq!(code, Some(2), "{stdout}");
    assert!(stdout.contains("ERROR Login login.yml"), "{stdout}");
}

#[test]
fn usage_errors_exit_with_3() {
    let dir = shop(1);
    for args in [
        &["--iterations", "0"][..],
        &["--reporter", "pdf=x.pdf"],
        &["--data", "x.csv", "--iterations", "2"],
        &["--quiet", "--verbose"],
    ] {
        let (code, _, stderr) = run(dir.path(), args);
        assert_eq!(code, Some(3), "{args:?}: {stderr}");
    }
    let (code, _, stderr) = run(dir.path(), &["login.yml"]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("is a file; run a folder"), "{stderr}");
    let (code, _, stderr) = run(dir.path(), &["--data", "ids.txt"]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("must end in .csv or .json"), "{stderr}");
}
