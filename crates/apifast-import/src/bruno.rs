//! A Bruno collection in the `.bru` format (Bruno before YAML) as an
//! apifast collection: `bruno.json` gives its name, `collection.bru` what
//! every request inherits, each folder's `folder.bru` its name and
//! defaults, each other `.bru` file a request, and `environments/*.bru`
//! the environments. Scripts and tests keep their code: it is Bruno's API
//! and are listed, as is any file that cannot be read.

mod parse;

use std::path::Path;

use apifast_app::{ImportWarning, ImportedCollection, ImportedItem};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, Assertion, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth,
    Body, Capture, CaptureScope, CredentialsPlacement, DigestAuth, Environment, Folder, FormField,
    Header, HttpRequest, Method, MultipartPart, MultipartValue, NtlmAuth, OAuth2Auth, OAuth2Flow,
    Operator, Param, ParamKind, RequestDefaults, ScriptBlock, ScriptKind, Setting, TokenPlacement,
    ValueKind, Variable, VariableValue, sync_params_from_url,
};

use parse::{BruFile, Pair, parse};

/// The blocks of a request's method, `http` taking it as `method`.
const METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "options", "head", "connect", "trace", "http",
];

/// Why a folder is not a Bruno `.bru` collection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BrunoError {
    /// It has no `bruno.json`.
    #[error("`{0}` has no bruno.json: not a Bruno collection")]
    NotCollection(String),
    /// A file or folder cannot be read.
    #[error("cannot read `{0}`: {1}")]
    Unreadable(String, String),
}

/// The Bruno collection in the folder `dir`.
///
/// # Errors
///
/// If it has no `bruno.json`, or a folder cannot be listed. A `.bru` file
/// that cannot be read is a warning, and left out.
pub fn import_bruno(dir: &Path) -> Result<ImportedCollection, BrunoError> {
    let config = std::fs::read_to_string(dir.join("bruno.json"))
        .map_err(|_| BrunoError::NotCollection(dir.display().to_string()))?;
    let config: serde_json::Value = serde_json::from_str(&config)
        .map_err(|e| BrunoError::Unreadable("bruno.json".into(), e.to_string()))?;
    let name = config
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("Bruno collection")
        .to_owned();
    let mut warnings = Vec::new();
    let defaults = match read_bru(&dir.join("collection.bru"), "collection.bru", &mut warnings) {
        Some(file) => defaults(&file, "", &mut warnings),
        None => RequestDefaults::default(),
    };
    let items = folder_items(dir, (&[], ""), &mut warnings)?;
    let environments = environments(&dir.join("environments"), &mut warnings);
    Ok(ImportedCollection {
        name,
        defaults,
        items,
        environments,
        warnings,
    })
}

/// The `.bru` file at `path`, or `None` if it is missing or cannot be read
/// (with a warning).
fn read_bru(path: &Path, at: &str, warnings: &mut Vec<ImportWarning>) -> Option<BruFile> {
    let text = std::fs::read_to_string(path).ok()?;
    match parse(&text) {
        Ok(file) => Some(file),
        Err(e) => {
            warnings.push(ImportWarning::new(
                at,
                format!("cannot be read (line {}: {}): left out", e.line, e.message),
            ));
            None
        }
    }
}

