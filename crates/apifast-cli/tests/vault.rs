//! `apifast vault` end to end: secrets stored and used by `send`, shared
//! between two machines (two data directories), and the key in CI.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use common::{Server, apifast_with, collection, text};

/// The `Authorization` header the server last saw.
fn authorization(server: &Server) -> String {
    server
        .seen()
        .last()
        .unwrap()
        .lines()
        .find(|l| l.to_lowercase().starts_with("authorization:"))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_local_secret_is_stored_from_stdin_and_sent() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let run = |args: &[&str], stdin: &str| apifast_with(dir.path(), data.path(), args, stdin, &[]);

    let status = run(&["vault", "status"], "");
    assert_eq!(text(&status.stdout), "vault\ttoken\tno value\n");

    let set = run(&["vault", "set", "vault", "token"], "s3cr3t\n");
    assert!(set.status.success(), "{}", text(&set.stderr));
    assert!(
        dir.path().join(".apifast/collection.yml").exists(),
        "it got an id"
    );
    let status = run(&["vault", "status"], "");
    assert_eq!(
        text(&status.stdout),
        "vault\ttoken\tstored on this machine\n"
    );

    let send = run(&["send", "users/get.yml", "-e", "vault"], "");
    assert!(send.status.success(), "{}", text(&send.stderr));
    assert_eq!(authorization(&server), "authorization: Bearer s3cr3t");
}

#[test]
fn only_declared_secrets_can_be_set_and_never_from_arguments() {
    let server = Server::start();
    let dir = collection(server.port);
    let data = tempfile::tempdir().unwrap();
    let set = apifast_with(
        dir.path(),
        data.path(),
        &["vault", "set", "local", "host"],
        "x",
        &[],
    );
    assert!(!set.status.success());
    assert!(
        text(&set.stderr).contains("is not a secret"),
        "{}",
        text(&set.stderr)
    );
    let empty = apifast_with(
        dir.path(),
        data.path(),
        &["vault", "set", "vault", "token"],
        "",
        &[],
    );
    assert!(!empty.status.success());
}

#[test]
fn a_shared_secret_needs_the_key_on_another_machine_or_in_ci() {
    let server = Server::start();
    let dir = collection(server.port);
    let (alice, bob) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let on = |data: &tempfile::TempDir, args: &[&str], stdin: &str, env: &[(&str, &str)]| {
        apifast_with(dir.path(), data.path(), args, stdin, env)
    };

    let set = on(
        &alice,
        &["vault", "set", "vault", "token", "--shared"],
        "t3am",
        &[],
    );
    assert!(set.status.success(), "{}", text(&set.stderr));
    let show = on(&alice, &["vault", "key", "show"], "", &[]);
    let key = text(&show.stdout).trim().to_owned();
    assert!(key.starts_with("AFK-"), "{key}");

    // Bob cloned the collection: he lacks the key.
    let send = on(&bob, &["send", "users/get.yml", "-e", "vault"], "", &[]);
    assert!(!send.status.success());
    assert!(
        text(&send.stderr).contains("collection's key"),
        "{}",
        text(&send.stderr)
    );
    let status = on(&bob, &["vault", "status"], "", &[]);
    assert!(
        text(&status.stdout).contains("locked"),
        "{}",
        text(&status.stdout)
    );

    // In CI the key comes from the environment, and is not stored.
    let ci = [("APIFAST_COLLECTION_KEY", key.as_str())];
    let send = on(&bob, &["send", "users/get.yml", "-e", "vault"], "", &ci);
    assert!(send.status.success(), "{}", text(&send.stderr));
    assert_eq!(authorization(&server), "authorization: Bearer t3am");
    assert!(!bob.path().join("keys").exists(), "never stored");
    let wrong = [("APIFAST_COLLECTION_KEY", "AFK-0000")];
    let send = on(&bob, &["send", "users/get.yml", "-e", "vault"], "", &wrong);
    assert!(!send.status.success());

    // Bob takes the key for good.
    let import = on(&bob, &["vault", "key", "import"], &format!("{key}\n"), &[]);
    assert!(import.status.success(), "{}", text(&import.stderr));
    let send = on(&bob, &["send", "users/get.yml", "-e", "vault"], "", &[]);
    assert!(send.status.success(), "{}", text(&send.stderr));

    // Alice rotates: Bob's key no longer opens the secret.
    let rotate = on(&alice, &["vault", "key", "rotate"], "", &[]);
    let new_key = text(&rotate.stdout).trim().to_owned();
    assert!(new_key.starts_with("AFK-") && new_key != key);
    let send = on(&bob, &["send", "users/get.yml", "-e", "vault"], "", &[]);
    assert!(!send.status.success());
}
