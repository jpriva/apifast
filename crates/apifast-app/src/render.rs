//! From a [`ResolvedRequest`] (inheritance applied, templates unrendered) to a
//! [`RenderedRequest`] (ready to send).

use std::fmt;

use apifast_domain::{
    ApiKeyPlacement, AuthScheme, Body, Method, MultipartValue, ParamKind, ResolvedRequest,
    ResolvedVariables, Secret, mask,
};
use apifast_template::{Host, Mode, Problem, render};

use crate::AppError;

/// A request ready to send: inheritance applied, templates rendered, auth
/// turned into headers or query parameters.
///
/// It can only be built by [`render_request`] from a [`ResolvedRequest`], so
/// the transport never receives a request that skipped either step.
///
/// Building one by hand must not compile (see the same check on
/// [`ResolvedRequest`]; keep the example in sync with the fields):
///
/// ```compile_fail,E0451
/// use apifast_app::{RenderedBody, RenderedRequest};
/// use apifast_domain::Method;
///
/// let forged = RenderedRequest {
///     method: Method::Get,
///     url: String::new(),
///     headers: Vec::new(),
///     body: RenderedBody::None,
///     timeout_ms: 0,
///     follow_redirects: true,
///     max_redirects: 0,
///     encode_url: true,
///     contains_secret: false,
/// };
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct RenderedRequest {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: RenderedBody,
    timeout_ms: u64,
    follow_redirects: bool,
    max_redirects: u32,
    encode_url: bool,
    auth: TransportAuth,
    contains_secret: bool,
    secrets: Vec<Secret>,
}

/// An auth scheme that only the transport can apply, because it depends on
/// the request as it goes out (the encoded URL, the final body) or on the
/// server's answer. Its fields are rendered.
#[derive(Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportAuth {
    /// Nothing to do, or auth already in the headers.
    #[default]
    None,
    /// Sign with AWS Signature Version 4.
    AwsV4(AwsV4Credentials),
    /// Answer an HTTP Digest challenge.
    Digest {
        /// User name.
        username: String,
        /// Password.
        password: String,
    },
    /// Authenticate the connection with NTLM.
    Ntlm {
        /// User name, without the domain.
        username: String,
        /// Password.
        password: String,
        /// Windows domain; may be empty.
        domain: String,
    },
}

/// Rendered AWS credentials and the scope of the signature.
#[derive(Clone, PartialEq, Eq)]
pub struct AwsV4Credentials {
    /// Access key id.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
    /// Session token; empty for none.
    pub session_token: String,
    /// Service, such as `s3`.
    pub service: String,
    /// Region, such as `us-east-1`.
    pub region: String,
}

/// Never shows the secret key or the session token, even when they were
/// typed in the file rather than taken from a secret variable.
impl fmt::Debug for TransportAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::AwsV4(c) => f
                .debug_struct("AwsV4")
                .field("access_key_id", &c.access_key_id)
                .field("service", &c.service)
                .field("region", &c.region)
                .finish_non_exhaustive(),
            Self::Digest { username, .. } => f
                .debug_struct("Digest")
                .field("username", username)
                .finish_non_exhaustive(),
            Self::Ntlm {
                username, domain, ..
            } => f
                .debug_struct("Ntlm")
                .field("username", username)
                .field("domain", domain)
                .finish_non_exhaustive(),
        }
    }
}

/// Printed with every secret value masked, so it never reaches a log.
impl fmt::Debug for RenderedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(dead_code, reason = "only printed")]
        struct RenderedRequest<'a> {
            method: &'a Method,
            url: &'a str,
            headers: &'a [(String, String)],
            body: &'a RenderedBody,
            timeout_ms: u64,
            follow_redirects: bool,
            max_redirects: u32,
            encode_url: bool,
            auth: &'a TransportAuth,
            contains_secret: bool,
        }
        let shown = format!(
            "{:?}",
            RenderedRequest {
                method: &self.method,
                url: &self.url,
                headers: &self.headers,
                body: &self.body,
                timeout_ms: self.timeout_ms,
                follow_redirects: self.follow_redirects,
                max_redirects: self.max_redirects,
                encode_url: self.encode_url,
                auth: &self.auth,
                contains_secret: self.contains_secret,
            }
        );
        f.write_str(&mask(&shown, &self.secrets))
    }
}

