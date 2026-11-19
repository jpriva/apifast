//! Insomnia exports as a collection, with Bruno's choices
//! (`bruno-converters/src/insomnia/insomnia-to-bruno.js`): the v4 export
//! (JSON, a flat list of resources tied by `parentId`) and the v5 one
//! (YAML, `collection.insomnia.rest/5.x`, a tree), read into a JSON value.
//!
//! - `{{ _.host }}` becomes `{{host}}`; Insomnia's template tags
//!   (`{% response … %}`) have no equivalent and are warned about.
//! - Query parameters, which Insomnia keeps apart, join the URL.
//! - Environments: the base one, and one per sub-environment merged over
//!   it. A nested value is an object variable, read with `{{user.name}}`.

use apifast_app::{ImportWarning, ImportedCollection, ImportedItem};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth, Body,
    DigestAuth, Environment, Folder, FormField, Header, HttpRequest, Method, MultipartPart,
    MultipartValue, NtlmAuth, OAuth2Auth, OAuth2Flow, Param, ParamKind, RequestDefaults, Setting,
    ValueKind, Variable, VariableValue, sync_params_from_url,
};
use serde_json::{Map, Value};

/// Why a document is not an Insomnia export.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InsomniaError {
    /// It is not an Insomnia export (v4 or v5).
    #[error("not an Insomnia export (v4 or v5)")]
    NotInsomnia,
}

/// Whether `value` looks like an Insomnia export, to tell it from other
/// formats.
pub fn is_insomnia(value: &Value) -> bool {
    is_v5(value) || is_v4(value)
}

fn is_v5(value: &Value) -> bool {
    text(value, "type").starts_with("collection.insomnia.rest/5")
}

fn is_v4(value: &Value) -> bool {
    text(value, "_type") == "export" && value.get("resources").is_some_and(Value::is_array)
}

/// The collection in the Insomnia export `export`.
///
/// # Errors
///
/// If it is not an Insomnia export, or a v5 one without its collection.
pub fn import_insomnia(export: &Value) -> Result<ImportedCollection, InsomniaError> {
    let mut warnings = Vec::new();
    let (name, items, environments) = if is_v5(export) {
        let collection = export
            .get("collection")
            .and_then(Value::as_array)
            .ok_or(InsomniaError::NotInsomnia)?;
        (
            non_empty(text(export, "name"), "Insomnia collection"),
            v5_items(collection, &[], &mut warnings),
            export
                .get("environments")
                .map(environments)
                .unwrap_or_default(),
        )
    } else if is_v4(export) {
        v4(export, &mut warnings)
    } else {
        return Err(InsomniaError::NotInsomnia);
    };
    if tags_used(export) {
        warnings.push(ImportWarning::new(
            "",
            "Insomnia template tags (`{% … %}`) are not imported: requests that use them need a variable instead",
        ));
    }
    Ok(ImportedCollection {
        name,
        defaults: RequestDefaults::default(),
        items,
        environments,
        warnings,
    })
}

/// A v5 tree: folders have `children`; requests, a `method` and a `url`.
fn v5_items(
    items: &[Value],
    path: &[String],
    warnings: &mut Vec<ImportWarning>,
) -> Vec<ImportedItem> {
    items
        .iter()
        .filter_map(|item| {
            let name = non_empty(text(item, "name"), "Untitled");
            let mut here = path.to_vec();
            here.push(name.clone());
            let at = here.join(" / ");
            if let Some(children) = item.get("children").and_then(Value::as_array) {
                let mut folder = Folder::new(name);
                folder.defaults = folder_defaults(item, &at, warnings);
                return Some(ImportedItem::Folder {
                    folder,
                    items: v5_items(children, &here, warnings),
                });
            }
            // Only requests have a method.
            item.get("method")?;
            let key = item
                .pointer("/meta/id")
                .and_then(Value::as_str)
                .map_or_else(|| at.clone(), str::to_owned);
            Some(ImportedItem::Request {
                request: Box::new(request(item, name, &at, warnings)),
                key: Some(key),
            })
        })
        .collect()
}