/// The requests and folders in `dir`, by their `seq` and then their name.
/// `rel` is `dir` relative to the collection, `/`-separated (empty for
/// its root): with a file's name, its key across imports.
fn folder_items(
    dir: &Path,
    (path, rel): (&[String], &str),
    warnings: &mut Vec<ImportWarning>,
) -> Result<Vec<ImportedItem>, BrunoError> {
    let listing = std::fs::read_dir(dir)
        .map_err(|e| BrunoError::Unreadable(dir.display().to_string(), e.to_string()))?;
    let mut entries: Vec<_> = listing.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut items: Vec<(Option<u32>, String, ImportedItem)> = Vec::new();
    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let entry_path = entry.path();
        if file_name.starts_with('.') || file_name == "node_modules" {
            continue;
        }
        if entry_path.is_dir() {
            if path.is_empty() && file_name == "environments" {
                continue;
            }
            let folder_file = read_bru(&entry_path.join("folder.bru"), &file_name, warnings);
            let name = (folder_file.as_ref().and_then(|f| f.get("meta", "name")))
                .unwrap_or(&file_name)
                .to_owned();
            let mut here = path.to_vec();
            here.push(name.clone());
            let at = here.join(" / ");
            let mut folder = Folder::new(name.clone());
            if let Some(file) = &folder_file {
                folder.defaults = defaults(file, &at, warnings);
            }
            let seq = folder_file
                .as_ref()
                .and_then(|f| f.get("meta", "seq")?.parse().ok());
            let child = if rel.is_empty() {
                file_name.clone()
            } else {
                format!("{rel}/{file_name}")
            };
            let children = folder_items(&entry_path, (&here, &child), warnings)?;
            items.push((
                seq,
                name,
                ImportedItem::Folder {
                    folder,
                    items: children,
                },
            ));
            continue;
        }
        let is_request = std::path::Path::new(&file_name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bru"))
            && !matches!(file_name.as_str(), "collection.bru" | "folder.bru");
        if !is_request {
            continue;
        }
        let stem = file_name.trim_end_matches(".bru").to_owned();
        let mut here = path.to_vec();
        here.push(stem.clone());
        let Some(file) = read_bru(&entry_path, &here.join(" / "), warnings) else {
            continue;
        };
        let name = file.get("meta", "name").unwrap_or(&stem).to_owned();
        here.pop();
        here.push(name.clone());
        let at = here.join(" / ");
        if let Some(kind) = file
            .get("meta", "type")
            .filter(|t| !matches!(*t, "http" | "graphql"))
        {
            warnings.push(ImportWarning::new(
                at,
                format!("`{kind}` requests are not supported yet: left out"),
            ));
            continue;
        }
        let seq = file.get("meta", "seq").and_then(|s| s.parse().ok());
        let request = request(&file, name.clone(), &at, warnings);
        let key = if rel.is_empty() {
            file_name.clone()
        } else {
            format!("{rel}/{file_name}")
        };
        let item = ImportedItem::Request {
            request: Box::new(request),
            key: Some(key),
        };
        items.push((seq, name, item));
    }
    // Bruno orders by `seq`; without one, after those with one.
    items.sort_by(|a, b| (a.0.unwrap_or(u32::MAX), &a.1).cmp(&(b.0.unwrap_or(u32::MAX), &b.1)));
    Ok(items.into_iter().map(|(_, _, item)| item).collect())
}

/// What a collection or folder file gives its requests.
fn defaults(file: &BruFile, at: &str, warnings: &mut Vec<ImportWarning>) -> RequestDefaults {
    let mode = file.get("auth", "mode").unwrap_or("inherit");
    RequestDefaults {
        headers: headers(file.pairs("headers")),
        auth: auth(file, mode, at, warnings),
        variables: variables(file.pairs("vars:pre-request")),
        scripts: scripts(file),
        ..RequestDefaults::default()
    }
}

