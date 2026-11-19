//! Postman collections (v2.0 and v2.1) and environments as apifast's,
//! with the same choices as Bruno's importer
//! (`bruno-converters/src/postman/postman-to-bruno.js`). What has no place
//! in apifast (scripts, some auth types, GraphQL bodies…) is listed as a
//! warning where it was, never dropped silently.

mod auth;

use std::fmt::Write as _;

use apifast_app::{ImportWarning, ImportedCollection, ImportedItem};
use apifast_domain::{
    AuthScheme, Body, Environment, Example, ExampleBody, ExampleRequest, ExampleResponse,
    FormField, Header, HttpRequest, Method, MultipartPart, MultipartValue, Param, ParamKind,
    RequestDefaults, Setting, Variable, sync_params_from_url,
};
use serde_json::Value;

/// Why a file is not a Postman collection or environment.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PostmanError {
    /// It is not JSON.
    #[error("not JSON: {0}")]
    NotJson(String),
    /// It is JSON, but not a Postman collection.
    #[error("not a Postman collection (v2.0 or v2.1)")]
    NotCollection,
    /// It is JSON, but not a Postman environment.
    #[error("not a Postman environment")]
    NotEnvironment,
}

/// The collection in the Postman export `json`.
///
/// # Errors
///
/// If it is not JSON, or not a Postman collection.
pub fn import_postman(json: &str) -> Result<ImportedCollection, PostmanError> {
    // Postman's `$guid` is apifast's `$randomUUID`.
    let json = json.replace("{{$guid}}", "{{$randomUUID}}");
    let value: Value =
        serde_json::from_str(&json).map_err(|e| PostmanError::NotJson(e.to_string()))?;
    // The API wraps the export in `{"collection": ...}`.
    let root = value.get("collection").unwrap_or(&value);
    let info = root.get("info").ok_or(PostmanError::NotCollection)?;
    let is_postman =
        text(info, "schema").contains("getpostman.com") || info.get("_postman_id").is_some();
    if !is_postman || root.get("item").and_then(Value::as_array).is_none() {
        return Err(PostmanError::NotCollection);
    }
    let mut warnings = Vec::new();
    let defaults = defaults(root, "", &mut warnings);
    let items = items(root, &[], &mut warnings);
    warnings.extend(unknown_dynamic(&json));
    Ok(ImportedCollection {
        name: non_empty(text(info, "name"), "Imported collection"),
        defaults,
        items,
        environments: Vec::new(),
        warnings,
    })
}

/// The environment in the Postman export `json`. Secret values are not
/// kept in the file: their variables are marked secret, for the vault.
///
/// # Errors
///
/// If it is not JSON, or not a Postman environment.
pub fn import_postman_environment(
    json: &str,
) -> Result<(Environment, Vec<ImportWarning>), PostmanError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| PostmanError::NotJson(e.to_string()))?;
    let values = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or(PostmanError::NotEnvironment)?;
    let name = non_empty(text(&value, "name"), "Imported environment");
    let mut warnings = Vec::new();
    let mut variables = Vec::new();
    for item in values {
        let key = text(item, "key");
        if key.is_empty() {
            continue;
        }
        let enabled = item.get("enabled") != Some(&Value::Bool(false));
        if text(item, "type") == "secret" {
            warnings.push(ImportWarning::new(
                format!("{name} / {key}"),
                "a secret: its value is not imported; set it in the vault",
            ));
            variables.push(Variable {
                enabled,
                ..Variable::secret(key)
            });
        } else {
            variables.push(Variable {
                enabled,
                ..Variable::text(key, scalar(item.get("value")))
            });
        }
    }
    Ok((Environment::new(name, variables), warnings))
}

