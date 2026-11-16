//! Assertions and captures in `apifast send`, end to end.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use common::{Server, apifast, collection, text};

const CHECKED: &str = "info:
  name: Checked
  type: http
http:
  method: GET
  url: \"{{host}}/users/{{user_id}}\"
runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: \"200\"
    - expression: res.body.path
      operator: eq
      value: \"/users/{{user_id}}\"
    - expression: res.body.path
      operator: startsWith
      value: \"{{expected}}\"
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.nothing
        method: jsonq
      variable:
        name: nothing
        scope: runtime
";

#[test]
fn assertions_are_reported_and_a_failure_exits_with_1() {
    let server = Server::start();
    let dir = collection(server.port);
    std::fs::write(dir.path().join("users/checked.yml"), CHECKED).unwrap();
    let data = tempfile::tempdir().unwrap();
    let send = |expected: &str, quiet: bool| {
        let var = format!("expected={expected}");
        let mut args = vec!["send", "users/checked.yml", "-e", "local", "--var", &var];
        if quiet {
            args.push("-q");
        }
        apifast(dir.path(), data.path(), &args)
    };

    let ok = send("/users", false);
    let stderr = text(&ok.stderr);
    assert_eq!(ok.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("pass  res.status eq 200"), "{stderr}");
    assert!(
        stderr.contains("pass  res.body.path eq /users/7"),
        "{stderr}"
    );
    assert!(
        stderr.contains("warning: capture of `nothing`: res.body.nothing is undefined"),
        "{stderr}"
    );
    assert_eq!(text(&ok.stdout), "{\"path\": \"/users/7\"}");

    let failed = send("/admins", true);
    let stderr = text(&failed.stderr);
    assert_eq!(failed.status.code(), Some(1), "{stderr}");
    assert!(
        !stderr.contains("pass  "),
        "quiet shows only failures: {stderr}"
    );
    assert!(
        stderr.contains(
            "FAIL  res.body.path startsWith /admins: expected startsWith /admins, got \"/users/7\""
        ),
        "{stderr}"
    );
}
