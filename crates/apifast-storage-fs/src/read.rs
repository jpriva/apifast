//! YAML → domain, following how Bruno reads OpenCollection files
//! (`bruno-filestore/src/formats/yml`): missing values take Bruno's defaults
//! instead of failing.

use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, Assertion, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth,
    Body, Capture, CaptureScope, Collection, Environment, Folder, FormField, Header, HttpRequest,
    Method, MultipartPart, MultipartValue, Operator, Param, ParamKind, RequestDefaults, Setting,
    Settings, ValueKind, Variable, VariableValue,
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
        Some("http") => Item::Http(http_request(doc, name, seq)),
        // Without a type, a file with an `http` block is still an HTTP request.
        None if get(doc, "http").is_some() => Item::Http(http_request(doc, name, seq)),
        _ => Item::Other {
            name,
            kind: kind.unwrap_or_else(|| "unknown".into()),
            seq,
        },
    }
}

fn http_request(doc: &Value, name: String, seq: Option<u32>) -> HttpRequest {
    let http = get(doc, "http");
    let field = |key| http.and_then(|h| get(h, key));
    // Files from OpenCollection v3 rc1 keep the auth in `runtime.auth`.
    let auth = field("auth").or_else(|| get_in(doc, &["runtime", "auth"]));

    HttpRequest {
        name,
        seq,
        tags: items(get_in(doc, &["info", "tags"]))
            .iter()
            .filter_map(|t| text(Some(t)))
            .collect(),
        method: Method::parse(&text(field("method")).unwrap_or_else(|| "GET".into())),
        url: text_or_empty(field("url")),
        params: items(field("params")).iter().map(param).collect(),
        headers: headers(field("headers")),
        body: body(field("body")),
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

fn param(item: &Value) -> Param {
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

fn body(value: Option<&Value>) -> Body {
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
            auth(Some(&yaml("{type: oauth2, flow: client_credentials}"))),
            Setting::Set(AuthScheme::Unsupported("oauth2".into()))
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
        assert_eq!(body("    type: graphql\n    data: x\n"), Body::None);
    }

    #[test]
    fn other_request_kinds_are_listed_not_parsed() {
        assert_eq!(
            item(&yaml(
                "info: {name: Q, type: graphql, seq: 3}\ngraphql: {url: x}\n"
            )),
            Item::Other {
                name: "Q".into(),
                kind: "graphql".into(),
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
