//! What the end-to-end tests of the binary share: a local HTTP server, a
//! collection on disk, and running `apifast`.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: each test file uses some; a failure fails the test"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

/// A threaded HTTP/1.1 server that answers by path and records requests.
pub struct Server {
    pub port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    pub fn start() -> Self {
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

    pub fn seen(&self) -> Vec<String> {
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
    // A login sets a session cookie, for the tests of the cookie jar.
    let cookie = if path == "/login" {
        "Set-Cookie: sid=from-login; Path=/\r\n"
    } else {
        ""
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nX-Served-By: test\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// A collection like one made in Bruno, pointing at `port`.
pub fn collection(port: u16) -> tempfile::TempDir {
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

pub fn apifast(dir: &Path, data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_apifast"))
        .args(args)
        .current_dir(dir)
        .env("APIFAST_DATA_DIR", data)
        // The user's keyring is never touched by tests.
        .env("APIFAST_NO_KEYRING", "1")
        .env_remove("APIFAST_COLLECTION_KEY")
        .output()
        .unwrap()
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs the binary with `stdin` and extra environment variables.
pub fn apifast_with(
    dir: &Path,
    data: &Path,
    args: &[&str],
    stdin: &str,
    env: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_apifast"));
    command
        .args(args)
        .current_dir(dir)
        .env("APIFAST_DATA_DIR", data)
        .env("APIFAST_NO_KEYRING", "1")
        .env_remove("APIFAST_COLLECTION_KEY")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