fn request(
    file: &BruFile,
    name: String,
    at: &str,
    warnings: &mut Vec<ImportWarning>,
) -> HttpRequest {
    let (block, http) = METHODS
        .iter()
        .find_map(|m| file.block(m).map(|_| (*m, file.pairs(m))))
        .unwrap_or(("get", &[]));
    let field = |key: &str| {
        (http.iter())
            .find(|p| p.key == key)
            .map_or("", |p| p.value.as_str())
    };
    let method = if block == "http" {
        field("method")
    } else {
        block
    };
    let mut request = HttpRequest::new(
        name,
        Method::parse(&method.to_ascii_uppercase()),
        field("url"),
    );
    request.headers = headers(file.pairs("headers"));
    request.body = body(file, field("body"), at, warnings);
    request.auth = auth(
        file,
        if field("auth").is_empty() {
            "inherit"
        } else {
            field("auth")
        },
        at,
        warnings,
    );
    request.variables = variables(file.pairs("vars:pre-request"));
    request.scripts = scripts(file);
    request.captures = (file.pairs("vars:post-response").iter())
        .map(|p| Capture {
            enabled: p.enabled,
            scope: CaptureScope::Runtime,
            ..Capture::new(p.value.clone(), p.key.trim_start_matches('@'))
        })
        .collect();
    request.assertions = file.pairs("assert").iter().map(assertion).collect();
    request.docs = file
        .text("docs")
        .map(str::to_owned)
        .filter(|d| !d.trim().is_empty());
    request.tags = (file.get("meta", "tags").into_iter())
        .flat_map(|t| t.split(','))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    settings(file, &mut request);
    // Path parameters keep their values; disabled query ones are kept off.
    for pair in file.pairs("params:path") {
        request.params.push(Param {
            name: pair.key.clone(),
            value: pair.value.clone(),
            kind: ParamKind::Path,
            enabled: pair.enabled,
        });
    }
    for pair in file.pairs("params:query").iter().filter(|p| !p.enabled) {
        request.params.push(Param {
            name: pair.key.clone(),
            value: pair.value.clone(),
            kind: ParamKind::Query,
            enabled: false,
        });
    }
    sync_params_from_url(&mut request);
    request
}

fn settings(file: &BruFile, request: &mut HttpRequest) {
    let get = |key: &str| file.get("settings", key);
    let flag = |key: &str| get(key).and_then(|v| v.parse::<bool>().ok());
    if let Some(encode) = flag("encodeUrl") {
        request.settings.encode_url = Setting::Set(encode);
    }
    if let Some(follow) = flag("followRedirects") {
        request.settings.follow_redirects = Setting::Set(follow);
    }
    if let Some(max) = get("maxRedirects").and_then(|v| v.parse().ok()) {
        request.settings.max_redirects = Setting::Set(max);
    }
    // 0 in a `.bru` file means "not set": it inherits.
    if let Some(timeout) = get("timeout")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|t| *t > 0)
    {
        request.settings.timeout_ms = Setting::Set(timeout);
    }
}

fn headers(pairs: &[Pair]) -> Vec<Header> {
    pairs
        .iter()
        .map(|p| Header {
            enabled: p.enabled,
            ..Header::new(p.key.clone(), p.value.clone())
        })
        .collect()
}

fn variables(pairs: &[Pair]) -> Vec<Variable> {
    pairs
        .iter()
        .map(|p| Variable {
            name: p.key.trim_start_matches('@').to_owned(),
            value: Some(VariableValue {
                kind: match p.kind.as_deref() {
                    Some("number") => ValueKind::Number,
                    Some("boolean") => ValueKind::Bool,
                    Some("object") => ValueKind::Object,
                    _ => ValueKind::String,
                },
                data: p.value.clone(),
            }),
            enabled: p.enabled,
            secret: false,
        })
        .collect()
}

/// `res.status: eq 200` as an assertion.
fn assertion(pair: &Pair) -> Assertion {
    let (operator, value) = pair
        .value
        .trim()
        .split_once(' ')
        .unwrap_or((pair.value.trim(), ""));
    let operator = Operator::parse(operator);
    let value = (operator.takes_value() || !value.is_empty()).then(|| value.trim().to_owned());
    Assertion {
        enabled: pair.enabled,
        ..Assertion::new(pair.key.clone(), operator, value.as_deref())
    }
}