/// What a collection or folder gives its requests: auth and variables.
fn defaults(node: &Value, at: &str, warnings: &mut Vec<ImportWarning>) -> RequestDefaults {
    scripts(node, at, warnings);
    RequestDefaults {
        auth: auth_of(node.get("auth"), at, warnings),
        variables: (node.get("variable").and_then(Value::as_array))
            .map(|list| {
                list.iter()
                    .filter(|v| !text(v, "key").is_empty())
                    .map(|v| Variable {
                        enabled: v.get("disabled") != Some(&Value::Bool(true)),
                        ..Variable::text(text(v, "key"), scalar(v.get("value")))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ..RequestDefaults::default()
    }
}

fn items(node: &Value, path: &[String], warnings: &mut Vec<ImportWarning>) -> Vec<ImportedItem> {
    let list = node
        .get("item")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .map(|item| {
            let name = non_empty(text(item, "name"), "Untitled");
            let mut here = path.to_vec();
            here.push(name.clone());
            let at = here.join(" / ");
            if item.get("item").is_some() {
                let mut folder = apifast_domain::Folder::new(name);
                folder.defaults = defaults(item, &at, warnings);
                ImportedItem::Folder {
                    folder,
                    items: items(item, &here, warnings),
                }
            } else {
                let key = Some(text(item, "id"))
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| at.clone());
                ImportedItem::Request {
                    request: Box::new(request(item, name, &at, warnings)),
                    key: Some(key),
                }
            }
        })
        .collect()
}

fn request(item: &Value, name: String, at: &str, warnings: &mut Vec<ImportWarning>) -> HttpRequest {
    scripts(item, at, warnings);
    let empty = Value::Null;
    let source = item.get("request").unwrap_or(&empty);
    let (method, url, params) = method_and_url(source);
    let mut request = HttpRequest::new(name, method, url);
    request.params = params;
    request.headers = headers(source.get("header"));
    request.body = body(source.get("body"), &request.headers, at, warnings);
    request.auth = auth_of(source.get("auth"), at, warnings);
    request.docs = description(source.get("description"));
    behavior(item, &mut request, at, warnings);
    request.examples = (item.get("response").and_then(Value::as_array))
        .map(|list| list.iter().map(example).collect())
        .unwrap_or_default();
    sync_params_from_url(&mut request);
    request
}

/// The method, the URL and its parameters that the URL does not show:
/// path parameters' values and disabled query parameters.
fn method_and_url(source: &Value) -> (Method, String, Vec<Param>) {
    // A request may be just its URL.
    if let Value::String(url) = source {
        return (Method::Get, url.clone(), Vec::new());
    }
    let method = Method::parse(&non_empty(text(source, "method"), "GET").to_ascii_uppercase());
    let Some(url) = source.get("url") else {
        return (method, String::new(), Vec::new());
    };
    if let Value::String(url) = url {
        return (method, url.clone(), Vec::new());
    }
    let mut raw = text(url, "raw");
    if raw.is_empty() {
        raw = assembled(url);
    }
    let mut params = Vec::new();
    for variable in url
        .get("variable")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        params.push(Param {
            name: text(variable, "key"),
            value: scalar(variable.get("value")),
            kind: ParamKind::Path,
            enabled: true,
        });
    }
    for query in url
        .get("query")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if query.get("disabled") == Some(&Value::Bool(true)) {
            params.push(Param {
                name: text(query, "key"),
                value: scalar(query.get("value")),
                kind: ParamKind::Query,
                enabled: false,
            });
        }
    }
    (method, raw, params)
}

/// A URL from its parts, when it has no `raw`.
fn assembled(url: &Value) -> String {
    let join = |key: &str, separator: &str| match url.get(key) {
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| scalar(Some(p)))
            .collect::<Vec<_>>()
            .join(separator),
        Some(other) => scalar(Some(other)),
        None => String::new(),
    };
    let mut out = String::new();
    let protocol = text(url, "protocol");
    if !protocol.is_empty() {
        let _ = write!(out, "{protocol}://");
    }
    out.push_str(&join("host", "."));
    let port = text(url, "port");
    if !port.is_empty() {
        let _ = write!(out, ":{port}");
    }
    let path = join("path", "/");
    if !path.is_empty() {
        out.push('/');
        out.push_str(&path);
    }
    let query: Vec<String> = (url
        .get("query")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter(|q| q.get("disabled") != Some(&Value::Bool(true)))
    .map(|q| match q.get("value") {
        Some(Value::Null) | None => text(q, "key"),
        value => format!("{}={}", text(q, "key"), scalar(value)),
    })
    .collect();
    if !query.is_empty() {
        out.push('?');
        out.push_str(&query.join("&"));
    }
    out
}

fn headers(list: Option<&Value>) -> Vec<Header> {
    let Some(Value::Array(list)) = list else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|header| match header {
            // v2.0 may write a header as `Name: value`.
            Value::String(line) => {
                let (name, value) = line.split_once(':')?;
                Some(Header::new(name.trim(), value.trim()))
            }
            object => {
                let name = text(object, "key");
                (!name.is_empty()).then(|| Header {
                    enabled: object.get("disabled") != Some(&Value::Bool(true)),
                    ..Header::new(name, scalar(object.get("value")))
                })
            }
        })
        .collect()
}

