//! The use cases of `apifast-app` running on a collection on disk.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{CountingRandom, FixedClock, MemoryResponses, MemoryTransport};
use apifast_app::{CancelToken, NoPrompter, RequestPath, SendOptions, SendPorts, send_request};
use apifast_storage_fs::FsCollection;

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory ports complete on the first poll"),
    }
}

#[test]
fn sends_a_bruno_collection_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: API\nrequest:\n  headers:\n    - name: accept\n      value: application/json\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
        ),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  type: folder\nrequest:\n  auth: inherit\n  variables:\n    - name: resource\n      value: users\n",
        ),
        (
            "users/get.yml",
            "info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/{{resource}}/:id?expand=true\"\n  params:\n    - name: id\n      value: \"{{process.env.USER_ID}}\"\n      type: path\n    - name: expand\n      value: \"true\"\n      type: query\n  auth: inherit\nsettings:\n  timeout: 5000\n",
        ),
        (
            "environments/base.yml",
            "name: base\nvariables:\n  - name: host\n    value: http://localhost\n",
        ),
        (
            "environments/prod.yml",
            "name: prod\nextends: base\nvariables:\n  - name: host\n    value: https://api.test\n  - name: token\n    value: t0k3n\n",
        ),
        (".env", "USER_ID=981\n"),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }
    let repo = FsCollection::open(dir.path()).unwrap();
    let transport = MemoryTransport::new(200, "{}");
    let ports = SendPorts {
        collections: &repo,
        environments: &repo,
        transport: &transport,
        responses: &MemoryResponses::default(),
        clock: &FixedClock(0),
        random: &CountingRandom::new(0),
        prompter: &NoPrompter,
        secrets: &apifast_app::NoSecrets,
        report: &|_| {},
        history: None,
        tokens: None,
        authorizer: None,
    };
    let options = SendOptions {
        environment: Some("prod".into()),
        ..SendOptions::default()
    };
    let outcome = block_on(send_request(
        ports,
        &RequestPath::new("users/get.yml"),
        &options,
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(outcome.head.status, 200);

    let sent = transport.sent().pop().unwrap();
    assert_eq!(sent.url(), "https://api.test/users/981?expand=true");
    assert_eq!(sent.timeout_ms(), 5000);
    let headers: Vec<_> = sent
        .headers()
        .iter()
        .map(|(n, v)| format!("{n}: {v}"))
        .collect();
    assert_eq!(
        headers,
        ["accept: application/json", "Authorization: Bearer t0k3n"]
    );
}
