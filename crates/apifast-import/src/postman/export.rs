//! A collection and its environments as Postman v2.1 exports, the inverse
//! of the importer: what Postman has no place for (assertions, captures,
//! request variables, timeouts…) is listed as a warning where it was.
//!
//! Postman has no inherited headers nor folder variables, so they are
//! moved to where Postman reads them: headers into each request, folder
//! variables to the collection's.

use apifast_app::{ImportWarning, ImportedCollection, ImportedItem};
use apifast_domain::{
    ApiKeyPlacement, Auth, AuthScheme, Body, CredentialsPlacement, Environment, Example, Header,
    HttpRequest, MultipartValue, OAuth2Auth, OAuth2Flow, OAuth2Request, Param, ParamKind,
    ParameterPlacement, RequestDefaults, Setting, Settings, TokenPlacement, Variable,
    environment_chain,
};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// `collection` as a Postman v2.1 collection, and what was not kept.
pub fn export_postman(collection: &ImportedCollection) -> (Value, Vec<ImportWarning>) {
    let mut exporter = Exporter {
        warnings: collection.warnings.clone(),
        hoisted: Vec::new(),
    };
    let defaults = &collection.defaults;
    let inherited = exporter.inherited_headers(&[], defaults, "");
    let items = exporter.items(&collection.items, &[], &inherited);
    let mut root = Map::new();
    root.insert(
        "info".into(),
        json!({"name": collection.name, "schema": SCHEMA}),
    );
    root.insert("item".into(), Value::Array(items));
    exporter.scope(&mut root, defaults, "");
    let mut variables = exporter.variables(&defaults.variables, "");
    for (name, value, at) in std::mem::take(&mut exporter.hoisted) {
        if variables.iter().any(|v| v["key"] == name) {
            exporter.warn(
                &at,
                format!("the folder variable `{name}` has the name of another one: not exported"),
            );
        } else {
            variables.push(json!({"key": name, "value": value, "type": "string"}));
        }
    }
    if !variables.is_empty() {
        root.insert("variable".into(), Value::Array(variables));
    }
    (Value::Object(root), exporter.warnings)
}

/// `environment` as a Postman environment. One that extends another is
/// exported with the variables of its chain merged, as Postman has no
/// inheritance; secrets go without their value.
pub fn export_postman_environment(
    environment: &Environment,
    all: &[Environment],
) -> (Value, Vec<ImportWarning>) {
    let mut warnings = Vec::new();
    let chain = environment_chain(&environment.name, all).unwrap_or_else(|e| {
        warnings.push(ImportWarning::new(
            environment.name.as_str(),
            format!("{e}: exported with its own variables only"),
        ));
        vec![environment]
    });
    if chain.len() > 1 {
        warnings.push(ImportWarning::new(
            environment.name.as_str(),
            "it extends another environment: exported with the variables of both merged",
        ));
    }
    let mut merged: Vec<&Variable> = Vec::new();
    for variable in chain.iter().flat_map(|e| &e.variables) {
        merged.retain(|v| v.name != variable.name);
        merged.push(variable);
    }
    let values: Vec<Value> = merged
        .into_iter()
        .map(|variable| {
            let at = format!("{} / {}", environment.name, variable.name);
            if variable.secret {
                warnings.push(ImportWarning::new(
                    at,
                    "a secret: exported without its value",
                ));
            }
            json!({
                "key": variable.name,
                "value": if variable.secret { String::new() } else { value_of(variable) },
                "type": if variable.secret { "secret" } else { "default" },
                "enabled": variable.enabled,
            })
        })
        .collect();
    let export = json!({
        "name": environment.name,
        "values": values,
        "_postman_variable_scope": "environment",
    });
    (export, warnings)
}

/// Why the files of an export could not be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// The collection has no environment of this name.
    #[error("no environment `{name}`; the collection has: {known}")]
    NoEnvironment {
        /// The name asked for.
        name: String,
        /// The names it has, comma-separated.
        known: String,
    },
    /// A file could not be written.
    #[error("cannot write `{path}`: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// What [`write_postman`] wrote, and what was not kept.
#[derive(Debug)]
pub struct PostmanFiles {
    /// The environments' files.
    pub environments: Vec<PathBuf>,
    /// What was not kept.
    pub warnings: Vec<ImportWarning>,
}