fn body(
    value: Option<&Value>,
    headers: &[Header],
    at: &str,
    warnings: &mut Vec<ImportWarning>,
) -> Body {
    let Some(value) = value.filter(|v| v.is_object()) else {
        return Body::None;
    };
    if value.get("disabled") == Some(&Value::Bool(true)) {
        return Body::None;
    }
    match text(value, "mode").as_str() {
        "raw" => {
            let raw = text(value, "raw");
            let language = value
                .pointer("/options/raw/language")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let content_type = (headers.iter())
                .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                .map(|h| h.value.to_ascii_lowercase())
                .unwrap_or_default();
            if language == "json" || content_type.contains("json") {
                Body::Json(raw)
            } else if language == "xml" || content_type.contains("xml") {
                Body::Xml(raw)
            } else {
                Body::Text(raw)
            }
        }
        "urlencoded" => Body::FormUrlEncoded(
            list(value, "urlencoded")
                .iter()
                .map(|field| FormField {
                    name: text(field, "key"),
                    value: scalar(field.get("value")),
                    enabled: field.get("disabled") != Some(&Value::Bool(true)),
                })
                .collect(),
        ),
        "formdata" => Body::Multipart(
            list(value, "formdata")
                .iter()
                .map(|part| {
                    let is_file = text(part, "type") == "file";
                    let content_type = Some(text(part, "contentType")).filter(|t| !t.is_empty());
                    MultipartPart {
                        name: text(part, "key"),
                        value: if is_file {
                            MultipartValue::Files(sources(part.get("src")))
                        } else {
                            MultipartValue::Text(scalar(part.get("value")))
                        },
                        content_type,
                        enabled: part.get("disabled") != Some(&Value::Bool(true)),
                    }
                })
                .collect(),
        ),
        "file" => Body::File {
            path: value
                .pointer("/file/src")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            content_type: None,
        },
        "graphql" => {
            warnings.push(ImportWarning::new(
                at,
                "a GraphQL body: imported as JSON with `query` and `variables`",
            ));
            let graphql = value.get("graphql").cloned().unwrap_or(Value::Null);
            let variables = match graphql.get("variables") {
                Some(Value::String(text)) => {
                    serde_json::from_str(text).unwrap_or(Value::String(text.clone()))
                }
                Some(other) => other.clone(),
                None => Value::Null,
            };
            let query = graphql.get("query").cloned().unwrap_or(Value::Null);
            let json = serde_json::json!({"query": query, "variables": variables});
            Body::Json(serde_json::to_string_pretty(&json).unwrap_or_default())
        }
        "" => Body::None,
        other => {
            warnings.push(ImportWarning::new(
                at,
                format!("a `{other}` body is not supported"),
            ));
            Body::None
        }
    }
}

/// `src` of a form part: one path or several.
fn sources(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(path)) if !path.is_empty() => vec![path.clone()],
        Some(Value::Array(paths)) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn auth_of(
    value: Option<&Value>,
    at: &str,
    warnings: &mut Vec<ImportWarning>,
) -> apifast_domain::Auth {
    auth::auth(value).unwrap_or_else(|why| {
        warnings.push(ImportWarning::new(at, why));
        Setting::Set(AuthScheme::None)
    })
}

