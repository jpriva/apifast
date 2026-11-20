use apifast_domain::{
    ApiKeyAuth, Assertion, BasicAuth, Folder, FormField, Method, MultipartPart, OAuth2Parameter,
    ValueKind, VariableValue,
};

use super::*;
use crate::import_postman;

fn request(name: &str, url: &str) -> HttpRequest {
    HttpRequest::new(name, Method::Get, url)
}

fn item(request: HttpRequest) -> ImportedItem {
    ImportedItem::Request {
        request: Box::new(request),
        key: None,
    }
}

/// A folder with OAuth 2.0, a variable and a header.
fn users_folder() -> Folder {
    let mut oauth = OAuth2Auth {
        flow: OAuth2Flow::AuthorizationCode,
        pkce: true,
        authorization_url: "https://a/auth".into(),
        access_token_url: "https://a/token".into(),
        client_id: "app".into(),
        scope: "read".into(),
        token_placement: TokenPlacement::Header("Token".into()),
        credentials_placement: CredentialsPlacement::BasicAuthHeader,
        ..OAuth2Auth::default()
    };
    oauth.parameters.push(OAuth2Parameter {
        request: OAuth2Request::Token,
        name: "audience".into(),
        value: "api".into(),
        placement: ParameterPlacement::Body,
    });
    let mut folder = Folder::new("Users");
    folder.defaults.auth = Setting::Set(AuthScheme::OAuth2(Box::new(oauth)));
    folder.defaults.variables = vec![Variable::text("page", "1")];
    folder.defaults.headers = vec![Header::new("X-Folder", "f")];
    folder
}

fn sample() -> ImportedCollection {
    let mut create = HttpRequest::new("Create", Method::Post, "{{baseUrl}}/users/:org?notify=1");
    create.params = vec![
        Param {
            name: "org".into(),
            value: "acme".into(),
            kind: ParamKind::Path,
            enabled: true,
        },
        Param {
            name: "dry".into(),
            value: "true".into(),
            kind: ParamKind::Query,
            enabled: false,
        },
    ];
    create.headers = vec![
        Header::new("X-Trace", "{{trace}}"),
        Header {
            enabled: false,
            ..Header::new("X-Off", "1")
        },
    ];
    create.body = Body::Json("{\"name\": \"{{name}}\"}".into());
    create.auth = Setting::Set(AuthScheme::Basic(BasicAuth {
        username: "{{user}}".into(),
        password: "p".into(),
    }));
    create.settings.follow_redirects = Setting::Set(false);
    create.settings.encode_url = Setting::Set(false);
    create.docs = Some("Creates a user.".into());
    create.assertions = vec![Assertion::new(
        "res.status",
        apifast_domain::Operator::Eq,
        Some("201"),
    )];
    apifast_domain::sync_params_from_url(&mut create);

    let mut form = request("Login", "https://auth.test/login");
    form.method = Method::Post;
    form.body = Body::FormUrlEncoded(vec![FormField {
        name: "user".into(),
        value: "ana".into(),
        enabled: true,
    }]);
    form.auth = Setting::Set(AuthScheme::None);

    let mut upload = request("Upload", "https://h:8080/up");
    upload.method = Method::Post;
    upload.body = Body::Multipart(vec![
        MultipartPart {
            name: "doc".into(),
            value: MultipartValue::Files(vec!["files/a.pdf".into()]),
            content_type: None,
            enabled: true,
        },
        MultipartPart {
            name: "note".into(),
            value: MultipartValue::Text("hi".into()),
            content_type: None,
            enabled: false,
        },
    ]);
    upload.auth = Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
        key: "X-Key".into(),
        value: "{{key}}".into(),
        placement: ApiKeyPlacement::Query,
    }));

    let folder = users_folder();

    ImportedCollection {
        name: "Shop".into(),
        defaults: RequestDefaults {
            variables: vec![
                Variable::text("baseUrl", "https://api.test"),
                Variable {
                    value: Some(VariableValue {
                        kind: ValueKind::Number,
                        data: "3".into(),
                    }),
                    ..Variable::text("retries", "")
                },
            ],
            headers: vec![Header::new("X-App", "apifast")],
            ..RequestDefaults::default()
        },
        items: vec![
            ImportedItem::Folder {
                folder: Box::new(folder),
                items: vec![item(create), item(form)],
            },
            item(upload),
        ],
        ..ImportedCollection::default()
    }
}

