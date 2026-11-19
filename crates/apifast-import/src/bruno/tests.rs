use std::path::Path;

use super::*;

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
}

fn collection() -> (tempfile::TempDir, ImportedCollection) {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "bruno.json",
                r#"{"version": "1", "name": "Shop", "type": "collection"}"#,
            ),
            (
                "collection.bru",
                "headers {\n  x-app: shop\n}\n\nauth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\nvars:pre-request {\n  @number\n  page: 1\n}\n\nscript:pre-request {\n  bru.setVar('a', 1);\n}\n",
            ),
            (
                "users/folder.bru",
                "meta {\n  name: Users\n  seq: 1\n}\n\nauth {\n  mode: basic\n}\n\nauth:basic {\n  username: ana\n  password: p\n}\n",
            ),
            (
                "users/create.bru",
                "meta {\n  name: Create user\n  type: http\n  seq: 2\n  tags: [\n    smoke\n  ]\n}\n\npost {\n  url: {{host}}/users/:org?notify=1\n  body: json\n  auth: inherit\n}\n\nparams:query {\n  notify: 1\n  ~dry: true\n}\n\nparams:path {\n  org: acme\n}\n\nbody:json {\n  {\n    \"name\": \"ana\"\n  }\n}\n\nvars:post-response {\n  user_id: res.body.id\n}\n\nassert {\n  res.status: eq 201\n  res.body.id: isNumber\n  ~res.body.x: isDefined\n}\n\nsettings {\n  encodeUrl: false\n  timeout: 0\n  followRedirects: false\n}\n\ntests {\n  test('ok', () => {});\n}\n\ndocs {\n  Creates a user.\n}\n",
            ),
            (
                "users/list.bru",
                "meta {\n  name: List users\n  seq: 1\n}\n\nget {\n  url: {{host}}/users\n  body: none\n  auth: none\n}\n",
            ),
            (
                "upload.bru",
                "meta {\n  name: Upload\n}\n\npost {\n  url: {{host}}/up\n  body: multipartForm\n  auth: awsv4\n}\n\nbody:multipart-form {\n  doc: @file(a.pdf|b.pdf) @contentType(application/pdf)\n  note: hi\n}\n\nauth:awsv4 {\n  accessKeyId: k\n  secretAccessKey: s\n  service: s3\n  region: eu-west-1\n}\n",
            ),
            ("broken.bru", "meta {\n  name: broken\n"),
            (
                "socket.bru",
                "meta {\n  name: Socket\n  type: ws\n}\n\nws {\n  url: ws://x\n}\n",
            ),
            (
                "environments/local.bru",
                "vars {\n  host: http://localhost:3000\n  ~old: x\n}\nvars:secret [\n  token\n]\n",
            ),
            ("node_modules/x.bru", "meta {\n  name: ignored\n}\n"),
        ],
    );
    let imported = import_bruno(dir.path()).unwrap();
    (dir, imported)
}

fn warned(collection: &ImportedCollection, at: &str, part: &str) -> bool {
    (collection.warnings.iter()).any(|w| w.at == at && w.message.contains(part))
}

#[test]
fn the_collection_and_its_folders_give_their_defaults() {
    let (_dir, collection) = collection();
    assert_eq!(collection.name, "Shop");
    assert_eq!(collection.defaults.headers[0], Header::new("x-app", "shop"));
    assert!(matches!(
        collection.defaults.auth,
        Setting::Set(AuthScheme::Bearer(_))
    ));
    assert_eq!(
        collection.defaults.variables[0]
            .value
            .as_ref()
            .unwrap()
            .kind,
        ValueKind::Number
    );
    assert!(warned(&collection, "", "pre-request script"));
    let ImportedItem::Folder { folder, items } = &collection.items[0] else {
        panic!("{:?}", collection.items[0])
    };
    assert_eq!(folder.name, "Users");
    assert!(matches!(
        folder.defaults.auth,
        Setting::Set(AuthScheme::Basic(_))
    ));
    let names: Vec<&str> = items
        .iter()
        .map(|i| match i {
            ImportedItem::Request { request: r, .. } => r.name.as_str(),
            ImportedItem::Folder { folder, .. } => folder.name.as_str(),
        })
        .collect();
    assert_eq!(names, ["List users", "Create user"], "by seq");
}

