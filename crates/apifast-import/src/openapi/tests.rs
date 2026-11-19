use apifast_domain::{ApiKeyPlacement, AuthScheme, OAuth2Flow};
use serde_json::json;

use super::*;

fn spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {"title": "Pet Store", "version": "1"},
        "servers": [
            {"url": "https://{region}.pets.test/v1/", "description": "Production", "variables": {"region": {"default": "eu"}}},
            {"url": "http://localhost:8080", "description": "Local"}
        ],
        "security": [{"bearer": []}],
        "tags": [{"name": "pets"}, {"name": "store"}],
        "paths": {
            "/pets/{petId}": {
                "parameters": [{"name": "petId", "in": "path", "required": true, "schema": {"type": "integer", "example": 7}}],
                "get": {
                    "tags": ["pets"],
                    "summary": "Get a pet\nwith more lines",
                    "description": "Returns one pet.",
                    "parameters": [
                        {"name": "fields", "in": "query", "required": true, "schema": {"type": "string", "default": "all"}},
                        {"name": "debug", "in": "query", "schema": {"type": "boolean"}},
                        {"name": "X-Trace", "in": "header", "required": true, "example": "abc"},
                        {"name": "session", "in": "cookie"}
                    ],
                    "responses": {
                        "200": {"description": "The pet", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Pet"}}}},
                        "404": {"description": "Not found"}
                    }
                },
                "delete": {"tags": ["pets"], "operationId": "deletePet", "security": [], "responses": {"204": {"description": "Gone"}}}
            },
            "/pets": {
                "post": {
                    "tags": ["pets"],
                    "operationId": "addPet",
                    "security": [{"key": []}],
                    "requestBody": {"$ref": "#/components/requestBodies/Pet"},
                    "responses": {"201": {"description": "Created", "content": {"application/json": {"example": {"id": 1}}}}}
                }
            },
            "/upload": {
                "post": {
                    "security": [{"oauth": ["write:pets", "read:pets"]}],
                    "requestBody": {"content": {"multipart/form-data": {"schema": {"properties": {
                        "file": {"type": "string", "format": "binary"},
                        "note": {"type": "string"}
                    }}}}},
                    "responses": {}
                }
            },
            "/login": {
                "post": {
                    "tags": ["store"],
                    "requestBody": {"content": {"application/x-www-form-urlencoded": {"example": {"user": "ana", "remember": true}}}},
                    "security": [{"oidc": []}],
                    "responses": {}
                }
            },
            "/health": {"get": {"responses": {}}}
        },
        "components": {
            "schemas": {"Pet": {"type": "object", "properties": {"id": {"type": "integer"}, "name": {"type": "string", "example": "Rex"}}}},
            "requestBodies": {"Pet": {"content": {"application/vnd.pets+json": {"schema": {"$ref": "#/components/schemas/Pet"}}}}},
            "securitySchemes": {
                "bearer": {"type": "http", "scheme": "bearer"},
                "key": {"type": "apiKey", "in": "query", "name": "api_key"},
                "oauth": {"type": "oauth2", "flows": {"clientCredentials": {"tokenUrl": "https://auth.test/token", "scopes": {}}}},
                "oidc": {"type": "openIdConnect", "openIdConnectUrl": "https://auth.test/.well-known"}
            }
        }
    })
}

fn request<'a>(collection: &'a ImportedCollection, name: &str) -> &'a HttpRequest {
    fn find<'a>(items: &'a [ImportedItem], name: &str) -> Option<&'a HttpRequest> {
        items.iter().find_map(|item| match item {
            ImportedItem::Request { request: r, .. } if r.name == name => Some(r.as_ref()),
            ImportedItem::Folder { items, .. } => find(items, name),
            ImportedItem::Request { .. } => None,
        })
    }
    find(&collection.items, name).unwrap_or_else(|| panic!("no `{name}`"))
}

fn warned(collection: &ImportedCollection, at: &str, part: &str) -> bool {
    (collection.warnings.iter()).any(|w| w.at == at && w.message.contains(part))
}

#[test]
fn servers_become_the_base_url_and_environments() {
    let collection = import_openapi(&spec()).unwrap();
    assert_eq!(collection.name, "Pet Store");
    let base = &collection.defaults.variables[0];
    assert_eq!(base.name, "baseUrl");
    assert_eq!(base.value.as_ref().unwrap().data, "https://eu.pets.test/v1");
    let names: Vec<&str> = collection
        .environments
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["Production", "Local"]);
    assert!(matches!(
        collection.defaults.auth,
        Setting::Set(AuthScheme::Bearer(_))
    ));
}

