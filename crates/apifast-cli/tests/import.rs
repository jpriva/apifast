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

    // Again, the same export: nothing changes.
    let again = apifast(dir.path(), data.path(), &args[..7]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(
        text(&again.stdout).contains("0 new, 0 changed, 1 unchanged"),
        "{}",
        text(&again.stdout)
    );
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

#[test]
fn an_insomnia_export_becomes_a_collection_that_sends() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let export = format!(
        r#"{{"_type": "export", "__export_format": 4, "resources": [
            {{"_id": "wrk_1", "_type": "workspace", "name": "Ins"}},
            {{"_id": "req_1", "_type": "request", "parentId": "wrk_1", "name": "Users", "method": "GET",
              "url": "{{{{ _.base }}}}/users", "parameters": [{{"name": "page", "value": "2"}}]}},
            {{"_id": "env_1", "_type": "environment", "parentId": "wrk_1", "name": "Base", "data": {{"base": "http://127.0.0.1:{}"}}}}
        ]}}"#,
        server.port
    );
    std::fs::write(dir.path().join("ins.json"), export).unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["import", "insomnia", "ins.json", "--out", "ins"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let collection = dir.path().join("ins");
    let send = apifast(
        &collection,
        data.path(),
        &["send", "users.yml", "-e", "Base"],
    );
    assert!(send.status.success(), "{}", text(&send.stderr));
    assert!(
        server.seen()[0].starts_with("GET /users?page=2 "),
        "{:?}",
        server.seen()
    );
}

#[test]
fn importing_again_updates_and_keeps_edits() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let spec = |version: &str| {
        format!(
            "openapi: 3.0.0\ninfo: {{title: Pets, version: '1'}}\npaths:\n  /pets:\n    get: {{operationId: listPets, summary: List pets, parameters: [{{name: v, in: query, schema: {{type: string, example: '{version}'}}}}]}}\n  /pets/{{id}}:\n    get: {{operationId: getPet, summary: Get pet, parameters: [{{name: v, in: query, schema: {{type: string, example: '{version}'}}}}]}}\n"
        )
    };
    std::fs::write(dir.path().join("pets.yaml"), spec("1")).unwrap();
    let import = |extra: &[&str]| {
        let mut args = vec!["import", "openapi", "pets.yaml", "--out", "pets"];
        args.extend_from_slice(extra);
        apifast(dir.path(), data.path(), &args)
    };
    let out = import(&[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(dir.path().join("pets/.apifast/import.yml").exists());
    let collection = std::fs::read_to_string(dir.path().join("pets/opencollection.yml")).unwrap();
    assert!(
        collection.contains(".apifast"),
        "hidden from Bruno: {collection}"
    );

    // The user edits one request; the spec changes both.
    let edited = dir.path().join("pets/get-pet.yml");
    let mine = std::fs::read_to_string(&edited)
        .unwrap()
        .replace("Get pet", "My pet");
    std::fs::write(&edited, &mine).unwrap();
    std::fs::write(dir.path().join("pets.yaml"), spec("2")).unwrap();

    let dry = import(&["--dry-run"]);
    let plan = text(&dry.stdout);
    assert!(
        plan.contains("list-pets.yml: changed in the source\n"),
        "{plan}"
    );
    assert!(
        plan.contains("get-pet.yml: changed in the source and here [skip]"),
        "{plan}"
    );
    assert!(plan.contains("--overwrite"), "{plan}");

    let out = import(&[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let list = std::fs::read_to_string(dir.path().join("pets/list-pets.yml")).unwrap();
    assert!(list.contains("value: \"2\""), "{list}");
    assert_eq!(
        std::fs::read_to_string(&edited).unwrap(),
        mine,
        "the edit is kept"
    );

    let out = import(&["--overwrite"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let get = std::fs::read_to_string(&edited).unwrap();
    assert!(
        get.contains("value: \"2\"") && !get.contains("My pet"),
        "{get}"
    );

    // A collection made by hand is not imported into.
    let other = dir.path().join("pets/.apifast/import.yml");
    std::fs::remove_file(other).unwrap();
    let out = import(&[]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("not imported"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_collection_exports_to_postman_and_imports_back() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shop.json"), POSTMAN).unwrap();
    let out = apifast(
        dir.path(),
        data.path(),
        &["import", "postman", "shop.json", "--out", "shop"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let environment = format!(
        "name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:{}\n",
        server.port
    );
    std::fs::create_dir_all(dir.path().join("shop/environments")).unwrap();
    std::fs::write(dir.path().join("shop/environments/local.yml"), environment).unwrap();

    let args = [
        "export",
        "postman",
        "--collection",
        "shop",
        "--out",
        "back.json",
        "--env",
        "local",
    ];
    let out = apifast(dir.path(), data.path(), &args);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let printed = text(&out.stdout);
    assert!(
        printed.contains("Exported `Shop` to back.json"),
        "{printed}"
    );
    assert!(
        dir.path().join("local.postman_environment.json").exists(),
        "{printed}"
    );

    // What Postman would read, imported again, still sends.
    let args = [
        "import",
        "postman",
        "back.json",
        "--env",
        "local.postman_environment.json",
        "--out",
        "again",
    ];
    let out = apifast(dir.path(), data.path(), &args);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let send = apifast(
        &dir.path().join("again"),
        data.path(),
        &["send", "users/list-users.yml", "-e", "local"],
    );
    assert!(send.status.success(), "{}", text(&send.stderr));
    assert!(
        server.seen()[0].starts_with("GET /users "),
        "{:?}",
        server.seen()
    );

    let missing = apifast(
        dir.path(),
        data.path(),
        &[
            "export",
            "postman",
            "--collection",
            "shop",
            "--out",
            "x.json",
            "--env",
            "prod",
        ],
    );
    assert!(!missing.status.success());
    assert!(
        text(&missing.stderr).contains("no environment `prod`"),
        "{}",
        text(&missing.stderr)
    );
}
