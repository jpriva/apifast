//! `apifast describe` end to end: the real binary and a collection on
//! disk, with a `.proto` file. No test uses the network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

mod common;

use common::{apifast, collection, text};

#[test]
fn a_grpc_request_lists_the_methods_of_its_proto() {
    let dir = collection(1);
    std::fs::create_dir_all(dir.path().join("protos")).unwrap();
    std::fs::write(
        dir.path().join("protos/hello.proto"),
        "syntax = \"proto3\";\npackage hello;\nmessage Req { string greeting = 1; }\nmessage Rep { string reply = 1; }\nservice Greeter {\n  rpc SayHello(Req) returns (Rep);\n  rpc LotsOfReplies(Req) returns (stream Rep);\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("hello.yml"),
        "info:\n  name: Hello\n  type: grpc\ngrpc:\n  url: localhost:1\n  method: /hello.Greeter/SayHello\n  protoFilePath: protos/hello.proto\n",
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["describe", "hello.yml", "-e", "local"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "/hello.Greeter/SayHello       unary             hello.Req -> hello.Rep\n\
         /hello.Greeter/LotsOfReplies  server-streaming  hello.Req -> hello.Rep\n"
    );

    // Any other request says what it is.
    let out = apifast(
        dir.path(),
        data.path(),
        &["describe", "users/get.yml", "-e", "local"],
    );
    assert!(!out.status.success());
    assert_eq!(
        text(&out.stderr),
        "error: `users/get.yml` is not a gRPC request\n"
    );
}