fn body(file: &BruFile, mode: &str, at: &str, warnings: &mut Vec<ImportWarning>) -> Body {
    let text = |name: &str| file.text(name).unwrap_or_default().to_owned();
    match mode {
        "" | "none" => Body::None,
        "json" => Body::Json(text("body:json")),
        "text" => Body::Text(text("body:text")),
        "xml" => Body::Xml(text("body:xml")),
        "sparql" => Body::Sparql(text("body:sparql")),
        "formUrlEncoded" => Body::FormUrlEncoded(
            (file.pairs("body:form-urlencoded").iter())
                .map(|p| FormField {
                    name: p.key.clone(),
                    value: p.value.clone(),
                    enabled: p.enabled,
                })
                .collect(),
        ),
        "multipartForm" => {
            Body::Multipart(file.pairs("body:multipart-form").iter().map(part).collect())
        }
        "file" => {
            let chosen = (file.pairs("body:file").iter())
                .find(|p| p.enabled)
                .or_else(|| file.pairs("body:file").first());
            match chosen {
                Some(pair) => {
                    let (value, content_type) = content_type(&pair.value);
                    Body::File {
                        path: file_paths(&value).into_iter().next().unwrap_or_default(),
                        content_type,
                    }
                }
                None => Body::None,
            }
        }
        "graphql" => Body::GraphQl {
            query: text("body:graphql"),
            variables: text("body:graphql:vars"),
        },
        other => {
            warnings.push(ImportWarning::new(
                at,
                format!("a `{other}` body is not supported"),
            ));
            Body::None
        }
    }
}

/// A multipart pair: `@file(a|b)` is files; `@contentType(…)` its type.
fn part(pair: &Pair) -> MultipartPart {
    let (value, content_type) = content_type(&pair.value);
    let files = file_paths(&value);
    MultipartPart {
        name: pair.key.clone(),
        value: if value.starts_with("@file(") {
            MultipartValue::Files(files)
        } else {
            MultipartValue::Text(value)
        },
        content_type,
        enabled: pair.enabled,
    }
}

/// `value @contentType(type)` as the value and the type.
fn content_type(value: &str) -> (String, Option<String>) {
    match value.rsplit_once("@contentType(") {
        Some((value, rest)) => {
            let kind = rest.trim_end().trim_end_matches(')').trim().to_owned();
            (
                value.trim().to_owned(),
                Some(kind).filter(|k| !k.is_empty()),
            )
        }
        None => (value.to_owned(), None),
    }
}

/// The paths of `@file(a|b)`.
fn file_paths(value: &str) -> Vec<String> {
    (value
        .strip_prefix("@file(")
        .and_then(|v| v.strip_suffix(')'))
        .into_iter())
    .flat_map(|paths| paths.split('|'))
    .filter(|p| !p.is_empty())
    .map(str::to_owned)
    .collect()
}

