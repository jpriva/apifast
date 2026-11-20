//! A build without the `grpc` feature: gRPC requests say they cannot be
//! called. Runs only in such a build (`--no-default-features`).

#![cfg(not(feature = "grpc"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

mod common;

use common::{apifast, collection, text};

#[test]
fn a_grpc_request_says_this_build_has_no_grpc() {
    let dir = collection(1);
    std::fs::write(
        dir.path().join("call.yml"),
        "info:\n  name: Call\n  type: grpc\ngrpc:\n  url: localhost:1\n  method: /a.B/C\n  message:\n    - title: m\n      message: '{}'\n",
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    for command in ["send", "describe"] {
        let out = apifast(
            dir.path(),
            data.path(),
            &[command, "call.yml", "-e", "local"],
        );
        assert!(!out.status.success());
        assert_eq!(
            text(&out.stderr),
            "error: this build of apifast has no gRPC support\n",
            "{command}"
        );
    }
}