impl RenderedRequest {
    /// Method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Final URL, with path parameters substituted and any API key in the query.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Headers to send, including the ones auth adds.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Body.
    pub fn body(&self) -> &RenderedBody {
        &self.body
    }

    /// Timeout in milliseconds; 0 means no limit.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Whether redirects are followed.
    pub fn follow_redirects(&self) -> bool {
        self.follow_redirects
    }

    /// Maximum number of redirects followed.
    pub fn max_redirects(&self) -> u32 {
        self.max_redirects
    }

    /// Whether the URL is encoded before sending.
    pub fn encode_url(&self) -> bool {
        self.encode_url
    }

    /// An auth the transport applies as it sends.
    pub fn auth(&self) -> &TransportAuth {
        &self.auth
    }

    /// Whether any secret went into the request. If so, the timeline, history
    /// and logs must mask it.
    pub fn contains_secret(&self) -> bool {
        self.contains_secret
    }

    /// The values of the secrets that went into the request, to mask them
    /// wherever it is shown or stored.
    pub fn secrets(&self) -> &[Secret] {
        &self.secrets
    }
}

/// Body of a [`RenderedRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderedBody {
    /// No body.
    None,
    /// Text body (JSON, text, XML, SPARQL).
    Text(String),
    /// `application/x-www-form-urlencoded` fields, not yet encoded.
    Form(Vec<(String, String)>),
    /// `multipart/form-data` parts. The transport chooses the boundary.
    Multipart(Vec<RenderedPart>),
    /// A file to send as is, by its path relative to the collection.
    File(String),
}

/// A `multipart` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPart {
    /// Part name.
    pub name: String,
    /// Text or file path.
    pub value: MultipartValue,
    /// `Content-Type` of the part, if given.
    pub content_type: Option<String>,
}

/// Result of [`render_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutcome {
    /// The request.
    pub request: RenderedRequest,
    /// Problems found in [`Mode::Lenient`]; always empty in [`Mode::Strict`].
    pub problems: Vec<Problem>,
}

/// Renders every template of `resolved` and applies its auth.
///
/// - Query parameters come from the URL, as in Bruno: the `params` list only
///   mirrors them for the UI. Path parameters replace `:name` path segments.
/// - A `Content-Type` is added for text and form bodies when no header sets one.
/// - Auth replaces any `Authorization` header (or the API key header) from
///   the request.
///
/// # Errors
///
/// [`AppError::Render`] with the first template problem in [`Mode::Strict`],
/// and [`AppError::Validation`] for an auth scheme or body kind this version
/// cannot send yet (rather than silently sending without it).
pub fn render_request(
    resolved: &ResolvedRequest,
    variables: &ResolvedVariables,
    host: &mut dyn Host,
    mode: Mode,
) -> Result<RenderOutcome, AppError> {
    let mut r = Renderer {
        variables,
        host,
        mode,
        contains_secret: false,
        secrets: Vec::new(),
        problems: Vec::new(),
    };

    let url = r.text(resolved.url())?;
    let mut path_params = Vec::new();
    for param in resolved
        .params()
        .iter()
        .filter(|p| p.kind == ParamKind::Path)
    {
        path_params.push((param.name.clone(), r.text(&param.value)?));
    }
    let mut url = substitute_path_params(&url, &path_params);

    let mut headers = Vec::with_capacity(resolved.headers().len() + 1);
    for header in resolved.headers() {
        headers.push((r.text(&header.value.name)?, r.text(&header.value.value)?));
    }

    let (body, content_type) = r.body(resolved.body())?;
    if let Some(content_type) = content_type
        && !has_header(&headers, "content-type")
    {
        headers.push(("Content-Type".into(), content_type));
    }

    let auth = apply_auth(&mut r, &resolved.auth().value, &mut headers, &mut url)?;

    let settings = resolved.settings();
    Ok(RenderOutcome {
        request: RenderedRequest {
            method: resolved.method().clone(),
            url,
            headers,
            body,
            timeout_ms: settings.timeout_ms.value,
            follow_redirects: settings.follow_redirects.value,
            max_redirects: settings.max_redirects.value,
            encode_url: settings.encode_url.value,
            auth,
            contains_secret: r.contains_secret,
            secrets: r.secrets,
        },
        problems: r.problems,
    })
}

