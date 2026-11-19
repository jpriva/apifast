//! `apifast import`, end to end: a Postman export becomes a collection
//! whose requests send.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

mod common;

use common::{Server, apifast, text};

const POSTMAN: &str = r#"{
  "info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
  "item": [
    {"name": "Users", "item": [
      {"name": "List users", "event": [{"listen": "test", "script": {"exec": ["pm.test('x')"]}}],
       "request": {"method": "GET", "header": [{"key": "X-Trace", "value": "1"}], "url": "{{host}}/users"}}
    ]}
  ]
}"#;

#[test]
fn a_postman_export_becomes_a_collection_that_sends() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shop.json"), POSTMAN).unwrap();
    let environment = format!(
        r#"{{"name": "local", "values": [{{"key": "host", "value": "http://127.0.0.1:{}", "enabled": true}}]}}"#,
        server.port
    );
    std::fs::write(dir.path().join("local.json"), environment).unwrap();

    let args = [
        "import",
        "postman",
        "shop.json",
        "--env",
        "local.json",
        "--out",
        "shop",
        "--dry-run",
    ];
    let out = apifast(dir.path(), data.path(), &args);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let printed = text(&out.stdout);
    assert!(printed.starts_with("Would create `Shop`"), "{printed}");
    assert!(printed.contains("users/list-users.yml"), "{printed}");
    assert!(
        printed.contains("Users / List users: a `test` script"),
        "{printed}"
    );
    assert!(
        !dir.path().join("shop").exists(),
        "a dry run writes nothing"
    );

    let out = apifast(dir.path(), data.path(), &args[..7]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let collection = dir.path().join("shop");
    assert!(collection.join("users/list-users.yml").exists());
    assert!(collection.join("environments/local.yml").exists());

    let send = apifast(
        &collection,
        data.path(),
        &["send", "users/list-users.yml", "-e", "local"],
    );
    assert!(send.status.success(), "{}", text(&send.stderr));
    let seen = server.seen();
    assert!(seen[0].starts_with("GET /users "), "{seen:?}");
    assert!(seen[0].contains("x-trace: 1"), "{seen:?}");

    // Never over a collection.
    let again = apifast(dir.path(), data.path(), &args[..7]);
    assert!(!again.status.success());
    assert!(text(&again.stderr).contains("already holds a collection"));
}

#[test]
fn an_openapi_spec_in_yaml_becomes_a_collection_that_sends() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let spec = format!(
        "openapi: 3.0.0\ninfo:\n  title: Pets\nservers:\n  - url: http://127.0.0.1:{}/v1\ntags:\n  - name: pets\npaths:\n  /pets/{{petId}}:\n    get:\n      tags: [pets]\n      summary: Get pet\n      parameters:\n        - name: petId\n          in: path\n          required: true\n          schema: {{type: integer, example: 7}}\n      responses:\n        '200':\n          description: ok\n",
        server.port
    );
    std::fs::write(dir.path().join("pets.yaml"), spec).unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["import", "openapi", "pets.yaml", "--out", "pets"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("pets/get-pet.yml"),
        "{}",
        text(&out.stdout)
    );

    let collection = dir.path().join("pets");
    let send = apifast(&collection, data.path(), &["send", "pets/get-pet.yml"]);
    assert!(send.status.success(), "{}", text(&send.stderr));
    assert!(
        server.seen()[0].starts_with("GET /v1/pets/7 "),
        "{:?}",
        server.seen()
    );

    let with_env = apifast(
        dir.path(),
        data.path(),
        &[
            "import",
            "openapi",
            "pets.yaml",
            "--env",
            "x.json",
            "--out",
            "other",
        ],
    );
    assert!(!with_env.status.success());
}

#[test]
fn a_bru_collection_becomes_a_collection_that_sends() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let old = dir.path().join("old");
    std::fs::create_dir_all(old.join("environments")).unwrap();
    std::fs::write(
        old.join("bruno.json"),
        r#"{"version": "1", "name": "Old", "type": "collection"}"#,
    )
    .unwrap();
    std::fs::write(old.join("collection.bru"), "headers {\n  x-from: bru\n}\n").unwrap();
    std::fs::write(
        old.join("ping.bru"),
        "meta {\n  name: Ping\n  seq: 1\n}\n\nget {\n  url: {{host}}/ping\n  body: none\n  auth: none\n}\n",
    )
    .unwrap();
    std::fs::write(
        old.join("environments/local.bru"),
        format!("vars {{\n  host: http://127.0.0.1:{}\n}}\n", server.port),
    )
    .unwrap();

    let out = apifast(
        dir.path(),
        data.path(),
        &["import", "bruno", "old", "--out", "new"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let collection = dir.path().join("new");
    let send = apifast(
        &collection,
        data.path(),
        &["send", "ping.yml", "-e", "local"],
    );
    assert!(send.status.success(), "{}", text(&send.stderr));
    let seen = server.seen();
    assert!(
        seen[0].starts_with("GET /ping ") && seen[0].contains("x-from: bru"),
        "{seen:?}"
    );
}
