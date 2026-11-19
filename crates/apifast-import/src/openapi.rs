//! OpenAPI 3 (3.0 and 3.1) as a collection, with Bruno's choices where it
//! has them (`bruno-converters/src/openapi/openapi-to-bruno.js`):
//!
//! - every request starts with `{{baseUrl}}`, the first server; with more,
//!   an environment per server;
//! - a folder per operation's first tag, in the order of `tags`; untagged
//!   operations at the root;
//! - `{id}` in a path becomes a path parameter (`:id`), with its example;
//! - query and header parameters are listed, on when required;
//! - the body and the responses take the spec's examples, or one built
//!   from the schema;
//! - security becomes auth with variables to fill in (`{{token}}`).
//!
//! Only local `$ref`s are followed; Swagger 2.0 is not read yet.

mod schema;
mod security;

use apifast_app::{ImportWarning, ImportedCollection, ImportedItem};
use apifast_domain::{
    Auth, Body, Environment, Example, ExampleBody, ExampleResponse, FormField, Header, HttpRequest,
    Method, MultipartPart, MultipartValue, Param, ParamKind, RequestDefaults, Setting, Variable,
    reason_phrase, sync_params_from_url,
};
use serde_json::Value;

use schema::Spec;

/// The HTTP methods of a path item, in the order requests are listed.
const METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

/// Why a document is not an OpenAPI 3 spec apifast reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OpenApiError {
    /// It is not OpenAPI.
    #[error("not an OpenAPI document")]
    NotOpenApi,
    /// It is Swagger 2.0.
    #[error("Swagger 2.0 is not supported yet; convert it to OpenAPI 3 first")]
    Swagger2,
}

/// The collection the OpenAPI 3 document `spec` describes (read from JSON
/// or YAML into a JSON value).
///
/// # Errors
///
/// If it is not an OpenAPI 3 document.
pub fn import_openapi(spec: &Value) -> Result<ImportedCollection, OpenApiError> {
    if spec.get("swagger").is_some() {
        return Err(OpenApiError::Swagger2);
    }
    let version = spec
        .get("openapi")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !version.starts_with('3') {
        return Err(OpenApiError::NotOpenApi);
    }
    let doc = Spec { root: spec };
    let mut warnings = Vec::new();
    let title = (spec.pointer("/info/title").and_then(Value::as_str))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("OpenAPI collection");

    let servers = servers(spec);
    let mut defaults = RequestDefaults::default();
    defaults.variables.push(Variable::text(
        "baseUrl",
        servers
            .first()
            .map(|(_, url)| url.clone())
            .unwrap_or_default(),
    ));
    if servers.is_empty() {
        warnings.push(ImportWarning::new(
            "",
            "the spec has no servers: set `baseUrl`",
        ));
    }
    let global_security = spec.get("security");
    if let Some(security) = global_security {
        match security::auth(&doc, security) {
            Ok(auth) => defaults.auth = Setting::Set(auth),
            Err(why) => warnings.push(ImportWarning::new("", why)),
        }
    }
    let environments = if servers.len() > 1 {
        (servers.iter())
            .map(|(name, url)| {
                Environment::new(name.clone(), vec![Variable::text("baseUrl", url.clone())])
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut folders: Vec<(String, Vec<ImportedItem>)> = tag_order(spec)
        .into_iter()
        .map(|tag| (tag, Vec::new()))
        .collect();
    let mut root = Vec::new();
    for (path, item) in spec
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(item) = doc.resolve(item) else {
            warnings.push(ImportWarning::new(
                path.as_str(),
                "its `$ref` cannot be followed",
            ));
            continue;
        };
        for method in METHODS {
            let Some(operation) = item.get(*method) else {
                continue;
            };
            let request = operation_request(&doc, (path, method, item, operation), &mut warnings);
            let tag = (operation.get("tags").and_then(Value::as_array))
                .and_then(|tags| tags.first())
                .and_then(Value::as_str);
            let item = ImportedItem::Request(Box::new(request));
            match tag {
                Some(tag) => match folders.iter_mut().find(|(name, _)| name == tag) {
                    Some((_, items)) => items.push(item),
                    None => folders.push((tag.to_owned(), vec![item])),
                },
                None => root.push(item),
            }
        }
    }
    let mut items: Vec<ImportedItem> = folders
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(name, items)| ImportedItem::Folder {
            folder: apifast_domain::Folder::new(name),
            items,
        })
        .collect();
    items.extend(root);
    Ok(ImportedCollection {
        name: title.to_owned(),
        defaults,
        items,
        environments,
        warnings,
    })
}

/// The servers: a name for each (its description, or its URL) and its URL
/// with its variables at their defaults, without a trailing `/`.
fn servers(spec: &Value) -> Vec<(String, String)> {
    let list = spec
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|server| {
            let mut url = server.get("url")?.as_str()?.to_owned();
            for (name, variable) in server
                .get("variables")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let default = text(variable, "default");
                url = url.replace(&format!("{{{name}}}"), &default);
            }
            let url = url.trim_end_matches('/').to_owned();
            let name = Some(text(server, "description"))
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| url.clone());
            Some((name, url))
        })
        .collect()
}