/// Applies `scheme`: to the headers or the query when it can, or as a
/// [`TransportAuth`] for the transport.
fn apply_auth(
    r: &mut Renderer<'_, '_>,
    scheme: &AuthScheme,
    headers: &mut Vec<(String, String)>,
    url: &mut String,
) -> Result<TransportAuth, AppError> {
    let mut auth = TransportAuth::None;
    match scheme {
        AuthScheme::AwsV4(aws) => {
            let credentials = AwsV4Credentials {
                access_key_id: r.text(&aws.access_key_id)?,
                secret_access_key: r.text(&aws.secret_access_key)?,
                session_token: r.text(&aws.session_token)?,
                service: r.text(&aws.service)?,
                region: r.text(&aws.region)?,
            };
            for (name, value) in [
                ("access key id", &credentials.access_key_id),
                ("secret access key", &credentials.secret_access_key),
                ("service", &credentials.service),
                ("region", &credentials.region),
            ] {
                if value.trim().is_empty() {
                    return Err(AppError::Validation(format!(
                        "AWS Signature v4 needs the {name}"
                    )));
                }
            }
            auth = TransportAuth::AwsV4(credentials);
        }
        AuthScheme::Digest(digest) => {
            auth = TransportAuth::Digest {
                username: r.text(&digest.username)?,
                password: r.text(&digest.password)?,
            };
        }
        AuthScheme::Ntlm(ntlm) => {
            let username = r.text(&ntlm.username)?;
            let mut domain = r.text(&ntlm.domain)?;
            // `DOMAIN\user` gives both, when the domain is not set apart.
            let username = match username.split_once('\\') {
                Some((d, user)) if domain.is_empty() => {
                    d.clone_into(&mut domain);
                    user.to_owned()
                }
                _ => username,
            };
            auth = TransportAuth::Ntlm {
                username,
                password: r.text(&ntlm.password)?,
                domain,
            };
        }
        AuthScheme::Basic(basic) => {
            let credentials = format!("{}:{}", r.text(&basic.username)?, r.text(&basic.password)?);
            set_header(
                headers,
                "Authorization",
                format!("Basic {}", base64(credentials.as_bytes())),
            );
        }
        AuthScheme::Bearer(bearer) => {
            let token = r.text(&bearer.token)?;
            set_header(headers, "Authorization", format!("Bearer {token}"));
        }
        AuthScheme::ApiKey(key) => {
            let name = r.text(&key.key)?;
            let value = r.text(&key.value)?;
            match key.placement {
                ApiKeyPlacement::Header => set_header(headers, &name, value),
                ApiKeyPlacement::Query => append_query(url, &name, &value),
            }
        }
        AuthScheme::OAuth2(_) => {
            return Err(AppError::Validation(
                "OAuth 2.0 cannot get tokens yet; set the token as a Bearer auth for now".into(),
            ));
        }
        AuthScheme::None => {}
        AuthScheme::Unsupported(kind) => {
            return Err(AppError::Validation(format!(
                "auth scheme `{kind}` is not supported yet"
            )));
        }
        // Error out rather than silently sending without auth. The message
        // does not include the scheme's fields.
        _ => {
            return Err(AppError::Validation(
                "this auth scheme is not supported yet".into(),
            ));
        }
    }
    Ok(auth)
}

struct Renderer<'v, 'h> {
    variables: &'v ResolvedVariables,
    host: &'h mut dyn Host,
    mode: Mode,
    contains_secret: bool,
    secrets: Vec<Secret>,
    problems: Vec<Problem>,
}

impl Renderer<'_, '_> {
    fn text(&mut self, template: &str) -> Result<String, AppError> {
        let rendered = render(template, self.variables, self.host, self.mode)?;
        self.contains_secret |= rendered.contains_secret;
        self.secrets.extend(rendered.secrets);
        self.problems.extend(rendered.problems);
        Ok(rendered.text.into_owned())
    }

