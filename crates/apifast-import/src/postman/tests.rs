use apifast_domain::{ApiKeyAuth, ApiKeyPlacement, BasicAuth, OAuth2Flow, TokenPlacement};

use super::*;

const COLLECTION: &str = r#"{
  "info": {
    "_postman_id": "1",
    "name": "Shop API",
    "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
  },
  "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}", "type": "string"}]},
  "variable": [{"key": "host", "value": "https://api.test"}, {"key": "old", "value": 1, "disabled": true}],
  "event": [{"listen": "prerequest", "script": {"exec": ["console.log('hi')"]}}],
  "item": [
    {
      "name": "Users",
      "auth": {"type": "basic", "basic": {"username": "ana", "password": "p"}},
      "item": [
        {
          "name": "Get user",
          "request": {
            "method": "GET",
            "header": [{"key": "Accept", "value": "application/json"}, {"key": "X-Off", "value": "1", "disabled": true}],
            "url": {
              "raw": "{{host}}/users/:id?expand=1",
              "host": ["{{host}}"], "path": ["users", ":id"],
              "query": [{"key": "expand", "value": "1"}, {"key": "debug", "value": "true", "disabled": true}],
              "variable": [{"key": "id", "value": "42"}]
            },
            "description": "Gets a user."
          },
          "response": [{
            "name": "Found",
            "originalRequest": {"method": "GET", "header": [], "url": {"raw": "{{host}}/users/42"}},
            "status": "OK", "code": 200, "_postman_previewlanguage": "json",
            "header": [{"key": "Content-Type", "value": "application/json"}],
            "body": "{\"id\": 42}"
          }]
        },
        {
          "name": "Create user",
          "event": [{"listen": "test", "script": {"exec": ["pm.test('ok')"]}}],
          "request": {
            "method": "POST",
            "header": [],
            "body": {"mode": "raw", "raw": "{\"id\": \"{{$guid}}\", \"at\": \"{{$isoTimestamp}}\", \"x\": \"{{$randomCompanyName}}\"}", "options": {"raw": {"language": "json"}}},
            "url": "{{host}}/users"
          },
          "protocolProfileBehavior": {"followRedirects": false, "disableUrlEncoding": true, "strictSSL": false}
        }
      ]
    },
    {
      "name": "Login",
      "request": {
        "auth": {"type": "noauth"},
        "method": "POST",
        "body": {"mode": "urlencoded", "urlencoded": [{"key": "user", "value": "ana"}, {"key": "otp", "value": "1", "disabled": true}]},
        "url": {"protocol": "https", "host": ["auth", "test"], "path": ["login"], "query": [{"key": "v", "value": "2"}]}
      }
    },
    {
      "name": "Upload",
      "request": {
        "auth": {"type": "apikey", "apikey": [{"key": "key", "value": "k"}, {"key": "value", "value": "{{key}}"}, {"key": "in", "value": "query"}]},
        "method": "POST",
        "body": {"mode": "formdata", "formdata": [{"key": "doc", "type": "file", "src": "/tmp/a.pdf"}, {"key": "note", "type": "text", "value": "hi"}]},
        "url": "{{host}}/upload"
      }
    },
    {
      "name": "Signed",
      "request": {
        "auth": {"type": "hawk", "hawk": []},
        "method": "GET",
        "url": "{{host}}/signed"
      }
    },
    {
      "name": "Token",
      "request": {
        "auth": {"type": "oauth2", "oauth2": [
          {"key": "grant_type", "value": "authorization_code_with_pkce"},
          {"key": "authUrl", "value": "https://auth/authorize"},
          {"key": "accessTokenUrl", "value": "https://auth/token"},
          {"key": "clientId", "value": "cli"},
          {"key": "addTokenTo", "value": "header"},
          {"key": "client_authentication", "value": "body"}
        ]},
        "method": "GET",
        "url": "{{host}}/me"
      }
    },
    {
      "name": "Query",
      "request": {
        "method": "POST",
        "body": {"mode": "graphql", "graphql": {"query": "{ me { id } }", "variables": "{\"a\": 1}"}},
        "url": "{{host}}/graphql"
      }
    }
  ]
}"#;

fn imported() -> ImportedCollection {
    import_postman(COLLECTION).unwrap()
}

fn find<'a>(items: &'a [ImportedItem], name: &str) -> &'a HttpRequest {
    for item in items {
        match item {
            ImportedItem::Request(request) if request.name == name => return request.as_ref(),
            ImportedItem::Folder { items, .. } => {
                if let Some(found) = try_find(items, name) {
                    return found;
                }
            }
            ImportedItem::Request(_) => {}
        }
    }
    panic!("no request `{name}`")
}

fn try_find<'a>(items: &'a [ImportedItem], name: &str) -> Option<&'a HttpRequest> {
    items.iter().find_map(|item| match item {
        ImportedItem::Request(request) if request.name == name => Some(request.as_ref()),
        ImportedItem::Folder { items, .. } => try_find(items, name),
        ImportedItem::Request(_) => None,
    })
}

fn warned(collection: &ImportedCollection, at: &str, part: &str) -> bool {
    (collection.warnings.iter()).any(|w| w.at == at && w.message.contains(part))
}

#[test]
fn the_collection_gives_its_name_auth_and_variables() {
    let collection = imported();
    assert_eq!(collection.name, "Shop API");
    assert!(matches!(
        collection.defaults.auth,
        Setting::Set(AuthScheme::Bearer(_))
    ));
    let variables = &collection.defaults.variables;
    assert_eq!(variables[0].name, "host");
    assert!(!variables[1].enabled);
    assert!(warned(&collection, "", "prerequest"));
}

