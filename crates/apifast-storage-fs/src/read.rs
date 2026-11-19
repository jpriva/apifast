//! YAML → domain, following how Bruno reads OpenCollection files
//! (`bruno-filestore/src/formats/yml`): missing values take Bruno's defaults
//! instead of failing.

use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, Assertion, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth,
    Body, Capture, CaptureScope, Collection, DigestAuth, Environment, Folder, FormField, GrpcCall,
    GrpcMessage, GrpcMethodType, Header, HttpRequest, Method, MultipartPart, MultipartValue,
    NtlmAuth, Operator, Param, ParamKind, RequestDefaults, Setting, Settings, ValueKind, Variable,
    VariableValue, WsMessage, WsMessageKind,
};
use apifast_domain::{
    CredentialsPlacement, OAuth2Auth, OAuth2Flow, OAuth2Parameter, OAuth2Request,
    ParameterPlacement, TokenPlacement,
};
use oneyaml::Value;

use crate::yaml::{boolean, get, get_in, is_disabled, items, text, text_or_empty, unsigned};

/// A request file: HTTP, or another kind this version only lists.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::large_enum_variant, reason = "short-lived: one per file read")]
pub(crate) enum Item {
    Http(HttpRequest),
    Other {
        name: String,
        kind: String,
        seq: Option<u32>,
    },
}

pub(crate) fn item(doc: &Value) -> Item {
    let info = get(doc, "info");
    let name = text(info.and_then(|i| get(i, "name"))).unwrap_or_else(|| "Untitled Request".into());
    let seq = info.and_then(|i| unsigned(get(i, "seq")));
    let kind = text(info.and_then(|i| get(i, "type")));
    match kind.as_deref() {
        Some("http") => Item::Http(http_request(doc, name, seq, "http")),
        // A GraphQL request is HTTP with its query as the body.
        Some("graphql") => Item::Http(http_request(doc, name, seq, "graphql")),
        // A WebSocket request too, with its messages as the body.
        Some("websocket") => Item::Http(http_request(doc, name, seq, "websocket")),
        // And a gRPC one, with its method and messages as the body.
        Some("grpc") => Item::Http(http_request(doc, name, seq, "grpc")),
        // Without a type, a file with an `http` block is still an HTTP request.
        None if get(doc, "http").is_some() => Item::Http(http_request(doc, name, seq, "http")),
        _ => Item::Other {
            name,
            kind: kind.unwrap_or_else(|| "unknown".into()),
            seq,
        },
    }
}

/// The request in the block `block` of `doc`: `http`; `graphql`, whose
/// body is the query and its variables; `websocket`, whose body is its
/// messages and whose parameters are those of its URL; or `grpc`, whose
/// body is its method and messages and whose headers are its `metadata`.
fn http_request(doc: &Value, name: String, seq: Option<u32>, block: &str) -> HttpRequest {
    let http = get(doc, block);
    let graphql = block == "graphql";
    let websocket = block == "websocket";
    let grpc = block == "grpc";
    let field = |key| http.and_then(|h| get(h, key));
    // Files from OpenCollection v3 rc1 keep the auth in `runtime.auth`.
    let auth = field("auth").or_else(|| get_in(doc, &["runtime", "auth"]));

    let mut request = HttpRequest {
        name,
        seq,
        tags: items(get_in(doc, &["info", "tags"]))
            .iter()
            .filter_map(|t| text(Some(t)))
            .collect(),
        // A gRPC block's `method` is the method it calls.
        method: if grpc {
            Method::Get
        } else {
            Method::parse(
                &text(field("method"))
                    .unwrap_or_else(|| if graphql { "POST" } else { "GET" }.into()),
            )
        },
        url: text_or_empty(field("url")),
        params: items(field("params")).iter().map(param).collect(),
        headers: headers(field(if grpc { "metadata" } else { "headers" })),
        body: if graphql {
            graphql_body(field("body"))
        } else if websocket {
            Body::WebSocket(ws_messages(field("message")))
        } else if grpc {
            Body::Grpc(grpc_call(http))
        } else {
            body(field("body"))
        },
        auth: self::auth(auth),
        variables: variables(get_in(doc, &["runtime", "variables"])),
        assertions: items(get_in(doc, &["runtime", "assertions"]))
            .iter()
            .map(assertion)
            .collect(),
        captures: items(get_in(doc, &["runtime", "actions"]))
            .iter()
            .filter(|item| is_capture(item))
            .map(capture)
            .collect(),
        settings: settings(get(doc, "settings")),
        docs: text(get(doc, "docs")),
        examples: crate::examples::read_examples(doc),
    };
    if websocket {
        // Its file has no parameters: they are those of its URL.
        apifast_domain::sync_params_from_url(&mut request);
    }
    request
}