/// Writes `collection` to `file` as Postman v2.1, and the environments
/// named in `environments` next to it, as `<name>.postman_environment.json`
/// (Postman's own naming). Existing files are replaced.
///
/// # Errors
///
/// If an environment does not exist (before writing anything), or a file
/// cannot be written.
pub fn write_postman(
    collection: &ImportedCollection,
    file: &Path,
    environments: &[String],
) -> Result<PostmanFiles, ExportError> {
    let chosen = environments
        .iter()
        .map(|name| {
            (collection.environments.iter())
                .find(|e| e.name == *name)
                .ok_or_else(|| ExportError::NoEnvironment {
                    name: name.clone(),
                    known: (collection.environments.iter())
                        .map(|e| e.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (export, mut warnings) = export_postman(collection);
    write_json(file, &export)?;
    let folder = file.parent().unwrap_or_else(|| Path::new(""));
    let mut written = Vec::new();
    for environment in chosen {
        let (export, more) = export_postman_environment(environment, &collection.environments);
        let path = folder.join(format!(
            "{}.postman_environment.json",
            file_name(&environment.name)
        ));
        write_json(&path, &export)?;
        written.push(path);
        warnings.extend(more);
    }
    Ok(PostmanFiles {
        environments: written,
        warnings,
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), ExportError> {
    let text = serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
    std::fs::write(path, text).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// `name` without the characters a file name cannot have.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

struct Exporter {
    warnings: Vec<ImportWarning>,
    /// Folder variables for the collection: name, value, and where.
    hoisted: Vec<(String, String, String)>,
}

impl Exporter {
    fn warn(&mut self, at: &str, message: impl Into<String>) {
        self.warnings.push(ImportWarning::new(at, message));
    }

    fn items(
        &mut self,
        items: &[ImportedItem],
        path: &[String],
        inherited: &[Header],
    ) -> Vec<Value> {
        items
            .iter()
            .map(|item| match item {
                ImportedItem::Folder { folder, items } => {
                    let here = [path, std::slice::from_ref(&folder.name)].concat();
                    let at = here.join(" / ");
                    let headers = self.inherited_headers(inherited, &folder.defaults, &at);
                    let mut node = Map::new();
                    node.insert("name".into(), json!(folder.name));
                    node.insert(
                        "item".into(),
                        Value::Array(self.items(items, &here, &headers)),
                    );
                    self.scope(&mut node, &folder.defaults, &at);
                    for variable in &folder.defaults.variables {
                        if variable.enabled {
                            let value = value_of(variable);
                            self.hoisted
                                .push((variable.name.clone(), value, at.clone()));
                        }
                    }
                    if !folder.defaults.variables.is_empty() {
                        self.warn(
                            &at,
                            "folder variables are moved to the collection: Postman has none",
                        );
                    }
                    Value::Object(node)
                }
                ImportedItem::Request { request, .. } => {
                    let at = [path, std::slice::from_ref(&request.name)]
                        .concat()
                        .join(" / ");
                    self.request(request, inherited, &at)
                }
            })
            .collect()
    }

    /// The headers a scope's requests get: those of its parents, then its
    /// own, a later one replacing one of the same name.
    fn inherited_headers(
        &mut self,
        parent: &[Header],
        defaults: &RequestDefaults,
        at: &str,
    ) -> Vec<Header> {
        if !defaults.headers.is_empty() {
            self.warn(
                at,
                "its headers are copied into each of its requests: Postman does not inherit headers",
            );
        }
        merge_headers(parent, &defaults.headers)
    }

    /// What a collection or folder has besides its items: auth and
    /// settings. Variables are handled apart.
    fn scope(&mut self, node: &mut Map<String, Value>, defaults: &RequestDefaults, at: &str) {
        if let Some(auth) = self.auth(&defaults.auth, at) {
            node.insert("auth".into(), auth);
        }
        if let Some(behavior) = self.behavior(&defaults.settings, at) {
            node.insert("protocolProfileBehavior".into(), behavior);
        }
    }

    fn request(&mut self, request: &HttpRequest, inherited: &[Header], at: &str) -> Value {
        let mut headers = merge_headers(inherited, &request.headers);
        if let Body::File {
            content_type: Some(content_type),
            ..
        } = &request.body
            && !headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("content-type"))
        {
            headers.push(Header::new("Content-Type", content_type.as_str()));
        }
        let mut source = Map::new();
        source.insert("method".into(), json!(request.method.as_str()));
        source.insert("header".into(), header_list(&headers));
        if let Some(body) = self.body(&request.body, at) {
            source.insert("body".into(), body);
        }
        source.insert("url".into(), url(&request.url, &request.params));
        if let Some(auth) = self.auth(&request.auth, at) {
            source.insert("auth".into(), auth);
        }
        if let Some(docs) = request.docs.as_ref().filter(|d| !d.trim().is_empty()) {
            source.insert("description".into(), json!(docs));
        }
        let mut item = Map::new();
        item.insert("name".into(), json!(request.name));
        item.insert("request".into(), Value::Object(source));
        let responses: Vec<Value> = request
            .examples
            .iter()
            .map(|e| self.example(e, at))
            .collect();
        item.insert("response".into(), Value::Array(responses));
        if let Some(behavior) = self.behavior(&request.settings, at) {
            item.insert("protocolProfileBehavior".into(), behavior);
        }
        self.not_kept(request, at);
        Value::Object(item)
    }

    /// Warnings for what a request has that Postman does not.
    fn not_kept(&mut self, request: &HttpRequest, at: &str) {
        let lost = [
            (
                !request.assertions.is_empty(),
                "its assertions are not exported: Postman checks responses in test scripts",
            ),
            (
                !request.captures.is_empty(),
                "its captures are not exported: Postman sets variables in scripts",
            ),
            (
                !request.variables.is_empty(),
                "its variables are not exported: Postman requests have none",
            ),
            (!request.tags.is_empty(), "its tags are not exported"),
        ];
        for (_, message) in lost.into_iter().filter(|(lost, _)| *lost) {
            self.warn(at, message);
        }
    }

    fn body(&mut self, body: &Body, at: &str) -> Option<Value> {
        let raw = |text: &str, language: &str| json!({"mode": "raw", "raw": text, "options": {"raw": {"language": language}}});
        Some(match body {
            Body::None => return None,
            Body::Json(text) => raw(text, "json"),
            Body::Xml(text) => raw(text, "xml"),
            Body::Text(text) => raw(text, "text"),
            Body::Sparql(text) => {
                self.warn(at, "a SPARQL body: exported as text");
                raw(text, "text")
            }
            Body::FormUrlEncoded(fields) => json!({
                "mode": "urlencoded",
                "urlencoded": fields.iter().map(|f| with_disabled(
                    json!({"key": f.name, "value": f.value}),
                    f.enabled,
                )).collect::<Vec<_>>(),
            }),
            Body::Multipart(parts) => json!({
                "mode": "formdata",
                "formdata": parts.iter().map(|part| {
                    let mut field = match &part.value {
                        MultipartValue::Text(value) => {
                            json!({"key": part.name, "value": value, "type": "text"})
                        }
                        MultipartValue::Files(paths) => {
                            let src = match paths.as_slice() {
                                [one] => json!(one),
                                many => json!(many),
                            };
                            json!({"key": part.name, "src": src, "type": "file"})
                        }
                    };
                    if let Some(content_type) = &part.content_type {
                        field["contentType"] = json!(content_type);
                    }
                    with_disabled(field, part.enabled)
                }).collect::<Vec<_>>(),
            }),
            Body::File { path, .. } => json!({"mode": "file", "file": {"src": path}}),
            _ => {
                self.warn(at, "its body cannot be exported yet: left out");
                return None;
            }
        })
    }

    /// Postman's `auth`; `None` inherits, as an absent one does in Postman.
    fn auth(&mut self, auth: &Auth, at: &str) -> Option<Value> {
        let Setting::Set(scheme) = auth else {
            return None;
        };
        let (kind, fields): (&str, Vec<(&str, Value)>) = match scheme {
            AuthScheme::None => return Some(json!({"type": "noauth"})),
            AuthScheme::Basic(basic) => (
                "basic",
                vec![
                    ("username", json!(basic.username)),
                    ("password", json!(basic.password)),
                ],
            ),
            AuthScheme::Bearer(bearer) => ("bearer", vec![("token", json!(bearer.token))]),
            AuthScheme::Digest(digest) => (
                "digest",
                vec![
                    ("username", json!(digest.username)),
                    ("password", json!(digest.password)),
                ],
            ),
            AuthScheme::Ntlm(ntlm) => (
                "ntlm",
                vec![
                    ("username", json!(ntlm.username)),
                    ("password", json!(ntlm.password)),
                    ("domain", json!(ntlm.domain)),
                ],
            ),
            AuthScheme::ApiKey(key) => (
                "apikey",
                vec![
                    ("key", json!(key.key)),
                    ("value", json!(key.value)),
                    (
                        "in",
                        json!(match key.placement {
                            ApiKeyPlacement::Query => "query",
                            ApiKeyPlacement::Header => "header",
                        }),
                    ),
                ],
            ),
            AuthScheme::AwsV4(aws) => {
                if !aws.profile_name.is_empty() {
                    self.warn(
                        at,
                        "the AWS profile is not exported: Postman takes the keys",
                    );
                }
                (
                    "awsv4",
                    vec![
                        ("accessKey", json!(aws.access_key_id)),
                        ("secretKey", json!(aws.secret_access_key)),
                        ("sessionToken", json!(aws.session_token)),
                        ("service", json!(aws.service)),
                        ("region", json!(aws.region)),
                    ],
                )
            }
            AuthScheme::OAuth2(oauth) => ("oauth2", oauth2_fields(oauth)),
            AuthScheme::Unsupported(kind) => {
                self.warn(
                    at,
                    format!("`{kind}` auth cannot be exported: left without auth"),
                );
                return Some(json!({"type": "noauth"}));
            }
            _ => {
                self.warn(at, "its auth cannot be exported yet: left without auth");
                return Some(json!({"type": "noauth"}));
            }
        };
        let list: Vec<Value> = fields
            .into_iter()
            .map(|(key, value)| {
                let kind = match value {
                    Value::Bool(_) => "boolean",
                    Value::Array(_) => "any",
                    _ => "string",
                };
                json!({"key": key, "value": value, "type": kind})
            })
            .collect();
        Some(json!({"type": kind, kind: list}))
    }

    /// Postman's `protocolProfileBehavior` for what `settings` sets.
    fn behavior(&mut self, settings: &Settings, at: &str) -> Option<Value> {
        let mut behavior = Map::new();
        if let Setting::Set(follow) = settings.follow_redirects {
            behavior.insert("followRedirects".into(), json!(follow));
        }
        if let Setting::Set(max) = settings.max_redirects {
            behavior.insert("maxRedirects".into(), json!(max));
        }
        if let Setting::Set(encode) = settings.encode_url {
            behavior.insert("disableUrlEncoding".into(), json!(!encode));
        }
        if let Setting::Set(_) = settings.timeout_ms {
            self.warn(
                at,
                "its timeout is not exported: Postman has one for all requests",
            );
        }
        (!behavior.is_empty()).then_some(Value::Object(behavior))
    }

    /// Collection variables. Secrets go without their value.
    fn variables(&mut self, variables: &[Variable], at: &str) -> Vec<Value> {
        variables
            .iter()
            .map(|variable| {
                if variable.secret {
                    let at = if at.is_empty() {
                        variable.name.clone()
                    } else {
                        format!("{at} / {}", variable.name)
                    };
                    self.warn(&at, "a secret: exported without its value");
                }
                let mut value = json!({
                    "key": variable.name,
                    "value": if variable.secret { String::new() } else { value_of(variable) },
                    "type": "string",
                });
                if !variable.enabled {
                    value["disabled"] = json!(true);
                }
                value
            })
            .collect()
    }

    fn example(&mut self, example: &Example, at: &str) -> Value {
        let mut response = Map::new();
        response.insert("name".into(), json!(example.name));
        if let Some(request) = &example.request {
            let mut original = Map::new();
            original.insert("method".into(), json!(request.method.as_str()));
            original.insert("header".into(), header_list(&request.headers));
            if let Some(body) = self.body(&request.body, at) {
                original.insert("body".into(), body);
            }
            original.insert("url".into(), url(&request.url, &request.params));
            response.insert("originalRequest".into(), Value::Object(original));
        }
        if let Some(saved) = &example.response {
            if let Some(status) = saved.status {
                response.insert("code".into(), json!(status));
            }
            if let Some(text) = &saved.status_text {
                response.insert("status".into(), json!(text));
            }
            response.insert("header".into(), header_list(&saved.headers));
            if let Some(body) = &saved.body {
                let language = match body.kind.as_str() {
                    "json" | "xml" | "html" => body.kind.as_str(),
                    _ => "text",
                };
                response.insert("_postman_previewlanguage".into(), json!(language));
                response.insert("body".into(), json!(body.data));
            }
        }
        Value::Object(response)
    }
}

fn oauth2_fields(oauth: &OAuth2Auth) -> Vec<(&'static str, Value)> {
    let grant = match (oauth.flow, oauth.pkce) {
        (OAuth2Flow::AuthorizationCode, true) => "authorization_code_with_pkce",
        (OAuth2Flow::AuthorizationCode, false) => "authorization_code",
        (OAuth2Flow::Password, _) => "password_credentials",
        (OAuth2Flow::Implicit, _) => "implicit",
        (OAuth2Flow::ClientCredentials, _) => "client_credentials",
    };
    let (add_to, prefix) = match &oauth.token_placement {
        TokenPlacement::Header(prefix) => ("header", prefix.as_str()),
        TokenPlacement::Query(_) => ("queryParams", ""),
    };
    let params = |request: OAuth2Request| {
        let list: Vec<Value> = (oauth.parameters.iter())
            .filter(|p| p.request == request)
            .map(|p| {
                let send_as = match p.placement {
                    ParameterPlacement::Query => "request_url",
                    ParameterPlacement::Body => "request_body",
                    ParameterPlacement::Header => "request_header",
                };
                json!({"key": p.name, "value": p.value, "enabled": true, "send_as": send_as})
            })
            .collect();
        Value::Array(list)
    };
    let client_authentication = match oauth.credentials_placement {
        CredentialsPlacement::Body => "body",
        CredentialsPlacement::BasicAuthHeader => "header",
    };
    vec![
        ("grant_type", json!(grant)),
        ("authUrl", json!(oauth.authorization_url)),
        ("accessTokenUrl", json!(oauth.access_token_url)),
        ("refreshTokenUrl", json!(oauth.refresh_token_url)),
        ("redirect_uri", json!(oauth.callback_url)),
        ("clientId", json!(oauth.client_id)),
        ("clientSecret", json!(oauth.client_secret)),
        ("client_authentication", json!(client_authentication)),
        ("username", json!(oauth.username)),
        ("password", json!(oauth.password)),
        ("scope", json!(oauth.scope)),
        ("state", json!(oauth.state)),
        ("tokenName", json!(oauth.token_id)),
        ("addTokenTo", json!(add_to)),
        ("headerPrefix", json!(prefix)),
        ("authRequestParams", params(OAuth2Request::Authorization)),
        ("tokenRequestParams", params(OAuth2Request::Token)),
        ("refreshRequestParams", params(OAuth2Request::Refresh)),
    ]
}

/// `parent`'s headers, then `own`, which replace those of the same name.
fn merge_headers(parent: &[Header], own: &[Header]) -> Vec<Header> {
    let mut out: Vec<Header> = (parent.iter())
        .filter(|h| !own.iter().any(|o| o.name.eq_ignore_ascii_case(&h.name)))
        .cloned()
        .collect();
    out.extend(own.iter().cloned());
    out
}

fn header_list(headers: &[Header]) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|h| with_disabled(json!({"key": h.name, "value": h.value}), h.enabled))
            .collect(),
    )
}

fn with_disabled(mut value: Value, enabled: bool) -> Value {
    if !enabled {
        value["disabled"] = json!(true);
    }
    value
}

fn value_of(variable: &Variable) -> String {
    variable
        .value
        .as_ref()
        .map(|v| v.data.clone())
        .unwrap_or_default()
}

/// Postman's URL object: the raw URL and its parts, with the disabled query
/// parameters (which the URL does not show) and the path parameters.
fn url(raw: &str, params: &[Param]) -> Value {
    let (base, query) = raw.split_once('?').unwrap_or((raw, ""));
    let (protocol, rest) = match base.split_once("://") {
        Some((protocol, rest)) => (Some(protocol), rest),
        None => (None, base),
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (name, Some(port))
        }
        _ => (host, None),
    };
    let mut url = Map::new();
    url.insert("raw".into(), json!(raw));
    if let Some(protocol) = protocol {
        url.insert("protocol".into(), json!(protocol));
    }
    if !host.is_empty() {
        url.insert("host".into(), json!(host.split('.').collect::<Vec<_>>()));
    }
    if let Some(port) = port {
        url.insert("port".into(), json!(port));
    }
    if !path.is_empty() {
        url.insert("path".into(), json!(path.split('/').collect::<Vec<_>>()));
    }
    let mut queries: Vec<Value> = (query.split('&'))
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => json!({"key": key, "value": value}),
            None => json!({"key": pair, "value": null}),
        })
        .collect();
    for param in params
        .iter()
        .filter(|p| p.kind == ParamKind::Query && !p.enabled)
    {
        queries.push(json!({"key": param.name, "value": param.value, "disabled": true}));
    }
    if !queries.is_empty() {
        url.insert("query".into(), Value::Array(queries));
    }
    let variables: Vec<Value> = (params.iter())
        .filter(|p| p.kind == ParamKind::Path)
        .map(|p| json!({"key": p.name, "value": p.value}))
        .collect();
    if !variables.is_empty() {
        url.insert("variable".into(), Value::Array(variables));
    }
    Value::Object(url)
}

#[cfg(test)]
mod tests;
