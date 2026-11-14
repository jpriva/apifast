//! `apifast send` end to end: the real binary, a collection on disk and a
//! local HTTP server. No test uses the external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

/// A threaded HTTP/1.1 server that answers by path and records requests.
struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = log.clone();
                thread::spawn(move || answer(stream, &log));
            }
        });
        Self { port, seen }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

fn answer(stream: TcpStream, log: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut head = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        head.push_str(&line);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok();
    let request = format!("{head}\r\n{}", String::from_utf8_lossy(&body));
    log.lock().unwrap().push(request.clone());

    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let (status, body) = match path.as_str() {
        p if p.starts_with("/missing") => ("404 Not Found", "nothing here".to_owned()),
        "/big" => ("200 OK", "x".repeat(300_000)),
        _ => ("200 OK", format!("{{\"path\": \"{path}\"}}")),
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nX-Served-By: test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// A collection like one made in Bruno, pointing at `port`.
fn collection(port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: API\nrequest:\n  headers:\n    - name: accept\n      value: application/json\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
        ),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\nrequest:\n  auth: inherit\n",
        ),
        (
            "users/get.yml",
            "info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users/:id\"\n  params:\n    - name: id\n      value: \"{{user_id}}\"\n      type: path\n  auth: inherit\n",
        ),
        (
            "users/missing.yml",
            "info:\n  name: Missing\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/missing\"\n",
        ),
        (
            "users/big.yml",
            "info:\n  name: Big\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/big\"\n",
        ),
        (
            "users/create.yml",
            "info:\n  name: Create\n  type: http\nhttp:\n  method: POST\n  url: \"{{host}}/users\"\n  body:\n    type: json\n    data: '{\"name\": \"{{name}}\"}'\n",
        ),
        (
            "users/ask.yml",
            "info:\n  name: Ask\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/ask/{{?Code}}\"\n",
        ),
        ("graphql.yml", "info:\n  name: Q\n  type: graphql\n"),
        (
            "environments/local.yml",
            &format!(
                "name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:{port}\n  - name: user_id\n    value: \"7\"\n  - name: token\n    value: t0k3n\n  - name: name\n    value: Ana\n"
            ),
        ),
        (
            "environments/vault.yml",
            "name: vault\nextends: local\nvariables:\n  - name: token\n    secret: true\n",
        ),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

fn apifast(dir: &Path, data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_apifast"))
        .args(args)
        .current_dir(dir)
        .env("APIFAST_DATA_DIR", data)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

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
    // An empty path parameter is left as written, as Bruno does.
    assert!(
        server.seen()[0].starts_with("GET /users/:id "),
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
            &["send", "graphql.yml"],
            "error: `graphql.yml`: `graphql` requests is not supported yet\n",
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
fn usage_errors_exit_with_2_and_version_works() {
    let data = tempfile::tempdir().unwrap();
    let out = apifast(data.path(), data.path(), &[]);
    assert_eq!(out.status.code(), Some(2));
    let out = apifast(data.path(), data.path(), &["--version"]);
    assert_eq!(
        text(&out.stdout),
        format!("apifast {}\n", env!("CARGO_PKG_VERSION"))
    );
}
