use serde_json::json;

use super::*;

fn v4() -> Value {
    json!({
        "_type": "export", "__export_format": 4,
        "resources": [
            {"_id": "wrk_1", "_type": "workspace", "name": "Shop"},
            {"_id": "fld_1", "_type": "request_group", "parentId": "wrk_1", "name": "Users", "metaSortKey": -2,
             "environment": {"page": 1},
             "authentication": {"type": "basic", "username": "{{ _.user }}", "password": "p"}},
            {"_id": "req_2", "_type": "request", "parentId": "fld_1", "name": "Create", "metaSortKey": 2,
             "method": "POST", "url": "{{ _.base_url }}/users/:org",
             "parameters": [{"name": "notify", "value": "1"}, {"name": "dry", "value": "true", "disabled": true}],
             "pathParameters": [{"name": "org", "value": "acme"}],
             "headers": [{"name": "X-Trace", "value": "{{_.trace}}"}, {"name": "X-Off", "value": "1", "disabled": true}],
             "body": {"mimeType": "application/json", "text": "{\"name\": \"{{ _.name }}\"}"},
             "authentication": {}, "settingEncodeUrl": false, "settingFollowRedirects": "off",
             "preRequestScript": "insomnia.environment.set('a', 1);",
             "description": "Creates a user."},
            {"_id": "req_1", "_type": "request", "parentId": "fld_1", "name": "List", "metaSortKey": 1,
             "method": "GET", "url": "{{ _.base_url }}/users",
             "authentication": {"type": "bearer", "token": "{{ _.token }}", "disabled": true}},
            {"_id": "req_3", "_type": "request", "parentId": "wrk_1", "name": "Upload", "metaSortKey": -1,
             "method": "POST", "url": "{{ _.base_url }}/up",
             "body": {"mimeType": "multipart/form-data", "params": [{"name": "doc", "type": "file", "fileName": "/tmp/a.pdf"}, {"name": "note", "value": "hi"}]},
             "authentication": {"type": "hawk", "id": "x"}},
            {"_id": "req_4", "_type": "request", "parentId": "wrk_1", "name": "Login", "metaSortKey": 0,
             "method": "POST", "url": "{% response 'body', 'req_1', '$.url' %}/login",
             "body": {"mimeType": "application/x-www-form-urlencoded", "params": [{"name": "user", "value": "ana"}]},
             "authentication": {"type": "oauth2", "grantType": "authorization_code", "accessTokenUrl": "https://a/token", "authorizationUrl": "https://a/auth", "usePkce": true}},
            {"_id": "env_base", "_type": "environment", "parentId": "wrk_1", "name": "Base Environment",
             "data": {"base_url": "https://api.test", "user": {"name": "admin", "id": 1}, "debug": false}},
            {"_id": "env_dev", "_type": "environment", "parentId": "env_base", "name": "Dev", "data": {"base_url": "http://localhost"}},
            {"_id": "ws_1", "_type": "websocket_request", "parentId": "wrk_1", "name": "Socket"}
        ]
    })
}

fn find<'a>(items: &'a [ImportedItem], name: &str) -> &'a HttpRequest {
    fn walk<'a>(items: &'a [ImportedItem], name: &str) -> Option<&'a HttpRequest> {
        items.iter().find_map(|i| match i {
            ImportedItem::Request { request: r, .. } if r.name == name => Some(r.as_ref()),
            ImportedItem::Folder { items, .. } => walk(items, name),
            ImportedItem::Request { .. } => None,
        })
    }
    walk(items, name).unwrap_or_else(|| panic!("no `{name}`"))
}

fn warned(collection: &ImportedCollection, at: &str, part: &str) -> bool {
    (collection.warnings.iter()).any(|w| w.at == at && w.message.contains(part))
}

#[test]
fn a_v4_export_gives_its_tree_in_order() {
    let collection = import_insomnia(&v4()).unwrap();
    assert_eq!(collection.name, "Shop");
    let shape: Vec<String> = (collection.items.iter())
        .map(|i| match i {
            ImportedItem::Folder { folder, items } => format!("{}({})", folder.name, items.len()),
            ImportedItem::Request { request: r, .. } => r.name.clone(),
        })
        .collect();
    assert_eq!(shape, ["Users(2)", "Upload", "Login"]);
    let ImportedItem::Folder { folder, items } = &collection.items[0] else {
        unreachable!()
    };
    let ImportedItem::Request { request: first, .. } = &items[0] else {
        unreachable!()
    };
    assert_eq!(first.name, "List", "by metaSortKey");
    assert!(
        matches!(folder.defaults.auth, Setting::Set(AuthScheme::Basic(ref b)) if b.username == "{{user}}")
    );
    assert_eq!(
        folder.defaults.variables[0].value.as_ref().unwrap().kind,
        ValueKind::Number
    );
    assert!(warned(&collection, "Socket", "`websocket_request`"));
}