/// A v4 export: the workspace's name, its tree and its environments.
fn v4(
    export: &Value,
    warnings: &mut Vec<ImportWarning>,
) -> (String, Vec<ImportedItem>, Vec<Environment>) {
    let resources: Vec<&Value> = export["resources"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let workspace = resources.iter().find(|r| text(r, "_type") == "workspace");
    let name = workspace.map_or_else(String::new, |w| text(w, "name"));
    let root = workspace.map_or_else(String::new, |w| text(w, "_id"));
    let items = v4_children(&resources, &root, &[], warnings);
    // The base environment hangs from the workspace; its subs, from it.
    let environment = resources
        .iter()
        .find(|r| text(r, "_type") == "environment" && text(r, "parentId") == root)
        .map(|base| {
            let id = text(base, "_id");
            let subs: Vec<Value> = (resources.iter())
                .filter(|r| text(r, "_type") == "environment" && text(r, "parentId") == id)
                .map(|r| (*r).clone())
                .collect();
            let mut base = (*base).clone();
            base["subEnvironments"] = Value::Array(subs);
            environments(&base)
        })
        .unwrap_or_default();
    for resource in &resources {
        let kind = text(resource, "_type");
        if matches!(kind.as_str(), "grpc_request" | "websocket_request") {
            warnings.push(ImportWarning::new(
                text(resource, "name"),
                format!("a `{kind}` is not supported yet: left out"),
            ));
        }
    }
    (non_empty(name, "Insomnia collection"), items, environment)
}

/// The folders and requests under `parent`, by `metaSortKey`.
fn v4_children(
    resources: &[&Value],
    parent: &str,
    path: &[String],
    warnings: &mut Vec<ImportWarning>,
) -> Vec<ImportedItem> {
    let mut children: Vec<&&Value> = (resources.iter())
        .filter(|r| {
            text(r, "parentId") == parent
                && matches!(text(r, "_type").as_str(), "request" | "request_group")
        })
        .collect();
    children.sort_by(|a, b| {
        let key = |r: &Value| r.get("metaSortKey").and_then(Value::as_f64).unwrap_or(0.0);
        key(a).total_cmp(&key(b))
    });
    children
        .into_iter()
        .map(|resource| {
            let name = non_empty(text(resource, "name"), "Untitled");
            let mut here = path.to_vec();
            here.push(name.clone());
            let at = here.join(" / ");
            if text(resource, "_type") == "request_group" {
                let mut folder = Folder::new(name);
                folder.defaults = folder_defaults(resource, &at, warnings);
                let items = v4_children(resources, &text(resource, "_id"), &here, warnings);
                ImportedItem::Folder { folder, items }
            } else {
                let key = Some(text(resource, "_id"))
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| at.clone());
                ImportedItem::Request {
                    request: Box::new(request(resource, name, &at, warnings)),
                    key: Some(key),
                }
            }
        })
        .collect()
}

/// A folder's auth and its environment, as variables.
fn folder_defaults(folder: &Value, at: &str, warnings: &mut Vec<ImportWarning>) -> RequestDefaults {
    scripts(folder, at, warnings);
    let variables = (folder.get("environment").and_then(Value::as_object))
        .map(variables)
        .unwrap_or_default();
    RequestDefaults {
        auth: auth(folder.get("authentication"), at, warnings),
        variables,
        ..RequestDefaults::default()
    }
}

fn request(item: &Value, name: String, at: &str, warnings: &mut Vec<ImportWarning>) -> HttpRequest {
    scripts(item, at, warnings);
    let method = non_empty(text(item, "method"), "GET").to_ascii_uppercase();
    let mut url = variables_of(&text(item, "url"));
    let mut params = Vec::new();
    let mut query = Vec::new();
    for param in list(item, "parameters") {
        let (name, value) = (text(param, "name"), variables_of(&text(param, "value")));
        if disabled(param) {
            params.push(Param {
                name,
                value,
                kind: ParamKind::Query,
                enabled: false,
            });
        } else {
            query.push(if value.is_empty() {
                name
            } else {
                format!("{name}={value}")
            });
        }
    }
    if !query.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&query.join("&"));
    }
    for param in list(item, "pathParameters") {
        params.push(Param {
            name: text(param, "name"),
            value: variables_of(&text(param, "value")),
            kind: ParamKind::Path,
            enabled: true,
        });
    }
    let mut request = HttpRequest::new(name, Method::parse(&method), url);
    request.params = params;
    request.headers = (list(item, "headers").iter())
        .filter(|h| !text(h, "name").is_empty())
        .map(|h| Header {
            enabled: !disabled(h),
            ..Header::new(text(h, "name"), variables_of(&text(h, "value")))
        })
        .collect();
    request.body = body(item.get("body"), at, warnings);
    request.auth = auth(item.get("authentication"), at, warnings);
    request.docs = Some(text(item, "description")).filter(|d| !d.trim().is_empty());
    // v4 keeps the settings as `settingEncodeUrl`…; v5, in `settings`.
    let encode = item
        .get("settingEncodeUrl")
        .or_else(|| item.pointer("/settings/encodeUrl"));
    if encode == Some(&Value::Bool(false)) {
        request.settings.encode_url = Setting::Set(false);
    }
    let follow = item
        .get("settingFollowRedirects")
        .or_else(|| item.pointer("/settings/followRedirects"));
    match follow.and_then(Value::as_str) {
        Some("on") => request.settings.follow_redirects = Setting::Set(true),
        Some("off") => request.settings.follow_redirects = Setting::Set(false),
        _ => {}
    }
    sync_params_from_url(&mut request);
    request
}