/// The messages of a `websocket` block: a list of named ones (`title`,
/// `selected`, `message`), or a single one, as Bruno writes them.
fn ws_messages(value: Option<&Value>) -> Vec<WsMessage> {
    let message = |value: Option<&Value>| {
        (
            WsMessageKind::parse(&text_or_empty(value.and_then(|v| get(v, "type")))),
            text_or_empty(value.and_then(|v| get(v, "data"))),
        )
    };
    match value {
        Some(list) if list.as_sequence().is_some() => items(Some(list))
            .iter()
            .enumerate()
            .map(|(i, variant)| {
                let (kind, data) = message(get(variant, "message"));
                WsMessage {
                    name: text(get(variant, "title"))
                        .unwrap_or_else(|| format!("message {}", i + 1)),
                    kind,
                    data,
                    selected: boolean(get(variant, "selected")).unwrap_or(false),
                }
            })
            .collect(),
        Some(single) => {
            let (kind, data) = message(Some(single));
            if data.trim().is_empty() {
                Vec::new()
            } else {
                vec![WsMessage::new("", kind, data)]
            }
        }
        None => Vec::new(),
    }
}

/// The call of a `grpc` block: its `method`, `methodType` and
/// `protoFilePath`, and its messages, a list of named ones (`title`,
/// `message`) or a single one, as Bruno writes them.
fn grpc_call(block: Option<&Value>) -> GrpcCall {
    let field = |key| block.and_then(|b| get(b, key));
    let messages = match field("message") {
        Some(list) if list.as_sequence().is_some() => items(Some(list))
            .iter()
            .enumerate()
            .map(|(i, variant)| GrpcMessage {
                name: text(get(variant, "title")).unwrap_or_else(|| format!("message {}", i + 1)),
                data: text_or_empty(get(variant, "message")),
            })
            .collect(),
        Some(single) => {
            let data = text_or_empty(Some(single));
            if data.trim().is_empty() {
                Vec::new()
            } else {
                vec![GrpcMessage::new("", data)]
            }
        }
        None => Vec::new(),
    };
    GrpcCall {
        method: text_or_empty(field("method")),
        method_type: text(field("methodType")).and_then(|t| GrpcMethodType::parse(&t)),
        proto_path: text(field("protoFilePath")).filter(|p| !p.trim().is_empty()),
        messages,
    }
}

fn assertion(item: &Value) -> Assertion {
    Assertion {
        expression: text_or_empty(get(item, "expression")),
        operator: Operator::parse(&text_or_empty(get(item, "operator"))),
        value: text(get(item, "value")),
        enabled: !is_disabled(item),
    }
}

/// Whether an item of `runtime.actions` is a capture (`set-variable`):
/// other kinds of action are kept in the file but not modeled.
pub(crate) fn is_capture(item: &Value) -> bool {
    text(get(item, "type")).as_deref() == Some("set-variable")
}

pub(crate) fn capture(item: &Value) -> Capture {
    Capture {
        expression: text_or_empty(get_in(item, &["selector", "expression"])),
        method: text_or_empty(get_in(item, &["selector", "method"])),
        phase: text_or_empty(get(item, "phase")),
        variable: text_or_empty(get_in(item, &["variable", "name"])),
        scope: CaptureScope::parse(&text_or_empty(get_in(item, &["variable", "scope"]))),
        enabled: !is_disabled(item),
    }
}

pub(crate) fn param(item: &Value) -> Param {
    Param {
        name: text_or_empty(get(item, "name")),
        value: text_or_empty(get(item, "value")),
        kind: match text(get(item, "type")).as_deref() {
            Some("path") => ParamKind::Path,
            _ => ParamKind::Query,
        },
        enabled: !is_disabled(item),
    }
}

pub(crate) fn headers(list: Option<&Value>) -> Vec<Header> {
    items(list)
        .iter()
        .map(|item| Header {
            name: text_or_empty(get(item, "name")),
            value: text_or_empty(get(item, "value")),
            enabled: !is_disabled(item),
        })
        .collect()
}

