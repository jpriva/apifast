//! OAuth 2.0 in `apifast send` and `apifast run`, against a local token
//! endpoint and API.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use common::{apifast, text};

/// `/token` gives `abc` for client `cli` with secret `shh` (in the body);
/// anything else needs `Authorization: Bearer abc`.
fn server() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                head.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).ok();
            let body = String::from_utf8_lossy(&body).into_owned();
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let (status, reply) = if path == "/token" {
                seen.lock().unwrap().push(format!("token {body}"));
                if body.contains("client_secret=shh") {
                    (
                        "200 OK",
                        r#"{"access_token":"abc","token_type":"Bearer","expires_in":3600}"#,
                    )
                } else {
                    ("401 Unauthorized", r#"{"error":"invalid_client"}"#)
                }
            } else if head.contains("authorization: Bearer abc") {
                seen.lock().unwrap().push(format!("api {path}"));
                ("200 OK", "{}")
            } else {
                ("401 Unauthorized", "{}")
            };
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (port, log)
}

fn collection(port: u16, secret: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let auth = format!(
        "request:\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: http://127.0.0.1:{port}/token\n    credentials:\n      clientId: cli\n      clientSecret: {secret}\n      placement: body\n"
    );
    let request = |name: &str, seq: u32| {
        format!(
            "info:\n  name: {name}\n  type: http\n  seq: {seq}\nhttp:\n  method: GET\n  url: http://127.0.0.1:{port}/{name}\n  auth: inherit\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n"
        )
    };
    for (path, content) in [
        (
            "opencollection.yml",
            format!("opencollection: 1.0.0\ninfo:\n  name: Shop\n{auth}"),
        ),
        ("orders.yml", request("orders", 1)),
        ("users.yml", request("users", 2)),
    ] {
        std::fs::write(dir.path().join(path), content).unwrap();
    }
    dir
}

#[test]
fn a_run_gets_the_token_once_and_sends_it() {
    let (port, log) = server();
    let dir = collection(port, "shh");
    let data = tempfile::tempdir().unwrap();
    let out = apifast(dir.path(), data.path(), &["run"]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(&out.stderr));
    let log = log.lock().unwrap().clone();
    assert_eq!(log.len(), 3, "{log:?}");
    assert!(log[0].starts_with("token ") && log[0].contains("grant_type=client_credentials"));
    assert_eq!(log[1..], ["api /orders", "api /users"]);
}

#[test]
fn a_token_error_says_why() {
    let (port, _) = server();
    let dir = collection(port, "wrong");
    let data = tempfile::tempdir().unwrap();
    let out = apifast(dir.path(), data.path(), &["send", "orders.yml"]);
    let stderr = text(&out.stderr);
    assert_ne!(out.status.code(), Some(0));
    assert!(
        stderr.contains("the token request failed (401): invalid_client"),
        "{stderr}"
    );
}