fn body(value: Option<&Value>, at: &str, warnings: &mut Vec<ImportWarning>) -> Body {
    let Some(body) = value.filter(|b| b.is_object()) else {
        return Body::None;
    };
    let mime = text(body, "mimeType");
    let mime = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let content = variables_of(&text(body, "text"));
    match mime.as_str() {
        "" if content.is_empty() => Body::None,
        "" | "text/plain" => Body::Text(content),
        "application/x-www-form-urlencoded" => Body::FormUrlEncoded(
            list(body, "params")
                .iter()
                .map(|p| FormField {
                    name: text(p, "name"),
                    value: variables_of(&text(p, "value")),
                    enabled: !disabled(p),
                })
                .collect(),
        ),
        "multipart/form-data" => Body::Multipart(
            list(body, "params")
                .iter()
                .map(|p| MultipartPart {
                    name: text(p, "name"),
                    value: if text(p, "type") == "file" {
                        MultipartValue::Files(
                            Some(text(p, "fileName"))
                                .filter(|f| !f.is_empty())
                                .into_iter()
                                .collect(),
                        )
                    } else {
                        MultipartValue::Text(variables_of(&text(p, "value")))
                    },
                    content_type: None,
                    enabled: !disabled(p),
                })
                .collect(),
        ),
        "application/octet-stream" => Body::File {
            path: text(body, "fileName"),
            content_type: None,
        },
        // Insomnia keeps it as the JSON it sends.
        "application/graphql" => graphql_body(&content).unwrap_or_else(|| {
            warnings.push(ImportWarning::new(
                at,
                "a GraphQL body that is not `{\"query\": …}`: imported as JSON",
            ));
            Body::Json(content)
        }),
        m if m.contains("json") => Body::Json(content),
        m if m.contains("xml") => Body::Xml(content),
        _ => Body::Text(content),
    }
}

/// `{"query": …, "variables": …}` as a GraphQL body.
fn graphql_body(json: &str) -> Option<Body> {
    let value: Value = serde_json::from_str(json).ok()?;
    let query = value.get("query")?.as_str()?.to_owned();
    let variables = match value.get("variables") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    Some(Body::GraphQl { query, variables })
}