    /// The rendered body and the `Content-Type` it implies, if any.
    fn body(&mut self, body: &Body) -> Result<(RenderedBody, Option<String>), AppError> {
        let text_body = |r: &mut Self, text: &str, content_type: &str| {
            Ok::<_, AppError>((
                RenderedBody::Text(r.text(text)?),
                Some(content_type.to_owned()),
            ))
        };
        Ok(match body {
            Body::None => (RenderedBody::None, None),
            Body::Json(text) => text_body(self, text, "application/json")?,
            Body::Text(text) => text_body(self, text, "text/plain")?,
            Body::Xml(text) => text_body(self, text, "application/xml")?,
            Body::Sparql(text) => text_body(self, text, "application/sparql-query")?,
            Body::FormUrlEncoded(fields) => {
                let mut out = Vec::new();
                for field in fields.iter().filter(|f| f.enabled) {
                    out.push((self.text(&field.name)?, self.text(&field.value)?));
                }
                (
                    RenderedBody::Form(out),
                    Some("application/x-www-form-urlencoded".to_owned()),
                )
            }
            Body::Multipart(parts) => {
                let mut out = Vec::new();
                for part in parts.iter().filter(|p| p.enabled) {
                    out.push(RenderedPart {
                        name: self.text(&part.name)?,
                        value: match &part.value {
                            MultipartValue::Text(text) => MultipartValue::Text(self.text(text)?),
                            MultipartValue::Files(paths) => {
                                let mut rendered = Vec::with_capacity(paths.len());
                                for path in paths {
                                    rendered.push(self.text(path)?);
                                }
                                MultipartValue::Files(rendered)
                            }
                        },
                        content_type: part.content_type.clone(),
                    });
                }
                // The transport adds the header, since it needs the boundary.
                (RenderedBody::Multipart(out), None)
            }
            Body::File { path, content_type } => {
                (RenderedBody::File(self.text(path)?), content_type.clone())
            }
            _ => {
                return Err(AppError::Validation(
                    "this body kind is not supported yet".into(),
                ));
            }
        })
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

/// Replaces every header named `name` (case-insensitively) with one value.
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    headers.push((name.to_owned(), value));
}

/// Replaces `:name` segments in the path of `url` (not in the host, query or
/// fragment). Segments without a matching, non-empty parameter are kept.
fn substitute_path_params(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_owned();
    }
    let path_start = url.find("://").map_or(0, |scheme_end| {
        url[scheme_end + 3..]
            .find('/')
            .map_or(url.len(), |p| p + scheme_end + 3)
    });
    let path_end = url[path_start..]
        .find(['?', '#'])
        .map_or(url.len(), |p| p + path_start);

    let path = url[path_start..path_end]
        .split('/')
        .map(|segment| {
            segment
                .strip_prefix(':')
                .and_then(|name| params.iter().find(|(n, v)| n == name && !v.is_empty()))
                .map_or(segment, |(_, value)| value.as_str())
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{}{path}{}", &url[..path_start], &url[path_end..])
}

/// Appends `name=value` to the query of `url`, before any fragment.
fn append_query(url: &mut String, name: &str, value: &str) {
    let fragment_at = url.find('#').unwrap_or(url.len());
    let fragment = url.split_off(fragment_at);
    let separator = if url.contains('?') { '&' } else { '?' };
    url.push(separator);
    percent_encode(name, url);
    url.push('=');
    percent_encode(value, url);
    url.push_str(&fragment);
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
fn percent_encode(text: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

/// Standard base64 with padding.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| char::from(ALPHABET[((n >> shift) & 0x3f) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected, "input {input:?}");
        }
    }

    #[test]
    fn path_params_replace_only_path_segments() {
        let p = params(&[("id", "42"), ("org", "acme"), ("host", "evil")]);
        assert_eq!(
            substitute_path_params("https://api.test/:org/users/:id?x=:id#:id", &p),
            "https://api.test/acme/users/42?x=:id#:id"
        );
        assert_eq!(
            substitute_path_params("http://:host/users", &p),
            "http://:host/users"
        );
    }

    #[test]
    fn unknown_or_empty_path_params_are_kept() {
        let p = params(&[("id", "")]);
        assert_eq!(
            substitute_path_params("/users/:id/:other", &p),
            "/users/:id/:other"
        );
    }

    #[test]
    fn query_values_are_appended_and_encoded_before_the_fragment() {
        let mut url = "https://api.test/a?x=1#top".to_owned();
        append_query(&mut url, "api key", "a&b=c/ñ");
        assert_eq!(
            url,
            "https://api.test/a?x=1&api%20key=a%26b%3Dc%2F%C3%B1#top"
        );

        let mut bare = "https://api.test".to_owned();
        append_query(&mut bare, "k", "v");
        assert_eq!(bare, "https://api.test?k=v");
    }
}