/// Auth of a request or folder. A missing auth means none, as in Bruno; only
/// `inherit` inherits.
pub(crate) fn auth(value: Option<&Value>) -> Auth {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Setting::Set(AuthScheme::None);
    };
    if let Some(mode) = value.as_str() {
        return match mode {
            "inherit" => Setting::Inherit,
            "none" => Setting::Set(AuthScheme::None),
            other => Setting::Set(AuthScheme::Unsupported(other.to_owned())),
        };
    }
    let field = |key| text_or_empty(get(value, key));
    Setting::Set(match text(get(value, "type")).as_deref() {
        Some("inherit") => return Setting::Inherit,
        Some("none") => AuthScheme::None,
        Some("basic") => AuthScheme::Basic(BasicAuth {
            username: field("username"),
            password: field("password"),
        }),
        Some("bearer") => AuthScheme::Bearer(BearerAuth {
            token: field("token"),
        }),
        Some("apikey") => AuthScheme::ApiKey(ApiKeyAuth {
            key: field("key"),
            value: field("value"),
            placement: match text(get(value, "placement")).as_deref() {
                Some("query" | "queryparams") => ApiKeyPlacement::Query,
                _ => ApiKeyPlacement::Header,
            },
        }),
        Some("digest") => AuthScheme::Digest(DigestAuth {
            username: field("username"),
            password: field("password"),
        }),
        Some("ntlm") => AuthScheme::Ntlm(NtlmAuth {
            username: field("username"),
            password: field("password"),
            domain: field("domain"),
        }),
        Some("oauth2") => AuthScheme::OAuth2(Box::new(oauth2(value))),
        Some("awsv4") => AuthScheme::AwsV4(Box::new(AwsV4Auth {
            access_key_id: field("accessKeyId"),
            secret_access_key: field("secretAccessKey"),
            session_token: field("sessionToken"),
            service: field("service"),
            region: field("region"),
            profile_name: field("profileName"),
        })),
        Some(other) => AuthScheme::Unsupported(other.to_owned()),
        None => AuthScheme::Unsupported("unknown".into()),
    })
}

/// An `oauth2` auth, with Bruno's defaults for what is missing: the token
/// as a `Bearer` header, fetched automatically and not refreshed, PKCE off.
fn oauth2(value: &Value) -> OAuth2Auth {
    let field = |path: &[&str]| text_or_empty(get_in(value, path));
    let defaults = OAuth2Auth::default();
    let token_placement = match get_in(value, &["tokenConfig", "placement"]) {
        Some(placement) if get(placement, "query").is_some() => {
            TokenPlacement::Query(text_or_empty(get(placement, "query")))
        }
        Some(placement) if get(placement, "header").is_some() => {
            TokenPlacement::Header(text_or_empty(get(placement, "header")))
        }
        _ => defaults.token_placement.clone(),
    };
    let mut parameters = Vec::new();
    for (key, request) in [
        ("authorizationRequest", OAuth2Request::Authorization),
        ("accessTokenRequest", OAuth2Request::Token),
        ("refreshTokenRequest", OAuth2Request::Refresh),
    ] {
        for item in items(get_in(value, &["additionalParameters", key])) {
            let placement = match text(get(item, "placement")).as_deref() {
                Some("query" | "queryparams") => ParameterPlacement::Query,
                Some("body") => ParameterPlacement::Body,
                _ => ParameterPlacement::Header,
            };
            parameters.push(OAuth2Parameter {
                request,
                name: text_or_empty(get(item, "name")),
                value: text_or_empty(get(item, "value")),
                placement,
            });
        }
    }
    let flag =
        |key: &str, default: bool| boolean(get_in(value, &["settings", key])).unwrap_or(default);
    OAuth2Auth {
        flow: OAuth2Flow::parse(&field(&["flow"])),
        authorization_url: field(&["authorizationUrl"]),
        access_token_url: field(&["accessTokenUrl"]),
        refresh_token_url: field(&["refreshTokenUrl"]),
        callback_url: field(&["callbackUrl"]),
        client_id: field(&["credentials", "clientId"]),
        client_secret: field(&["credentials", "clientSecret"]),
        credentials_placement: match field(&["credentials", "placement"]).as_str() {
            "basic_auth_header" => CredentialsPlacement::BasicAuthHeader,
            _ => CredentialsPlacement::Body,
        },
        username: field(&["resourceOwner", "username"]),
        password: field(&["resourceOwner", "password"]),
        scope: field(&["scope"]),
        state: field(&["state"]),
        // A `pkce` block turns it on, unless it says `disabled: true`.
        pkce: get(value, "pkce").is_some_and(|p| !is_disabled(p)),
        token_id: text(get_in(value, &["tokenConfig", "id"])).unwrap_or(defaults.token_id),
        token_placement,
        token_source: text(get_in(value, &["tokenConfig", "source"]))
            .unwrap_or(defaults.token_source),
        auto_fetch: flag("autoFetchToken", defaults.auto_fetch),
        auto_refresh: flag("autoRefreshToken", defaults.auto_refresh),
        parameters,
    }
}