fn auth(value: Option<&Value>, at: &str, warnings: &mut Vec<ImportWarning>) -> Auth {
    let Some(auth) = value.filter(|a| a.as_object().is_some_and(|o| !o.is_empty())) else {
        return Setting::Inherit;
    };
    if auth.get("disabled") == Some(&Value::Bool(true)) {
        return Setting::Set(AuthScheme::None);
    }
    let get = |key: &str| variables_of(&text(auth, key));
    let scheme = match text(auth, "type").as_str() {
        "none" => AuthScheme::None,
        "basic" => AuthScheme::Basic(BasicAuth {
            username: get("username"),
            password: get("password"),
        }),
        "bearer" => AuthScheme::Bearer(BearerAuth {
            token: get("token"),
        }),
        "digest" => AuthScheme::Digest(DigestAuth {
            username: get("username"),
            password: get("password"),
        }),
        "ntlm" => AuthScheme::Ntlm(NtlmAuth {
            username: get("username"),
            password: get("password"),
            domain: String::new(),
        }),
        "apikey" => AuthScheme::ApiKey(ApiKeyAuth {
            key: get("key"),
            value: get("value"),
            placement: if text(auth, "addTo") == "queryParams" {
                ApiKeyPlacement::Query
            } else {
                ApiKeyPlacement::Header
            },
        }),
        "iam" => AuthScheme::AwsV4(Box::new(AwsV4Auth {
            access_key_id: get("accessKeyId"),
            secret_access_key: get("secretAccessKey"),
            session_token: get("sessionToken"),
            service: get("service"),
            region: get("region"),
            profile_name: String::new(),
        })),
        "oauth2" => {
            let flow = match text(auth, "grantType").as_str() {
                "password" => OAuth2Flow::Password,
                "authorization_code" => OAuth2Flow::AuthorizationCode,
                "implicit" => OAuth2Flow::Implicit,
                _ => OAuth2Flow::ClientCredentials,
            };
            AuthScheme::OAuth2(Box::new(OAuth2Auth {
                flow,
                authorization_url: get("authorizationUrl"),
                access_token_url: get("accessTokenUrl"),
                callback_url: get("redirectUrl"),
                client_id: get("clientId"),
                client_secret: get("clientSecret"),
                username: get("username"),
                password: get("password"),
                scope: get("scope"),
                state: get("state"),
                pkce: auth.get("usePkce") == Some(&Value::Bool(true)),
                ..OAuth2Auth::default()
            }))
        }
        "" => return Setting::Inherit,
        other => {
            warnings.push(ImportWarning::new(
                at,
                format!("`{other}` auth is not supported: left without auth"),
            ));
            AuthScheme::None
        }
    };
    Setting::Set(scheme)
}

/// The base environment and one per sub-environment, merged over it.
fn environments(base: &Value) -> Vec<Environment> {
    let data = |env: &Value| {
        env.get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let base_data = data(base);
    let mut out = vec![Environment::new(
        non_empty(text(base, "name"), "Base Environment"),
        variables(&base_data),
    )];
    for (i, sub) in list(base, "subEnvironments").iter().enumerate() {
        let mut merged = base_data.clone();
        merged.extend(data(sub));
        out.push(Environment::new(
            non_empty(text(sub, "name"), &format!("Environment {}", i + 1)),
            variables(&merged),
        ));
    }
    out
}

/// Variables of an environment's data: text, numbers and booleans as
/// they are; objects and lists as object variables.
fn variables(data: &Map<String, Value>) -> Vec<Variable> {
    data.iter()
        .map(|(name, value)| {
            let (kind, data) = match value {
                Value::String(text) => (ValueKind::String, variables_of(text)),
                Value::Number(n) => (ValueKind::Number, n.to_string()),
                Value::Bool(b) => (ValueKind::Bool, b.to_string()),
                Value::Null => (ValueKind::String, String::new()),
                other => (ValueKind::Object, other.to_string()),
            };
            Variable {
                value: Some(VariableValue { kind, data }),
                ..Variable::text(name.clone(), "")
            }
        })
        .collect()
}

/// A warning for scripts (`preRequestScript` in v4, `scripts` in v5).
fn scripts(item: &Value, at: &str, warnings: &mut Vec<ImportWarning>) {
    let code = [
        item.get("preRequestScript"),
        item.get("afterResponseScript"),
        item.pointer("/scripts/preRequest"),
        item.pointer("/scripts/afterResponse"),
    ];
    if code
        .iter()
        .flatten()
        .any(|c| c.as_str().is_some_and(|c| !c.trim().is_empty()))
    {
        warnings.push(ImportWarning::new(
            at,
            "scripts are not imported: they arrive with the JavaScript engine",
        ));
    }
}

/// `{{ _.name }}` as `{{name}}`.
fn variables_of(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = rest[start + 2..start + end].trim();
        let inner = inner.strip_prefix("_.").unwrap_or(inner).trim();
        out.push_str("{{");
        out.push_str(inner);
        out.push_str("}}");
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out
}

/// Whether a template tag is used anywhere.
fn tags_used(export: &Value) -> bool {
    export.to_string().contains("{%")
}

fn disabled(item: &Value) -> bool {
    item.get("disabled") == Some(&Value::Bool(true))
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn non_empty(text: String, fallback: &str) -> String {
    if text.trim().is_empty() {
        fallback.to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests;