fn requests(items: &[ImportedItem]) -> Vec<HttpRequest> {
    items
        .iter()
        .flat_map(|item| match item {
            ImportedItem::Folder { items, .. } => requests(items),
            ImportedItem::Request { request, .. } => vec![request.as_ref().clone()],
        })
        .collect()
}

fn round_trip(collection: &ImportedCollection) -> (ImportedCollection, Vec<ImportWarning>) {
    let (export, warnings) = export_postman(collection);
    let text = serde_json::to_string_pretty(&export).unwrap();
    (import_postman(&text).unwrap(), warnings)
}

fn warned(warnings: &[ImportWarning], at: &str, part: &str) -> bool {
    warnings
        .iter()
        .any(|w| w.at == at && w.message.contains(part))
}

#[test]
fn an_export_imports_back_as_the_same_requests() {
    let source = sample();
    let (back, _) = round_trip(&source);
    assert_eq!(back.name, "Shop");
    let (sent, got) = (requests(&source.items), requests(&back.items));
    assert_eq!(got.len(), 3);
    for (sent, got) in sent.iter().zip(&got) {
        let expected = HttpRequest {
            // Inherited headers are copied in; assertions are not exported.
            headers: got.headers.clone(),
            assertions: Vec::new(),
            ..sent.clone()
        };
        assert_eq!(*got, expected, "{}", sent.name);
    }
    let create = &got[0];
    let names: Vec<&str> = create.headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["X-App", "X-Folder", "X-Trace", "X-Off"]);
    assert!(!create.headers[3].enabled);
    let ImportedItem::Folder { folder, .. } = &back.items[0] else {
        panic!("{:?}", back.items[0]);
    };
    let source_folder = match &source.items[0] {
        ImportedItem::Folder { folder, .. } => folder,
        ImportedItem::Request { .. } => unreachable!(),
    };
    assert_eq!(
        folder.defaults.auth, source_folder.defaults.auth,
        "OAuth 2.0 with PKCE"
    );
}

#[test]
fn what_postman_has_no_place_for_is_moved_or_warned_about() {
    let (export, warnings) = export_postman(&sample());
    let variables: Vec<&str> = (export["variable"].as_array().unwrap().iter())
        .map(|v| v["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        variables,
        ["baseUrl", "retries", "page"],
        "the folder's, moved up"
    );
    assert!(warned(&warnings, "Users", "folder variables are moved"));
    assert!(warned(&warnings, "", "copied into each of its requests"));
    assert!(warned(
        &warnings,
        "Users / Create",
        "assertions are not exported"
    ));
    let url = &export["item"][0]["item"][0]["request"]["url"];
    assert_eq!(url["host"], json!(["{{baseUrl}}"]));
    assert_eq!(url["path"], json!(["users", ":org"]));
    assert_eq!(url["variable"], json!([{"key": "org", "value": "acme"}]));
    assert_eq!(
        url["query"][1],
        json!({"key": "dry", "value": "true", "disabled": true})
    );
    let upload = &export["item"][1]["request"]["url"];
    assert_eq!(upload["port"], "8080");
    assert_eq!(upload["host"], json!(["h"]));
    assert_eq!(export["info"]["schema"], SCHEMA);
}

#[test]
fn environments_merge_their_chain_and_keep_secrets_out() {
    let base = Environment::new(
        "base",
        vec![
            Variable::text("host", "https://api"),
            Variable::text("page", "1"),
        ],
    );
    let mut dev = Environment::new(
        "dev",
        vec![
            Variable::text("host", "http://localhost"),
            Variable::secret("token"),
        ],
    );
    dev.extends = Some("base".into());
    let all = [base, dev];
    let (export, warnings) = export_postman_environment(&all[1], &all);
    let values = export["values"].as_array().unwrap();
    let keys: Vec<&str> = values.iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["page", "host", "token"]);
    assert_eq!(values[1]["value"], "http://localhost");
    assert_eq!(
        values[2],
        json!({"key": "token", "value": "", "type": "secret", "enabled": true})
    );
    assert!(warned(&warnings, "dev", "extends another"));
    assert!(warned(&warnings, "dev / token", "without its value"));
    let (environment, _) = crate::import_postman_environment(&export.to_string()).unwrap();
    assert_eq!(environment.variables.len(), 3);
    assert!(environment.variables[2].secret);
}