/// The body of a `graphql` block: `query` and `variables`, both text.
fn graphql_body(value: Option<&Value>) -> Body {
    let field = |key| value.and_then(|b| get(b, key));
    Body::GraphQl {
        query: text_or_empty(field("query")),
        variables: text_or_empty(field("variables")),
    }
}

pub(crate) fn body(value: Option<&Value>) -> Body {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Body::None;
    };
    let data = get(value, "data");
    match text(get(value, "type")).as_deref() {
        Some("json") => Body::Json(text_or_empty(data)),
        Some("text") => Body::Text(text_or_empty(data)),
        Some("xml") => Body::Xml(text_or_empty(data)),
        Some("sparql") => Body::Sparql(text_or_empty(data)),
        Some("form-urlencoded") => Body::FormUrlEncoded(
            items(data)
                .iter()
                .map(|item| FormField {
                    name: text_or_empty(get(item, "name")),
                    value: text_or_empty(get(item, "value")),
                    enabled: !is_disabled(item),
                })
                .collect(),
        ),
        Some("multipart-form") => Body::Multipart(items(data).iter().map(multipart_part).collect()),
        Some("file") => {
            // Bruno keeps several file variants and sends the selected one.
            let variants = items(data);
            let chosen = variants
                .iter()
                .find(|v| boolean(get(v, "selected")) == Some(true))
                .or_else(|| variants.first());
            Body::File {
                path: chosen
                    .map(|v| text_or_empty(get(v, "filePath")))
                    .unwrap_or_default(),
                content_type: chosen
                    .and_then(|v| text(get(v, "contentType")))
                    .filter(|c| !c.is_empty()),
            }
        }
        // apifast's, for an example's body (a request keeps it in its
        // `graphql` block, without a type).
        Some("graphql") => graphql_body(Some(value)),
        // Bruno sends no body for kinds it does not know, and neither do we.
        _ => Body::None,
    }
}

fn multipart_part(item: &Value) -> MultipartPart {
    let value = get(item, "value");
    MultipartPart {
        name: text_or_empty(get(item, "name")),
        value: if text(get(item, "type")).as_deref() == Some("file") {
            MultipartValue::Files(match value {
                Some(Value::Sequence(paths)) => {
                    paths.iter().filter_map(|p| text(Some(p))).collect()
                }
                other => text(other).into_iter().filter(|p| !p.is_empty()).collect(),
            })
        } else {
            MultipartValue::Text(text_or_empty(value))
        },
        content_type: text(get(item, "contentType")).filter(|c| !c.is_empty()),
        enabled: !is_disabled(item),
    }
}

/// Variables of a request, folder or collection. Environment variables, which
/// can be secrets, go through [`environment`].
pub(crate) fn variables(list: Option<&Value>) -> Vec<Variable> {
    items(list).iter().map(variable).collect()
}

fn variable(item: &Value) -> Variable {
    Variable {
        name: text_or_empty(get(item, "name")),
        value: Some(variable_value(get(item, "value"))),
        enabled: !is_disabled(item),
        secret: false,
    }
}

fn variable_value(value: Option<&Value>) -> VariableValue {
    match value {
        Some(typed @ Value::Mapping(_))
            if get(typed, "type").is_some() && get(typed, "data").is_some() =>
        {
            VariableValue {
                kind: value_kind(text(get(typed, "type")).as_deref()),
                data: text_or_empty(get(typed, "data")),
            }
        }
        other => VariableValue::text(text_or_empty(other)),
    }
}

pub(crate) fn value_kind(kind: Option<&str>) -> ValueKind {
    match kind {
        Some("number") => ValueKind::Number,
        Some("boolean") => ValueKind::Bool,
        Some("object") => ValueKind::Object,
        _ => ValueKind::String,
    }
}

/// Request settings.
///
/// Without a `settings` block, everything inherits. With one, a value that is
/// set is the request's own; `timeout` follows Bruno: a number is used as is
/// (0 means no limit), `inherit` inherits, and anything else is 0.
fn settings(value: Option<&Value>) -> Settings {
    let Some(value) = value.filter(|v| v.as_mapping().is_some()) else {
        return Settings::default();
    };
    let timeout = get(value, "timeout");
    Settings {
        timeout_ms: match timeout {
            Some(Value::String(s)) if s == "inherit" => Setting::Inherit,
            other => Setting::Set(unsigned(other).unwrap_or(0)),
        },
        follow_redirects: boolean(get(value, "followRedirects"))
            .map_or(Setting::Inherit, Setting::Set),
        max_redirects: unsigned(get(value, "maxRedirects")).map_or(Setting::Inherit, Setting::Set),
        encode_url: boolean(get(value, "encodeUrl")).map_or(Setting::Inherit, Setting::Set),
    }
}