/// Postman's per-request settings that apifast has: redirects and URL
/// encoding. The others are listed.
fn behavior(item: &Value, request: &mut HttpRequest, at: &str, warnings: &mut Vec<ImportWarning>) {
    let Some(Value::Object(behavior)) = item.get("protocolProfileBehavior") else {
        return;
    };
    for (key, value) in behavior {
        match (key.as_str(), value.as_bool()) {
            ("followRedirects", Some(follow)) => {
                request.settings.follow_redirects = Setting::Set(follow);
            }
            ("disableUrlEncoding", Some(disable)) => {
                request.settings.encode_url = Setting::Set(!disable);
            }
            ("maxRedirects", _) => {
                if let Some(max) = value.as_u64().and_then(|m| u32::try_from(m).ok()) {
                    request.settings.max_redirects = Setting::Set(max);
                }
            }
            ("disableBodyPruning", _) => {}
            _ => warnings.push(ImportWarning::new(
                at,
                format!("the `{key}` setting is not kept"),
            )),
        }
    }
}

/// A warning for scripts: they wait for the JavaScript engine (F9).
fn scripts(node: &Value, at: &str, warnings: &mut Vec<ImportWarning>) {
    let events = node
        .get("event")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for event in events {
        let lines = event.pointer("/script/exec");
        let has_code = match lines {
            Some(Value::Array(lines)) => lines
                .iter()
                .any(|l| l.as_str().is_some_and(|l| !l.trim().is_empty())),
            Some(Value::String(code)) => !code.trim().is_empty(),
            _ => false,
        };
        if has_code {
            let listen = text(&event, "listen");
            warnings.push(ImportWarning::new(
                at,
                format!(
                    "a `{listen}` script is not imported: scripts arrive with the JavaScript engine"
                ),
            ));
        }
    }
}

/// A saved response as an example.
fn example(response: &Value) -> Example {
    let request = response.get("originalRequest").map(|original| {
        let (method, url, params) = method_and_url(original);
        let headers = headers(original.get("header"));
        let body = body(original.get("body"), &headers, "", &mut Vec::new());
        ExampleRequest {
            method,
            url,
            headers,
            params,
            body,
        }
    });
    let headers = headers(response.get("header"));
    let data = text(response, "body");
    let kind = match text(response, "_postman_previewlanguage").as_str() {
        "json" => "json",
        "xml" => "xml",
        "html" => "html",
        _ => "text",
    };
    Example {
        name: non_empty(text(response, "name"), "Untitled Example"),
        description: None,
        request,
        response: Some(ExampleResponse {
            status: response
                .get("code")
                .and_then(Value::as_u64)
                .and_then(|c| u16::try_from(c).ok()),
            status_text: Some(text(response, "status")).filter(|s| !s.is_empty()),
            headers,
            body: (!data.is_empty()).then(|| ExampleBody {
                kind: kind.into(),
                data,
            }),
        }),
    }
}

/// `{{$name}}` variables apifast does not generate, once each.
fn unknown_dynamic(json: &str) -> Vec<ImportWarning> {
    let mut names = std::collections::BTreeSet::new();
    let mut rest = json;
    while let Some(start) = rest.find("{{$") {
        rest = &rest[start + 3..];
        let Some(end) = rest.find("}}") else { break };
        let name = &rest[..end];
        let known = apifast_template::DYNAMIC_VARIABLES.contains(&name);
        if !known && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            names.insert(name.to_owned());
        }
        rest = &rest[end..];
    }
    names
        .into_iter()
        .map(|name| {
            ImportWarning::new(
                "",
                format!("`{{{{${name}}}}}` is not a variable apifast generates: sends with it fail until it is replaced"),
            )
        })
        .collect()
}

fn description(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text.clone(),
        object => object.get("content")?.as_str()?.to_owned(),
    };
    (!text.trim().is_empty()).then_some(text)
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn text(value: &Value, key: &str) -> String {
    scalar(value.get(key))
}

/// A string, or a number or boolean as text; anything else is empty.
fn scalar(value: Option<&Value>) -> String {
    match value {
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