#[test]
fn a_request_keeps_its_url_params_body_checks_and_settings() {
    let (_dir, collection) = collection();
    let ImportedItem::Folder { items, .. } = &collection.items[0] else {
        unreachable!()
    };
    let ImportedItem::Request {
        request: create, ..
    } = &items[1]
    else {
        unreachable!()
    };
    assert_eq!(create.method, Method::Post);
    assert_eq!(create.url, "{{host}}/users/:org?notify=1");
    assert_eq!(create.auth, Setting::Inherit);
    assert_eq!(create.tags, ["smoke"]);
    assert_eq!(create.body, Body::Json("{\n  \"name\": \"ana\"\n}".into()));
    let org = create
        .params
        .iter()
        .find(|p| p.kind == ParamKind::Path)
        .unwrap();
    assert_eq!(org.value, "acme");
    assert!(create.params.iter().any(|p| p.name == "dry" && !p.enabled));
    assert_eq!(create.captures[0].expression, "res.body.id");
    assert_eq!(create.captures[0].variable, "user_id");
    assert_eq!(create.assertions[0].value.as_deref(), Some("201"));
    assert_eq!(create.assertions[1].value, None);
    assert!(!create.assertions[2].enabled);
    assert_eq!(create.settings.encode_url, Setting::Set(false));
    assert_eq!(create.settings.follow_redirects, Setting::Set(false));
    assert_eq!(create.settings.timeout_ms, Setting::Inherit, "0 is not set");
    assert_eq!(create.docs.as_deref(), Some("Creates a user."));
    assert!(warned(&collection, "Users / Create user", "tests block"));
    let ImportedItem::Request { request: list, .. } = &items[0] else {
        unreachable!()
    };
    assert_eq!(list.auth, Setting::Set(AuthScheme::None));
}

#[test]
fn multipart_aws_environments_and_what_is_left_out() {
    let (_dir, collection) = collection();
    let upload = collection
        .items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Request { request: r, .. } if r.name == "Upload" => Some(r),
            _ => None,
        })
        .unwrap();
    let Body::Multipart(parts) = &upload.body else {
        panic!("{:?}", upload.body)
    };
    assert_eq!(
        parts[0].value,
        MultipartValue::Files(vec!["a.pdf".into(), "b.pdf".into()])
    );
    assert_eq!(parts[0].content_type.as_deref(), Some("application/pdf"));
    assert_eq!(parts[1].value, MultipartValue::Text("hi".into()));
    assert!(
        matches!(&upload.auth, Setting::Set(AuthScheme::AwsV4(aws)) if aws.region == "eu-west-1")
    );

    let local = &collection.environments[0];
    assert_eq!(local.name, "local");
    assert!(!local.variables[1].enabled);
    assert!(local.variables[2].secret && local.variables[2].value.is_none());
    assert!(warned(&collection, "broken", "cannot be read"));
    assert!(warned(
        &collection,
        "Socket",
        "`ws` requests are not supported"
    ));
    let all = apifast_app::plan_import(&collection);
    assert!(!all.entries.iter().any(|e| e.name == "ignored"));
}

#[test]
fn a_folder_without_bruno_json_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        import_bruno(dir.path()),
        Err(BrunoError::NotCollection(_))
    ));
}

#[test]
fn a_graphql_request_keeps_its_query_and_variables() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "bruno.json",
                r#"{"version": "1", "name": "G", "type": "collection"}"#,
            ),
            (
                "me.bru",
                "meta {\n  name: Me\n  type: graphql\n}\n\npost {\n  url: https://h/graphql\n  body: graphql\n  auth: none\n}\n\nbody:graphql {\n  { me { id } }\n}\n\nbody:graphql:vars {\n  {\"a\": 1}\n}\n",
            ),
        ],
    );
    let collection = import_bruno(dir.path()).unwrap();
    let ImportedItem::Request { request, .. } = &collection.items[0] else {
        panic!("{:?}", collection.items);
    };
    assert_eq!(
        request.body,
        Body::GraphQl {
            query: "{ me { id } }".into(),
            variables: "{\"a\": 1}".into()
        }
    );
    assert_eq!(collection.warnings, Vec::new());
}