/// `folder.yml`. `dir_name` names a folder whose file has no name.
pub(crate) fn folder(doc: &Value, dir_name: &str) -> Folder {
    let info = get(doc, "info");
    Folder {
        name: text(info.and_then(|i| get(i, "name"))).unwrap_or_else(|| dir_name.to_owned()),
        seq: info.and_then(|i| unsigned(get(i, "seq"))),
        defaults: request_defaults(get(doc, "request"), auth(get_in(doc, &["request", "auth"]))),
    }
}

/// `opencollection.yml`. The identifier comes from `.apifast/`, not from here.
pub(crate) fn collection(doc: &Value) -> Collection {
    let auth_value = get_in(doc, &["request", "auth"]);
    // At the collection level there is nothing to inherit from, so a missing
    // auth is the default (no auth) rather than an explicit one.
    let collection_auth = if auth_value.is_some_and(|v| !v.is_null()) {
        auth(auth_value)
    } else {
        Setting::Inherit
    };
    Collection {
        id: None,
        name: text(get_in(doc, &["info", "name"])).unwrap_or_else(|| "Untitled Collection".into()),
        defaults: request_defaults(get(doc, "request"), collection_auth),
        proxy: proxy(get_in(doc, &["config", "proxy"])),
        client_certificates: client_certificates(get_in(doc, &["config", "clientCertificates"])),
        // From `.apifast/`, read apart.
        dns_overrides: Vec::new(),
    }
}

/// `config.clientCertificates`, as Bruno reads it: `pem` and `pkcs12`
/// entries; others are skipped.
fn client_certificates(value: Option<&Value>) -> Vec<apifast_domain::ClientCertificate> {
    use apifast_domain::{ClientCertificate, ClientCertificateKind};
    items(value)
        .iter()
        .filter_map(|item| {
            let field = |key: &str| text_or_empty(get(item, key));
            let kind = match field("type").as_str() {
                "pem" => ClientCertificateKind::Pem {
                    certificate: field("certificateFilePath"),
                    private_key: field("privateKeyFilePath"),
                },
                "pkcs12" => ClientCertificateKind::Pkcs12 {
                    file: field("pkcs12FilePath"),
                },
                _ => return None,
            };
            Some(ClientCertificate {
                domain: field("domain"),
                kind,
                passphrase: field("passphrase"),
                enabled: !is_disabled(item),
            })
        })
        .collect()
}

/// `config.proxy`, as Bruno reads it: `disabled: true` turns it off,
/// otherwise `inherit` says whether the application's is used. What is
/// missing or odd means inherit.
fn proxy(value: Option<&Value>) -> apifast_domain::CollectionProxy {
    use apifast_domain::{CollectionProxy, ProxyConfig, ProxyMode, ProxyProtocol};
    let Some(value) = value.filter(|v| v.as_mapping().is_some()) else {
        return CollectionProxy::default();
    };
    let field = |path: &[&str]| text_or_empty(get_in(value, path));
    let mode = if boolean(get(value, "disabled")) == Some(true) {
        ProxyMode::Off
    } else if boolean(get(value, "inherit")) == Some(false) {
        ProxyMode::Custom
    } else {
        ProxyMode::Inherit
    };
    CollectionProxy {
        mode,
        config: ProxyConfig {
            protocol: ProxyProtocol::parse(&field(&["config", "protocol"])),
            hostname: field(&["config", "hostname"]),
            port: field(&["config", "port"]),
            auth: boolean(get_in(value, &["config", "auth", "disabled"])) != Some(true),
            username: field(&["config", "auth", "username"]),
            password: field(&["config", "auth", "password"]),
            bypass: field(&["config", "bypassProxy"]),
        },
    }
}

fn request_defaults(request: Option<&Value>, auth: Auth) -> RequestDefaults {
    let field = |key| request.and_then(|r| get(r, key));
    RequestDefaults {
        headers: headers(field("headers")),
        auth,
        variables: variables(field("variables")),
        // OpenCollection has no folder or collection settings.
        settings: Settings::default(),
    }
}

/// `environments/<name>.yml`. `file_stem` names an environment without a name.
pub(crate) fn environment(doc: &Value, file_stem: &str) -> Environment {
    Environment {
        name: text(get(doc, "name")).unwrap_or_else(|| file_stem.to_owned()),
        color: text(get(doc, "color")).filter(|c| !c.is_empty()),
        extends: text(get(doc, "extends")).filter(|e| !e.trim().is_empty()),
        variables: items(get(doc, "variables"))
            .iter()
            .map(|item| {
                if boolean(get(item, "secret")) == Some(true) {
                    Variable {
                        name: text_or_empty(get(item, "name")),
                        value: None,
                        enabled: !is_disabled(item),
                        secret: true,
                    }
                } else {
                    variable(item)
                }
            })
            .collect(),
    }
}