#[test]
fn folders_keep_their_auth_and_requests_their_url_params_and_headers() {
    let collection = imported();
    let ImportedItem::Folder { folder, items } = &collection.items[0] else {
        panic!("Users is a folder")
    };
    assert_eq!(folder.name, "Users");
    assert_eq!(
        folder.defaults.auth,
        Setting::Set(AuthScheme::Basic(BasicAuth {
            username: "ana".into(),
            password: "p".into(),
        }))
    );
    let get = find(items, "Get user");
    assert_eq!(get.url, "{{host}}/users/:id?expand=1");
    assert_eq!(get.auth, Setting::Inherit);
    assert_eq!(get.docs.as_deref(), Some("Gets a user."));
    assert!(!get.headers[1].enabled);
    let id = get
        .params
        .iter()
        .find(|p| p.kind == ParamKind::Path)
        .unwrap();
    assert_eq!((id.name.as_str(), id.value.as_str()), ("id", "42"));
    let debug = get.params.iter().find(|p| p.name == "debug").unwrap();
    assert!(!debug.enabled);
    assert!(get.params.iter().any(|p| p.name == "expand" && p.enabled));
    let example = &get.examples[0];
    assert_eq!(example.name, "Found");
    let response = example.response.as_ref().unwrap();
    assert_eq!(response.status, Some(200));
    assert_eq!(response.body.as_ref().unwrap().kind, "json");
}

#[test]
fn bodies_settings_scripts_and_dynamic_variables() {
    let collection = imported();
    let create = find(&collection.items, "Create user");
    let Body::Json(json) = &create.body else {
        panic!("{:?}", create.body)
    };
    assert!(
        json.contains("{{$randomUUID}}") && !json.contains("$guid"),
        "{json}"
    );
    assert_eq!(create.settings.follow_redirects, Setting::Set(false));
    assert_eq!(create.settings.encode_url, Setting::Set(false));
    assert!(warned(&collection, "Users / Create user", "`test` script"));
    assert!(warned(&collection, "Users / Create user", "strictSSL"));
    assert!(warned(&collection, "", "randomCompanyName"));
    assert!(!warned(&collection, "", "isoTimestamp"));

    let login = find(&collection.items, "Login");
    assert_eq!(login.url, "https://auth.test/login?v=2");
    assert_eq!(login.auth, Setting::Set(AuthScheme::None));
    let Body::FormUrlEncoded(fields) = &login.body else {
        panic!("{:?}", login.body)
    };
    assert!(fields[0].enabled && !fields[1].enabled);

    let upload = find(&collection.items, "Upload");
    let Body::Multipart(parts) = &upload.body else {
        panic!("{:?}", upload.body)
    };
    assert_eq!(
        parts[0].value,
        MultipartValue::Files(vec!["/tmp/a.pdf".into()])
    );
    assert_eq!(parts[1].value, MultipartValue::Text("hi".into()));
    assert_eq!(
        upload.auth,
        Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
            key: "k".into(),
            value: "{{key}}".into(),
            placement: ApiKeyPlacement::Query,
        }))
    );

    let query = find(&collection.items, "Query");
    let Body::Json(graphql) = &query.body else {
        panic!("{:?}", query.body)
    };
    assert!(
        graphql.contains("\"query\": \"{ me { id } }\""),
        "{graphql}"
    );
    assert!(warned(&collection, "Query", "GraphQL"));
}

#[test]
fn auth_types_map_or_say_why_not() {
    let collection = imported();
    let signed = find(&collection.items, "Signed");
    assert_eq!(signed.auth, Setting::Set(AuthScheme::None));
    assert!(warned(
        &collection,
        "Signed",
        "`hawk` auth is not supported"
    ));

    let token = find(&collection.items, "Token");
    let Setting::Set(AuthScheme::OAuth2(oauth)) = &token.auth else {
        panic!("{:?}", token.auth)
    };
    assert_eq!(oauth.flow, OAuth2Flow::AuthorizationCode);
    assert!(oauth.pkce);
    assert_eq!(oauth.authorization_url, "https://auth/authorize");
    assert_eq!(
        oauth.token_placement,
        TokenPlacement::Header("Bearer".into())
    );
    assert_eq!(
        oauth.credentials_placement,
        apifast_domain::CredentialsPlacement::Body
    );
}

#[test]
fn environments_keep_their_values_but_not_secret_ones() {
    let (environment, warnings) = import_postman_environment(
        r#"{"name": "prod", "values": [
            {"key": "host", "value": "https://api.test", "enabled": true},
            {"key": "token", "value": "s3cr3t", "type": "secret", "enabled": true},
            {"key": "off", "value": "x", "enabled": false}
        ]}"#,
    )
    .unwrap();
    assert_eq!(environment.name, "prod");
    assert_eq!(environment.variables.len(), 3);
    assert!(environment.variables[1].secret && environment.variables[1].value.is_none());
    assert!(!environment.variables[2].enabled);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn other_files_are_refused() {
    assert!(matches!(
        import_postman("nope"),
        Err(PostmanError::NotJson(_))
    ));
    assert_eq!(
        import_postman(r#"{"a": 1}"#),
        Err(PostmanError::NotCollection)
    );
    assert_eq!(
        import_postman_environment(r#"{"info": {}}"#),
        Err(PostmanError::NotEnvironment)
    );
    // The API's wrapper is read too.
    let wrapped = format!(r#"{{"collection": {COLLECTION}}}"#);
    assert_eq!(import_postman(&wrapped).unwrap().name, "Shop API");
}