#[test]
fn operations_are_grouped_by_tag_and_named() {
    let collection = import_openapi(&spec()).unwrap();
    let shape: Vec<String> = collection
        .items
        .iter()
        .map(|item| match item {
            ImportedItem::Folder { folder, items } => format!("{}({})", folder.name, items.len()),
            ImportedItem::Request { request: r, .. } => r.name.clone(),
        })
        .collect();
    assert_eq!(
        shape,
        ["pets(3)", "store(1)", "POST /upload", "GET /health"]
    );
    request(&collection, "Get a pet");
    request(&collection, "deletePet");
}

#[test]
fn parameters_become_path_params_query_and_headers() {
    let collection = import_openapi(&spec()).unwrap();
    let get = request(&collection, "Get a pet");
    assert_eq!(get.url, "{{baseUrl}}/pets/:petId?fields=all");
    assert_eq!(get.docs.as_deref(), Some("Returns one pet."));
    let path = get
        .params
        .iter()
        .find(|p| p.kind == ParamKind::Path)
        .unwrap();
    assert_eq!((path.name.as_str(), path.value.as_str()), ("petId", "7"));
    let debug = get.params.iter().find(|p| p.name == "debug").unwrap();
    assert!(!debug.enabled);
    assert_eq!(get.headers[0], Header::new("X-Trace", "abc"));
    assert!(warned(
        &collection,
        "Get a pet",
        "`cookie` parameter `session`"
    ));
    let example = &get.examples[0];
    assert_eq!(example.name, "200 The pet");
    let body = example.response.as_ref().unwrap().body.as_ref().unwrap();
    assert!(body.data.contains("\"name\": \"Rex\""), "{}", body.data);
    assert_eq!(get.examples.len(), 1, "the 404 has no content");
}

#[test]
fn bodies_and_auth_follow_the_operation() {
    let collection = import_openapi(&spec()).unwrap();
    let add = request(&collection, "addPet");
    let Body::Json(json) = &add.body else {
        panic!("{:?}", add.body)
    };
    assert!(json.contains("\"name\": \"Rex\""), "{json}");
    assert_eq!(
        add.headers[0],
        Header::new("Content-Type", "application/vnd.pets+json")
    );
    let Setting::Set(AuthScheme::ApiKey(key)) = &add.auth else {
        panic!("{:?}", add.auth)
    };
    assert_eq!(
        (key.key.as_str(), key.placement),
        ("api_key", ApiKeyPlacement::Query)
    );
    assert_eq!(
        request(&collection, "deletePet").auth,
        Setting::Set(AuthScheme::None)
    );

    let upload = request(&collection, "POST /upload");
    let Body::Multipart(parts) = &upload.body else {
        panic!("{:?}", upload.body)
    };
    assert_eq!(parts[0].value, MultipartValue::Files(Vec::new()));
    assert_eq!(parts[1].value, MultipartValue::Text("string".into()));
    let Setting::Set(AuthScheme::OAuth2(oauth)) = &upload.auth else {
        panic!("{:?}", upload.auth)
    };
    assert_eq!(oauth.flow, OAuth2Flow::ClientCredentials);
    assert_eq!(oauth.scope, "write:pets read:pets");

    let login = request(&collection, "POST /login");
    let Body::FormUrlEncoded(fields) = &login.body else {
        panic!("{:?}", login.body)
    };
    assert_eq!(
        (fields[1].name.as_str(), fields[1].value.as_str()),
        ("remember", "true")
    );
    assert!(warned(
        &collection,
        "POST /login",
        "`openIdConnect` security"
    ));
    assert_eq!(request(&collection, "GET /health").auth, Setting::Inherit);
}

#[test]
fn other_documents_are_refused() {
    assert_eq!(
        import_openapi(&json!({"swagger": "2.0"})),
        Err(OpenApiError::Swagger2)
    );
    assert_eq!(
        import_openapi(&json!({"info": {}})),
        Err(OpenApiError::NotOpenApi)
    );
    let bare = import_openapi(&json!({"openapi": "3.1.0", "paths": {}})).unwrap();
    assert_eq!(bare.name, "OpenAPI collection");
    assert!(warned(&bare, "", "no servers"));
}