/// The tags, in the order the spec lists them.
fn tag_order(spec: &Value) -> Vec<String> {
    (spec
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|t| t.get("name").and_then(Value::as_str))
    .map(str::to_owned)
    .collect()
}

fn operation_request(
    doc: &Spec<'_>,
    (path, method, item, operation): (&str, &str, &Value, &Value),
    warnings: &mut Vec<ImportWarning>,
) -> HttpRequest {
    let name = [text(operation, "summary"), text(operation, "operationId")]
        .into_iter()
        .map(|n| n.lines().next().unwrap_or_default().trim().to_owned())
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| format!("{} {path}", method.to_ascii_uppercase()));
    let at = name.clone();
    // `{id}` as `:id`, apifast's path parameters.
    let url_path = path.replace('{', ":").replace('}', "");
    let mut request = HttpRequest::new(
        name,
        Method::parse(&method.to_ascii_uppercase()),
        format!("{{{{baseUrl}}}}{url_path}"),
    );
    request.docs = Some(text(operation, "description")).filter(|d| !d.trim().is_empty());

    // Parameters of the path item, then the operation's, which win.
    let mut parameters: Vec<&Value> = Vec::new();
    for list in [item.get("parameters"), operation.get("parameters")] {
        for parameter in list.and_then(Value::as_array).into_iter().flatten() {
            let Some(parameter) = doc.resolve(parameter) else {
                warnings.push(ImportWarning::new(
                    &at,
                    "a parameter's `$ref` cannot be followed",
                ));
                continue;
            };
            let key = (text(parameter, "name"), text(parameter, "in"));
            parameters.retain(|p| (text(p, "name"), text(p, "in")) != key);
            parameters.push(parameter);
        }
    }
    let mut query = Vec::new();
    for parameter in parameters {
        let name = text(parameter, "name");
        let value = parameter_value(doc, parameter);
        let required = parameter.get("required") == Some(&Value::Bool(true));
        match text(parameter, "in").as_str() {
            "path" => request.params.push(Param {
                name,
                value,
                kind: ParamKind::Path,
                enabled: true,
            }),
            "query" if required => query.push(format!("{name}={value}")),
            "query" => request.params.push(Param {
                name,
                value,
                kind: ParamKind::Query,
                enabled: false,
            }),
            "header" => request.headers.push(Header {
                enabled: required,
                ..Header::new(name, value)
            }),
            other => warnings.push(ImportWarning::new(
                &at,
                format!("the `{other}` parameter `{name}` is not imported"),
            )),
        }
    }
    if !query.is_empty() {
        request.url = format!("{}?{}", request.url, query.join("&"));
    }

    if let Some(body) = operation.get("requestBody").and_then(|b| doc.resolve(b)) {
        request.body = request_body(doc, body, &mut request.headers, &at, warnings);
    }
    if let Some(security) = operation.get("security") {
        match security::auth(doc, security) {
            Ok(auth) => request.auth = Auth::Set(auth),
            Err(why) => warnings.push(ImportWarning::new(&at, why)),
        }
    }
    request.examples = examples(doc, operation);
    sync_params_from_url(&mut request);
    request
}