#[test]
fn requests_keep_url_params_headers_body_auth_and_settings() {
    let collection = import_insomnia(&v4()).unwrap();
    let create = find(&collection.items, "Create");
    assert_eq!(create.url, "{{base_url}}/users/:org?notify=1");
    let org = create
        .params
        .iter()
        .find(|p| p.kind == ParamKind::Path)
        .unwrap();
    assert_eq!(org.value, "acme");
    assert!(create.params.iter().any(|p| p.name == "dry" && !p.enabled));
    assert_eq!(create.headers[0].value, "{{trace}}");
    assert!(!create.headers[1].enabled);
    assert_eq!(create.body, Body::Json("{\"name\": \"{{name}}\"}".into()));
    assert_eq!(create.auth, Setting::Inherit, "an empty one inherits");
    assert_eq!(create.settings.encode_url, Setting::Set(false));
    assert_eq!(create.settings.follow_redirects, Setting::Set(false));
    assert_eq!(create.docs.as_deref(), Some("Creates a user."));
    assert!(warned(&collection, "Users / Create", "scripts"));

    assert_eq!(
        find(&collection.items, "List").auth,
        Setting::Set(AuthScheme::None),
        "disabled"
    );
    let upload = find(&collection.items, "Upload");
    let Body::Multipart(parts) = &upload.body else {
        panic!("{:?}", upload.body)
    };
    assert_eq!(
        parts[0].value,
        MultipartValue::Files(vec!["/tmp/a.pdf".into()])
    );
    assert!(warned(&collection, "Upload", "`hawk` auth"));
    let login = find(&collection.items, "Login");
    assert!(
        matches!(&login.auth, Setting::Set(AuthScheme::OAuth2(o)) if o.pkce && o.flow == OAuth2Flow::AuthorizationCode)
    );
    assert!(warned(&collection, "", "template tags"));
}

#[test]
fn environments_are_the_base_and_each_sub_over_it() {
    let collection = import_insomnia(&v4()).unwrap();
    let names: Vec<&str> = collection
        .environments
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["Base Environment", "Dev"]);
    let dev = &collection.environments[1];
    let value = |name: &str| {
        dev.variables
            .iter()
            .find(|v| v.name == name)
            .unwrap()
            .value
            .clone()
            .unwrap()
    };
    assert_eq!(value("base_url").data, "http://localhost");
    assert_eq!(value("user").kind, ValueKind::Object);
    assert_eq!(value("user").data, r#"{"name":"admin","id":1}"#);
    assert_eq!(value("debug").kind, ValueKind::Bool);
}

#[test]
fn a_v5_export_is_a_tree() {
    let export = json!({
        "type": "collection.insomnia.rest/5.0",
        "name": "Shop v5",
        "collection": [
            {"name": "Users", "children": [
                {"name": "Get", "method": "GET", "url": "{{ _.base_url }}/users", "settings": {"encodeUrl": true, "followRedirects": "global"}}
            ]},
            {"name": "Health", "method": "GET", "url": "https://h/health", "scripts": {"afterResponse": "x"}}
        ],
        "environments": {"name": "Base", "data": {"base_url": "https://api"}, "subEnvironments": [{"name": "Local", "data": {"base_url": "http://l"}}]}
    });
    let collection = import_insomnia(&export).unwrap();
    assert_eq!(collection.name, "Shop v5");
    let get = find(&collection.items, "Get");
    assert_eq!(get.url, "{{base_url}}/users");
    assert_eq!(get.settings.follow_redirects, Setting::Inherit);
    assert!(warned(&collection, "Health", "scripts"));
    assert_eq!(collection.environments.len(), 2);
}

#[test]
fn other_documents_are_refused() {
    assert!(!is_insomnia(&json!({"openapi": "3.0.0"})));
    assert_eq!(
        import_insomnia(&json!({"info": {}})),
        Err(InsomniaError::NotInsomnia)
    );
    let without = json!({"type": "collection.insomnia.rest/5.0", "name": "x"});
    assert!(is_insomnia(&without));
    assert_eq!(import_insomnia(&without), Err(InsomniaError::NotInsomnia));
    assert_eq!(
        variables_of("a {{ _.b }} {{c}} {{ d.e }} {{"),
        "a {{b}} {{c}} {{d.e}} {{"
    );
}
