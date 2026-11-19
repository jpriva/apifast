//! `apifast send` end to end: the real binary, a collection on disk and a
//! local HTTP server. No test uses the external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

mod common;

use std::net::TcpListener;

use common::{Server, apifast, collection, text};

#[test]
fn sends_a_request_and_prints_the_body_to_stdout() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();

    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/get.yml", "--env", "local"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), r#"{"path": "/users/7"}"#);
    let summary = text(&out.stderr);
    assert!(
        summary.starts_with("200 OK · ") && summary.contains(" ms · 20 B"),
        "{summary}"
    );

    let request = &server.seen()[0];
    assert!(request.starts_with("GET /users/7 HTTP/1.1"), "{request}");
    assert!(request.contains("authorization: Bearer t0k3n"), "{request}");
    assert!(request.contains("accept: application/json"), "{request}");
}

#[test]
fn the_response_is_stored_in_the_data_directory() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    apifast(
        dir.path(),
        data.path(),
        &["send", "users/get.yml", "-e", "local"],
    );

    let responses: Vec<_> = std::fs::read_dir(data.path().join("responses"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(responses.len(), 1, "one folder for the collection");
    assert!(
        responses[0]
            .file_name()
            .to_string_lossy()
            .starts_with("path-")
    );
    let bodies: Vec<_> = std::fs::read_dir(responses[0].path())
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        std::fs::read_to_string(bodies[0].path()).unwrap(),
        r#"{"path": "/users/7"}"#
    );
    // Sending never writes into the collection.
    assert!(!dir.path().join(".apifast").exists());
}

#[test]
fn include_prints_the_head_and_quiet_hides_the_summary() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/get.yml", "-e", "local", "-i", "-q"],
    );
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("HTTP/1.1 200 OK\n"), "{stdout}");
    assert!(stdout.contains("x-served-by: test\n"), "{stdout}");
    assert!(stdout.ends_with("\n\n{\"path\": \"/users/7\"}"), "{stdout}");
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn vars_override_every_scope() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &[
            "send",
            "users/create.yml",
            "-e",
            "local",
            "--var",
            "name=Bea",
            "--var",
            "user_id=9",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let request = &server.seen()[0];
    assert!(request.contains(r#"{"name": "Bea"}"#), "{request}");
    assert!(
        request.contains("content-type: application/json"),
        "{request}"
    );
}

#[test]
fn a_missing_variable_fails_unless_allowed() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();

    // Without an environment, `host` is not defined.
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/get.yml", "--var", "user_id=1"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), "error: variable `host` is not defined\n");
    assert_eq!(server.seen().len(), 0, "nothing must be sent");

    // Allowed: missing values are empty and each is warned about.
    let host = format!("host=http://127.0.0.1:{}", server.port);
    let out = apifast(
        dir.path(),
        data.path(),
        &[
            "send",
            "users/get.yml",
            "--var",
            &host,
            "--allow-unresolved",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("warning: variable `user_id` is not defined\nwarning: variable `token` is not defined\n200 OK"),
        "{stderr}"
    );
    // An empty path parameter is left as written, then encoded with the
    // rest of the path, as Bruno does.
    assert!(
        server.seen()[0].starts_with("GET /users/%3Aid "),
        "{}",
        server.seen()[0]
    );

    // Warnings are shown before sending, so a failed send still shows them.
    let out = apifast(
        dir.path(),
        data.path(),
        &[
            "send",
            "users/get.yml",
            "--var",
            "host=http://127.0.0.1:1",
            "--allow-unresolved",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("warning: variable `user_id` is not defined\n"),
        "{stderr}"
    );
    assert!(stderr.contains("error: cannot connect"), "{stderr}");
}

#[test]
fn a_secret_without_a_value_is_reported_by_name_only() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/get.yml", "-e", "vault"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "error: secret `token` has no value available\n"
    );
}

#[test]
fn fail_turns_http_errors_into_exit_code_4() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/missing.yml", "-e", "local"],
    );
    assert!(out.status.success());
    assert!(text(&out.stderr).starts_with("404 Not Found"));

    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/missing.yml", "-e", "local", "--fail"],
    );
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(text(&out.stdout), "nothing here");
}

#[test]
fn large_bodies_are_copied_whole() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/big.yml", "-e", "local"],
    );
    assert_eq!(out.stdout.len(), 300_000);
    assert!(
        text(&out.stderr).contains("293.0 KB"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn prompts_without_a_terminal_count_as_missing() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "users/ask.yml", "-e", "local"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "error: no value given for prompt `Code`\n"
    );
}

#[test]
fn errors_are_one_clear_line() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let cases: [(&[&str], &str); 5] = [
        (
            &["send", "users/get.yml", "-e", "staging"],
            "error: environment `staging` does not exist\n",
        ),
        (
            &["send", "socket.yml"],
            "error: WebSocket requests cannot connect yet\n",
        ),
        (&["send", "nope.yml"], "error: cannot open `nope.yml`"),
        (
            &[
                "send",
                "users/get.yml",
                "-e",
                "local",
                "--collection",
                "users",
            ],
            "error: `",
        ),
        (
            &["send", "users/get.yml", "--var", "oops"],
            "error: invalid value 'oops' for '--var <NAME=VALUE>'",
        ),
    ];
    for (args, expected) in cases {
        let out = apifast(dir.path(), data.path(), args);
        assert!(!out.status.success(), "{args:?}");
        assert!(
            text(&out.stderr).starts_with(expected),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn a_refused_connection_fails_without_leaking_the_query() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = collection(port);
    let data = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("leak.yml"),
        "info:\n  name: L\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/x?api_key=SECRET123\"\n",
    )
    .unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "leak.yml", "-e", "local"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with(&format!("error: cannot connect to `127.0.0.1:{port}`")),
        "{stderr}"
    );
    assert!(!stderr.contains("SECRET123"), "{stderr}");
}

#[test]
fn usage_errors_exit_with_3_and_version_works() {
    let data = tempfile::tempdir().unwrap();
    let out = apifast(data.path(), data.path(), &[]);
    assert_eq!(out.status.code(), Some(3));
    let out = apifast(data.path(), data.path(), &["--version"]);
    assert_eq!(
        text(&out.stdout),
        format!("apifast {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn a_graphql_request_sends_its_query_and_variables_as_json() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    // As Bruno writes it.
    std::fs::write(
        dir.path().join("me.yml"),
        "info:\n  name: Me\n  type: graphql\n\ngraphql:\n  method: POST\n  url: \"{{host}}/graphql\"\n  body:\n    query: |-\n      query($id: ID!) { user(id: $id) { name } }\n    variables: |-\n      {\"id\": \"{{user_id}}\"}\n  auth: none\n",
    )
    .unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "me.yml", "--env", "local"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let request = &server.seen()[0];
    assert!(request.starts_with("POST /graphql HTTP/1.1"), "{request}");
    assert!(
        request.contains("content-type: application/json"),
        "{request}"
    );
    assert!(
        request.ends_with(
            r#"{"query":"query($id: ID!) { user(id: $id) { name } }","variables":{"id":"7"}}"#
        ),
        "{request}"
    );

    std::fs::write(
        dir.path().join("bad.yml"),
        "info:\n  name: Bad\n  type: graphql\ngraphql:\n  url: \"{{host}}/graphql\"\n  body:\n    query: \"{ a }\"\n    variables: \"[1]\"\n",
    )
    .unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["send", "bad.yml", "--env", "local"],
    );
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("must be a JSON object"),
        "{}",
        text(&out.stderr)
    );
}