fn auth(file: &BruFile, mode: &str, at: &str, warnings: &mut Vec<ImportWarning>) -> Auth {
    let get = |block: &str, key: &str| file.get(block, key).unwrap_or_default().to_owned();
    let scheme = match mode {
        "inherit" => return Setting::Inherit,
        "none" => AuthScheme::None,
        "basic" => AuthScheme::Basic(BasicAuth {
            username: get("auth:basic", "username"),
            password: get("auth:basic", "password"),
        }),
        "bearer" => AuthScheme::Bearer(BearerAuth {
            token: get("auth:bearer", "token"),
        }),
        "digest" => AuthScheme::Digest(DigestAuth {
            username: get("auth:digest", "username"),
            password: get("auth:digest", "password"),
        }),
        "ntlm" => AuthScheme::Ntlm(NtlmAuth {
            username: get("auth:ntlm", "username"),
            password: get("auth:ntlm", "password"),
            domain: get("auth:ntlm", "domain"),
        }),
        "apikey" => AuthScheme::ApiKey(ApiKeyAuth {
            key: get("auth:apikey", "key"),
            value: get("auth:apikey", "value"),
            placement: if get("auth:apikey", "placement") == "queryparams" {
                ApiKeyPlacement::Query
            } else {
                ApiKeyPlacement::Header
            },
        }),
        "awsv4" => AuthScheme::AwsV4(Box::new(AwsV4Auth {
            access_key_id: get("auth:awsv4", "accessKeyId"),
            secret_access_key: get("auth:awsv4", "secretAccessKey"),
            session_token: get("auth:awsv4", "sessionToken"),
            service: get("auth:awsv4", "service"),
            region: get("auth:awsv4", "region"),
            profile_name: get("auth:awsv4", "profileName"),
        })),
        "oauth2" => match oauth2(&|key| get("auth:oauth2", key)) {
            Ok(oauth) => AuthScheme::OAuth2(Box::new(oauth)),
            Err(why) => {
                warnings.push(ImportWarning::new(at, why));
                AuthScheme::None
            }
        },
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

fn oauth2(get: &dyn Fn(&str) -> String) -> Result<OAuth2Auth, String> {
    let flow = match get("grant_type").as_str() {
        "client_credentials" | "" => OAuth2Flow::ClientCredentials,
        "password" => OAuth2Flow::Password,
        "authorization_code" => OAuth2Flow::AuthorizationCode,
        "implicit" => OAuth2Flow::Implicit,
        other => return Err(format!("OAuth 2.0 grant `{other}` is not supported")),
    };
    let or = |key: &str, default: &str| {
        Some(get(key))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_owned())
    };
    let defaults = OAuth2Auth::default();
    Ok(OAuth2Auth {
        flow,
        authorization_url: get("authorization_url"),
        access_token_url: get("access_token_url"),
        refresh_token_url: get("refresh_token_url"),
        callback_url: get("callback_url"),
        client_id: get("client_id"),
        client_secret: get("client_secret"),
        credentials_placement: if get("credentials_placement") == "basic_auth_header" {
            CredentialsPlacement::BasicAuthHeader
        } else {
            CredentialsPlacement::Body
        },
        username: get("username"),
        password: get("password"),
        scope: get("scope"),
        state: get("state"),
        pkce: get("pkce") == "true",
        token_id: or("credentials_id", &defaults.token_id),
        token_placement: if get("token_placement") == "url" {
            TokenPlacement::Query(or("token_query_key", "access_token"))
        } else {
            TokenPlacement::Header(get("token_header_prefix"))
        },
        token_source: or("token_source", &defaults.token_source),
        auto_fetch: get("auto_fetch_token") != "false",
        auto_refresh: get("auto_refresh_token") == "true",
        ..defaults
    })
}

/// The scripts and the tests block, already in Bruno's API.
fn scripts(file: &BruFile) -> Vec<ScriptBlock> {
    [
        ("script:pre-request", ScriptKind::BeforeRequest),
        ("script:post-response", ScriptKind::AfterResponse),
        ("tests", ScriptKind::Tests),
    ]
    .into_iter()
    .filter_map(|(name, kind)| {
        let code = file.text(name).filter(|code| !code.trim().is_empty())?;
        Some(ScriptBlock::new(kind, code))
    })
    .collect()
}

/// The environments in `dir`: `vars` with their values; `vars:secret`
/// declared secret without one, as Bruno keeps their values apart.
fn environments(dir: &Path, warnings: &mut Vec<ImportWarning>) -> Vec<Environment> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = listing.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    let mut out = Vec::new();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "bru"))
    {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let at = format!("environments / {name}");
        let Some(file) = read_bru(path, &at, warnings) else {
            continue;
        };
        let mut variables = variables(file.pairs("vars"));
        if let Some(parse::Block::List(secrets)) = file.block("vars:secret") {
            for secret in secrets {
                let (secret, enabled) = match secret.strip_prefix('~') {
                    Some(name) => (name, false),
                    None => (secret.as_str(), true),
                };
                variables.push(Variable {
                    enabled,
                    ..Variable::secret(secret)
                });
            }
            if !secrets.is_empty() {
                warnings.push(ImportWarning::new(
                    at,
                    format!(
                        "{} secrets: Bruno keeps their values apart; set them in the vault",
                        secrets.len()
                    ),
                ));
            }
        }
        out.push(Environment::new(name, variables));
    }
    out
}

#[cfg(test)]
mod tests;