/// A parameter's example, default or first allowed value, as text.
fn parameter_value(doc: &Spec<'_>, parameter: &Value) -> String {
    let example = parameter
        .get("example")
        .or_else(|| {
            parameter
                .get("examples")
                .and_then(Value::as_object)
                .and_then(|e| e.values().next())
                .and_then(|e| doc.resolve(e))
                .and_then(|e| e.get("value"))
        })
        .cloned()
        .or_else(|| parameter.get("schema").map(|s| doc.example(s)));
    match example {
        Some(Value::String(text)) => text,
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// The body for the first content type apifast has a body for.
fn request_body(
    doc: &Spec<'_>,
    body: &Value,
    headers: &mut Vec<Header>,
    at: &str,
    warnings: &mut Vec<ImportWarning>,
) -> Body {
    let Some(content) = body.get("content").and_then(Value::as_object) else {
        return Body::None;
    };
    let Some((media, entry)) = content.iter().next() else {
        return Body::None;
    };
    let example = media_example(doc, entry);
    let lower = media.to_ascii_lowercase();
    let schema = entry.get("schema").and_then(|s| doc.resolve(s));
    let pretty = |value: &Value| match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    // A type apifast does not set by itself goes as a header.
    let standard = [
        "application/json",
        "application/xml",
        "text/xml",
        "text/plain",
        "application/x-www-form-urlencoded",
        "multipart/form-data",
    ];
    if !standard.contains(&lower.as_str())
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(Header::new("Content-Type", media.clone()));
    }
    if lower.contains("json") {
        Body::Json(pretty(&example))
    } else if lower.contains("x-www-form-urlencoded") {
        Body::FormUrlEncoded(
            fields(&example)
                .into_iter()
                .map(|(name, value)| FormField {
                    name,
                    value,
                    enabled: true,
                })
                .collect(),
        )
    } else if lower.starts_with("multipart/") {
        let binary = |name: &str| {
            schema
                .and_then(|s| s.pointer(&format!("/properties/{name}")))
                .and_then(|p| doc.resolve(p))
                .is_some_and(|p| {
                    matches!(
                        p.get("format").and_then(Value::as_str),
                        Some("binary" | "base64")
                    )
                })
        };
        Body::Multipart(
            fields(&example)
                .into_iter()
                .map(|(name, value)| MultipartPart {
                    value: if binary(&name) {
                        MultipartValue::Files(Vec::new())
                    } else {
                        MultipartValue::Text(value)
                    },
                    name,
                    content_type: None,
                    enabled: true,
                })
                .collect(),
        )
    } else if lower.contains("xml") {
        warnings.push(ImportWarning::new(
            at,
            "an XML body: written empty, as its schema is not turned into XML",
        ));
        Body::Xml(String::new())
    } else if lower == "application/octet-stream" {
        Body::File {
            path: String::new(),
            content_type: Some(media.clone()),
        }
    } else {
        Body::Text(pretty(&example))
    }
}

/// The example of a media type entry: `example`, the first of `examples`,
/// or one built from its schema.
fn media_example(doc: &Spec<'_>, entry: &Value) -> Value {
    if let Some(example) = entry.get("example") {
        return example.clone();
    }
    if let Some(first) = entry
        .get("examples")
        .and_then(Value::as_object)
        .and_then(|e| e.values().next())
        && let Some(value) = doc.resolve(first).and_then(|e| e.get("value"))
    {
        return value.clone();
    }
    entry.get("schema").map_or(Value::Null, |s| doc.example(s))
}

/// An example object's fields, as text.
fn fields(example: &Value) -> Vec<(String, String)> {
    (example.as_object().into_iter().flatten())
        .map(|(name, value)| {
            let value = match value {
                Value::String(text) => text.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            (name.clone(), value)
        })
        .collect()
}

/// The responses with content, as examples: the spec's examples, or one
/// built from the schema.
fn examples(doc: &Spec<'_>, operation: &Value) -> Vec<Example> {
    let mut out = Vec::new();
    for (status, response) in operation
        .get("responses")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(response) = doc.resolve(response) else {
            continue;
        };
        let Some((media, entry)) = response
            .get("content")
            .and_then(Value::as_object)
            .and_then(|c| c.iter().next())
        else {
            continue;
        };
        let code = status.parse::<u16>().ok();
        let data = match media_example(doc, entry) {
            Value::Null => continue,
            Value::String(text) => text,
            other => serde_json::to_string_pretty(&other).unwrap_or_default(),
        };
        let name = Some(text(response, "description"))
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| status.clone());
        out.push(Example {
            name: format!(
                "{status} {}",
                name.lines().next().unwrap_or_default().trim()
            ),
            description: None,
            request: None,
            response: Some(ExampleResponse {
                status: code,
                status_text: code.and_then(reason_phrase).map(str::to_owned),
                headers: vec![Header::new("Content-Type", media.clone())],
                body: Some(ExampleBody {
                    kind: ExampleBody::kind_for(media).to_owned(),
                    data,
                }),
            }),
        });
    }
    out
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
