//! `send_request` storing bodies on disk.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryTransport,
};
use apifast_app::{
    AppError, CancelToken, NoPrompter, RequestPath, ResponseStore, SendOptions, SendPorts,
    TransportError, send_request,
};
use apifast_domain::{Collection, HttpRequest, Method};
use apifast_history::FsResponseStore;

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
fn a_sent_response_is_stored_and_a_cancelled_one_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsResponseStore::new(dir.path());
    let collections = MemoryCollections::new(Collection::new("c")).with_request(
        "r.yml",
        HttpRequest::new("r", Method::Get, "https://api.test"),
    );
    let environments = MemoryEnvironments::default();
    let transport = MemoryTransport::new(200, "stored on disk");
    let (clock, random) = (FixedClock(0), CountingRandom::new(0));
    let ports = || SendPorts {
        collections: &collections,
        environments: &environments,
        transport: &transport,
        responses: &store,
        clock: &clock,
        random: &random,
        prompter: &NoPrompter,
        report: &|_| {},
    };
    let path = RequestPath::new("r.yml");

    let outcome = block_on(send_request(
        ports(),
        &path,
        &SendOptions::default(),
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(
        store.read(&outcome.body.id, 0, 100).unwrap(),
        b"stored on disk"
    );

    let cancel = CancelToken::new();
    cancel.cancel();
    let result = block_on(send_request(
        ports(),
        &path,
        &SendOptions::default(),
        &cancel,
    ));
    assert_eq!(result, Err(AppError::Transport(TransportError::Cancelled)));
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names.len(),
        1,
        "only the first response is stored: {names:?}"
    );
}