/// Patterns in `extensions.bruno.ignore` of `opencollection.yml`.
pub(crate) fn ignore_patterns(doc: &Value) -> Vec<String> {
    items(get_in(doc, &["extensions", "bruno", "ignore"]))
        .iter()
        .filter_map(|p| text(Some(p)))
        .collect()
}

/// The `dns` list of `.apifast/collection.yml`: hosts sent to fixed
/// addresses. An entry is enabled unless it says `enabled: false`.
pub(crate) fn dns_overrides(doc: &Value) -> Vec<apifast_domain::DnsOverride> {
    items(get(doc, "dns"))
        .iter()
        .map(|item| apifast_domain::DnsOverride {
            host: text_or_empty(get(item, "host")),
            address: text_or_empty(get(item, "address")),
            enabled: boolean(get(item, "enabled")).unwrap_or(true),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> Value {
        oneyaml::from_str(text).unwrap()
    }

    fn http(text: &str) -> HttpRequest {
        match item(&yaml(text)) {
            Item::Http(request) => request,
            other @ Item::Other { .. } => panic!("expected an HTTP request, got {other:?}"),
        }
    }

    #[test]
    fn reads_a_full_http_request() {
        let request = http(
            "info:\n  name: Create\n  type: http\n  seq: 2\n  tags: [smoke, 1]\n\
             http:\n  method: post\n  url: \"{{host}}/users/:id\"\n\
             \x20 params:\n    - {name: id, value: 42, type: path}\n    - {name: q, value: x, disabled: true}\n\
             \x20 headers:\n    - {name: accept, value: application/json}\n\
             \x20 body:\n    type: json\n    data: '{}'\n\
             \x20 auth:\n    type: bearer\n    token: \"{{token}}\"\n\
             runtime:\n  variables:\n    - {name: n, value: {type: number, data: '5'}}\n\
             settings:\n  timeout: 1000\n  followRedirects: false\n  maxRedirects: 3\n  encodeUrl: true\n\
             docs: Hello\n",
        );
        assert_eq!(request.name, "Create");
        assert_eq!(request.seq, Some(2));
        assert_eq!(request.tags, ["smoke", "1"]);
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.url, "{{host}}/users/:id");
        assert_eq!(
            request.params,
            [
                Param {
                    name: "id".into(),
                    value: "42".into(),
                    kind: ParamKind::Path,
                    enabled: true
                },
                Param {
                    name: "q".into(),
                    value: "x".into(),
                    kind: ParamKind::Query,
                    enabled: false
                },
            ]
        );
        assert_eq!(request.headers, [Header::new("accept", "application/json")]);
        assert_eq!(request.body, Body::Json("{}".into()));
        assert_eq!(
            request.auth,
            Setting::Set(AuthScheme::Bearer(BearerAuth {
                token: "{{token}}".into()
            }))
        );
        assert_eq!(
            request.variables[0].value,
            Some(VariableValue {
                kind: ValueKind::Number,
                data: "5".into()
            })
        );
        assert_eq!(request.settings.timeout_ms, Setting::Set(1000));
        assert_eq!(request.settings.follow_redirects, Setting::Set(false));
        assert_eq!(request.settings.max_redirects, Setting::Set(3));
        assert_eq!(request.docs.as_deref(), Some("Hello"));
    }

    #[test]
    fn missing_values_take_bruno_defaults() {
        let request = http("info:\n  type: http\n");
        assert_eq!(request.name, "Untitled Request");
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url, "");
        assert_eq!(request.body, Body::None);
        // No auth key means no auth, not inherited.
        assert_eq!(request.auth, Setting::Set(AuthScheme::None));
        assert_eq!(request.settings, Settings::default());
    }

    #[test]
    fn auth_modes_follow_bruno() {
        assert_eq!(auth(Some(&yaml("inherit"))), Setting::Inherit);
        assert_eq!(auth(Some(&yaml("{type: inherit}"))), Setting::Inherit);
        assert_eq!(auth(Some(&yaml("none"))), Setting::Set(AuthScheme::None));
        assert_eq!(auth(None), Setting::Set(AuthScheme::None));
        assert_eq!(
            auth(Some(&yaml("{type: basic, username: u}"))),
            Setting::Set(AuthScheme::Basic(BasicAuth {
                username: "u".into(),
                password: String::new()
            }))
        );
        assert_eq!(
            auth(Some(&yaml(
                "{type: apikey, key: k, value: v, placement: query}"
            ))),
            Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
                key: "k".into(),
                value: "v".into(),
                placement: ApiKeyPlacement::Query
            }))
        );
        assert_eq!(
            auth(Some(&yaml("{type: wsse, username: u}"))),
            Setting::Set(AuthScheme::Unsupported("wsse".into()))
        );
    }

    #[test]
    fn legacy_runtime_auth_is_read() {
        let request = http("info: {type: http}\nhttp: {url: x}\nruntime:\n  auth: inherit\n");
        assert_eq!(request.auth, Setting::Inherit);
    }

    #[test]
    fn timeout_follows_bruno() {
        let timeout = |s: &str| {
            http(&format!("info: {{type: http}}\nsettings:\n{s}"))
                .settings
                .timeout_ms
        };
        assert_eq!(timeout("  timeout: 0\n"), Setting::Set(0));
        assert_eq!(timeout("  timeout: inherit\n"), Setting::Inherit);
        assert_eq!(timeout("  encodeUrl: true\n"), Setting::Set(0));
    }

    #[test]
    fn reads_every_body_kind() {
        let body = |s: &str| http(&format!("info: {{type: http}}\nhttp:\n  body:\n{s}")).body;
        assert_eq!(
            body("    type: xml\n    data: <a/>\n"),
            Body::Xml("<a/>".into())
        );
        assert_eq!(
            body(
                "    type: form-urlencoded\n    data:\n      - {name: a, value: 1}\n      - {name: b, value: 2, disabled: true}\n"
            ),
            Body::FormUrlEncoded(vec![
                FormField {
                    name: "a".into(),
                    value: "1".into(),
                    enabled: true
                },
                FormField {
                    name: "b".into(),
                    value: "2".into(),
                    enabled: false
                },
            ])
        );
        assert_eq!(
            body(
                "    type: multipart-form\n    data:\n      - {name: t, type: text, value: hi}\n      - {name: f, type: file, value: [a.png, b.png], contentType: image/png}\n"
            ),
            Body::Multipart(vec![
                MultipartPart {
                    name: "t".into(),
                    value: MultipartValue::Text("hi".into()),
                    content_type: None,
                    enabled: true
                },
                MultipartPart {
                    name: "f".into(),
                    value: MultipartValue::Files(vec!["a.png".into(), "b.png".into()]),
                    content_type: Some("image/png".into()),
                    enabled: true
                },
            ])
        );
        assert_eq!(
            body(
                "    type: file\n    data:\n      - {filePath: a.bin, contentType: '', selected: false}\n      - {filePath: b.bin, contentType: application/pdf, selected: true}\n"
            ),
            Body::File {
                path: "b.bin".into(),
                content_type: Some("application/pdf".into())
            }
        );
        assert_eq!(body("    type: grpc\n    data: x\n"), Body::None);
        assert_eq!(
            body("    type: graphql\n    query: q\n"),
            Body::GraphQl {
                query: "q".into(),
                variables: String::new()
            }
        );
    }

    #[test]
    fn graphql_requests_are_http_with_their_query_as_the_body() {
        let Item::Http(request) = item(&yaml(
            "info: {name: Q, type: graphql, seq: 3}\ngraphql:\n  url: https://h/graphql\n  body:\n    query: '{ users { id } }'\n    variables: '{\"a\": 1}'\n",
        )) else {
            panic!("not read as a request");
        };
        assert_eq!(request.method, Method::Post, "Bruno's default");
        assert_eq!(request.url, "https://h/graphql");
        assert_eq!(
            request.body,
            Body::GraphQl {
                query: "{ users { id } }".into(),
                variables: "{\"a\": 1}".into()
            }
        );
    }

    #[test]
    fn websocket_requests_have_their_messages_as_the_body() {
        let Item::Http(request) = item(&yaml(
            "info: {name: W, type: websocket, seq: 2}\nwebsocket:\n  url: ws://h/ws?room=1\n  headers: [{name: X-A, value: b}]\n  message:\n    type: json\n    data: '{\"a\": 1}'\n  auth: inherit\n",
        )) else {
            panic!("not read as a request");
        };
        assert_eq!(request.url, "ws://h/ws?room=1");
        assert_eq!(request.params.len(), 1, "from the URL");
        assert_eq!(request.headers.len(), 1);
        assert_eq!(request.auth, Auth::Inherit);
        assert_eq!(
            request.body,
            Body::WebSocket(vec![WsMessage::new("", WsMessageKind::Json, "{\"a\": 1}")])
        );

        let Item::Http(request) = item(&yaml(
            "info: {name: W, type: websocket}\nwebsocket:\n  url: ws://h\n  message:\n    - title: hi\n      selected: true\n      message: {type: text, data: hello}\n    - message: {type: xml, data: <a/>}\n",
        )) else {
            panic!("not read as a request");
        };
        let Body::WebSocket(messages) = request.body else {
            panic!("{:?}", request.body);
        };
        assert_eq!(
            messages,
            vec![
                WsMessage {
                    selected: true,
                    ..WsMessage::new("hi", WsMessageKind::Text, "hello")
                },
                WsMessage::new("message 2", WsMessageKind::Xml, "<a/>"),
            ]
        );
    }

    #[test]
    fn grpc_requests_have_their_call_as_the_body_and_metadata_as_headers() {
        let request = http(
            "info: {name: G, type: grpc, seq: 4}\ngrpc:\n  url: \"{{host}}\"\n  method: /hello.HelloService/SayHello\n  methodType: unary\n  protoFilePath: ../hello.proto\n  metadata:\n    - name: x-team\n      value: core\n    - name: x-off\n      value: \"1\"\n      disabled: true\n  message:\n    - title: hi\n      message: '{\"greeting\": \"hi\"}'\n    - message: '{}'\n  auth: inherit\n",
        );
        assert_eq!(request.url, "{{host}}");
        assert_eq!(request.headers[0], Header::new("x-team", "core"));
        assert!(!request.headers[1].enabled);
        assert_eq!(request.auth, Setting::Inherit);
        assert_eq!(
            request.body,
            Body::Grpc(GrpcCall {
                method: "/hello.HelloService/SayHello".into(),
                method_type: Some(GrpcMethodType::Unary),
                proto_path: Some("../hello.proto".into()),
                messages: vec![
                    GrpcMessage::new("hi", "{\"greeting\": \"hi\"}"),
                    GrpcMessage::new("message 2", "{}"),
                ],
            })
        );
        // A single plain message, as Bruno also reads it; an unknown type.
        let request = http(
            "info: {name: G, type: grpc}\ngrpc:\n  url: x\n  method: /a.B/C\n  methodType: other\n  message: '{}'\n",
        );
        let Body::Grpc(call) = request.body else {
            panic!("{:?}", request.body);
        };
        assert_eq!(call.method_type, None);
        assert_eq!(call.messages, [GrpcMessage::new("", "{}")]);
    }

    #[test]
    fn other_request_kinds_are_listed_not_parsed() {
        assert_eq!(
            item(&yaml(
                "info: {name: Q, type: soap, seq: 3}\nsoap: {url: x}\n"
            )),
            Item::Other {
                name: "Q".into(),
                kind: "soap".into(),
                seq: Some(3)
            }
        );
    }

    #[test]
    fn reads_folders_collections_and_environments() {
        let f = folder(
            &yaml(
                "info: {name: Users, type: folder, seq: 1}\nrequest:\n  headers: [{name: x, value: y}]\n  auth: inherit\n",
            ),
            "dir",
        );
        assert_eq!(f.name, "Users");
        assert_eq!(f.seq, Some(1));
        assert_eq!(f.defaults.auth, Setting::Inherit);
        assert_eq!(f.defaults.headers, [Header::new("x", "y")]);
        // A folder.yml without auth turns the collection's auth off, as in Bruno.
        assert_eq!(
            folder(&yaml("info: {type: folder}\n"), "dir").defaults.auth,
            Setting::Set(AuthScheme::None)
        );
        assert_eq!(folder(&yaml("info: {type: folder}\n"), "dir").name, "dir");

        let c = collection(&yaml(
            "opencollection: 1.0.0\ninfo: {name: API}\nextensions:\n  bruno:\n    ignore: [node_modules, .apifast]\n",
        ));
        assert_eq!(c.name, "API");
        assert_eq!(c.defaults.auth, Setting::Inherit);
        assert_eq!(
            ignore_patterns(&yaml("extensions: {bruno: {ignore: [a, b]}}")),
            ["a", "b"]
        );

        let e = environment(
            &yaml(
                "name: Prod\ncolor: '#f00'\nextends: base\nvariables:\n  - {name: host, value: h}\n  - {name: key, secret: true}\n",
            ),
            "prod",
        );
        assert_eq!(e.name, "Prod");
        assert_eq!(e.color.as_deref(), Some("#f00"));
        assert_eq!(e.extends.as_deref(), Some("base"));
        assert_eq!(
            e.variables,
            [Variable::text("host", "h"), Variable::secret("key")]
        );
    }
}
