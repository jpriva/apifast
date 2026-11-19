//! A cURL command as a request (docs/12, F6), with the options Bruno reads
//! (`bruno-app/src/utils/curl/parse-curl.js`) and what curl does with them:
//!
//! - `-X`, `-H`, `-A`, `-e`, `-b` (a `name=value` cookie), `-u` with
//!   `--basic`, `--digest` or `--ntlm`, `--oauth2-bearer`, `-I`, `-G`;
//! - bodies: `-d` and its kinds (`--data-raw`, `--data-binary`,
//!   `--data-urlencode`, `--json`), joined with `&` like curl; `@file` as a
//!   file body; `-F` and `--form-string` as multipart.
//!
//! Options that change nothing of the request (`-s`, `-L`, `-o file`…) are
//! skipped; any other is listed in [`CurlImport::ignored`], never dropped
//! silently. Variables like `$HOST` are not expanded.

mod shell;

use std::fmt::Write as _;

use apifast_domain::{
    AuthScheme, BasicAuth, BearerAuth, Body, DigestAuth, FormField, Header, HttpRequest, Method,
    MultipartPart, MultipartValue, NtlmAuth, Setting, sync_params_from_url,
};

/// A request read from a cURL command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    /// The request, named after its URL's path.
    pub request: HttpRequest,
    /// The options that were not kept, as written (`--proxy`, `-k`…).
    pub ignored: Vec<String>,
}

/// Why a command is not a cURL request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CurlError {
    /// It does not start with `curl`.
    #[error("not a curl command")]
    NotCurl,
    /// A quote is not closed.
    #[error("a {0} quote is not closed")]
    Unclosed(char),
    /// It has no URL.
    #[error("the curl command has no URL")]
    NoUrl,
    /// An option needs a value and has none.
    #[error("`{0}` needs a value")]
    MissingValue(String),
}

/// Options curl takes with a value that change nothing apifast keeps.
const SKIPPED_WITH_VALUE: &[&str] = &[
    "-o",
    "--output",
    "-m",
    "--max-time",
    "--connect-timeout",
    "-w",
    "--write-out",
    "-c",
    "--cookie-jar",
    "--retry",
    "--max-redirs",
    "-D",
    "--dump-header",
    "--limit-rate",
    "-r",
    "--range",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--retry-delay",
    "--retry-max-time",
];

/// Options curl takes with a value that apifast does not keep: reported.
const IGNORED_WITH_VALUE: &[&str] = &[
    "-x",
    "--proxy",
    "-U",
    "--proxy-user",
    "--cacert",
    "--capath",
    "-E",
    "--cert",
    "--key",
    "--cert-type",
    "--key-type",
    "--resolve",
    "--connect-to",
    "--interface",
    "-T",
    "--upload-file",
    "--aws-sigv4",
    "--unix-socket",
    "-K",
    "--config",
    "--noproxy",
];

/// Options without a value that change nothing apifast keeps.
const SKIPPED_FLAGS: &[&str] = &[
    "-s",
    "--silent",
    "-S",
    "--show-error",
    "-L",
    "--location",
    "-v",
    "--verbose",
    "-i",
    "--include",
    "-f",
    "--fail",
    "--fail-with-body",
    "-#",
    "--progress-bar",
    "-N",
    "--no-buffer",
    "--http1.1",
    "--http2",
    "--http1.0",
    "-g",
    "--globoff",
    "--location-trusted",
    "-n",
    "--netrc",
    "--no-progress-meter",
    "--compressed",
    "--basic",
    "-q",
];

/// Short options that take a value, written together or apart (`-XPOST`).
const SHORT_WITH_VALUE: &[char] = &[
    'X', 'H', 'd', 'u', 'b', 'F', 'A', 'e', 'o', 'm', 'x', 'U', 'E', 'T', 'w', 'c', 'D', 'r', 'K',
];

/// How a `-d` value is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DataKind {
    /// `-d`, `--data`, `--data-ascii`: `@file` reads a file.
    Plain,
    /// `--data-raw`: as written, `@` included.
    Raw,
    /// `--data-binary`: `@file` sends the file as it is.
    Binary,
    /// `--data-urlencode`: the value is encoded.
    UrlEncode,
}

/// What the options set, before it becomes a request.
#[derive(Default)]
struct Parsed {
    url: Option<String>,
    method: Option<String>,
    headers: Vec<Header>,
    data: Vec<(DataKind, String)>,
    json: bool,
    form: Vec<MultipartPart>,
    user: Option<String>,
    auth_kind: Option<&'static str>,
    bearer: Option<String>,
    head: bool,
    get: bool,
    ignored: Vec<String>,
}

/// The request `command` sends.
///
/// # Errors
///
/// If it is not a curl command, has an unclosed quote, no URL, or an
/// option without its value.
pub fn parse_curl(command: &str) -> Result<CurlImport, CurlError> {
    let words = shell::words(command.trim())
        .map_err(|shell::ShellError::Unclosed(q)| CurlError::Unclosed(q))?;
    let mut words = words.into_iter();
    if !words
        .next()
        .is_some_and(|w| w == "curl" || w.ends_with("/curl"))
    {
        return Err(CurlError::NotCurl);
    }
    let mut parsed = Parsed::default();
    let mut pending: Vec<String> = Vec::new();
    while let Some(word) = pending.pop().or_else(|| words.next()) {
        let (option, attached) = split_option(&word);
        let mut value = || {
            attached
                .clone()
                .or_else(|| words.next())
                .ok_or_else(|| CurlError::MissingValue(option.clone()))
        };
        match option.as_str() {
            "-X" | "--request" => parsed.method = Some(value()?.to_ascii_uppercase()),
            "-H" | "--header" => header(&mut parsed, &value()?),
            "-A" | "--user-agent" => parsed.headers.push(Header::new("User-Agent", value()?)),
            "-e" | "--referer" => parsed.headers.push(Header::new("Referer", value()?)),
            "-b" | "--cookie" => {
                let cookie = value()?;
                if cookie.contains('=') {
                    parsed.headers.push(Header::new("Cookie", cookie));
                } else {
                    parsed.ignored.push(format!("{option} {cookie}"));
                }
            }
            "-d" | "--data" | "--data-ascii" => parsed.data.push((DataKind::Plain, value()?)),
            "--data-raw" => parsed.data.push((DataKind::Raw, value()?)),
            "--data-binary" => parsed.data.push((DataKind::Binary, value()?)),
            "--data-urlencode" => parsed.data.push((DataKind::UrlEncode, value()?)),
            "--json" => {
                parsed.json = true;
                parsed.data.push((DataKind::Binary, value()?));
            }
            "-F" | "--form" => form(&mut parsed, &value()?, true),
            "--form-string" => form(&mut parsed, &value()?, false),
            "-u" | "--user" => parsed.user = Some(value()?),
            "--digest" => parsed.auth_kind = Some("digest"),
            "--ntlm" => parsed.auth_kind = Some("ntlm"),
            "--oauth2-bearer" => parsed.bearer = Some(value()?),
            "-I" | "--head" => parsed.head = true,
            "-G" | "--get" => parsed.get = true,
            "--url" => parsed.url = Some(value()?),
            "-k" | "--insecure" => parsed.ignored.push(option.clone()),
            o if SKIPPED_FLAGS.contains(&o) => {}
            o if SKIPPED_WITH_VALUE.contains(&o) => {
                value()?;
            }
            o if IGNORED_WITH_VALUE.contains(&o) => {
                let v = value()?;
                parsed.ignored.push(format!("{option} {v}"));
            }
            o if o.starts_with("--") => parsed.ignored.push(word),
            o if o.starts_with('-') && o.len() > 1 => match short_flags(&word) {
                // `-sSL`, or `-sXPOST`: each letter on its own.
                Some(split) => pending.extend(split.into_iter().rev()),
                None => parsed.ignored.push(word),
            },
            _ if parsed.url.is_none() => parsed.url = Some(word),
            _ => parsed.ignored.push(word),
        }
    }
    build(parsed)
}

/// `-XPOST` as `-X` and `POST`; `--data=x` as `--data` and `x`.
fn split_option(word: &str) -> (String, Option<String>) {
    if let Some(long) = word.strip_prefix("--")
        && let Some((name, value)) = long.split_once('=')
    {
        return (format!("--{name}"), Some(value.to_owned()));
    }
    let mut chars = word.chars();
    if chars.next() == Some('-')
        && let Some(letter) = chars.next()
        && letter != '-'
        && SHORT_WITH_VALUE.contains(&letter)
        && word.len() > 2
    {
        return (format!("-{letter}"), Some(word[2..].to_owned()));
    }
    (word.to_owned(), None)
}

/// `-sSL` as `-s`, `-S`, `-L`; `-sXPOST` as `-s`, `-XPOST`. `None` if it is
/// a single unknown letter.
fn short_flags(word: &str) -> Option<Vec<String>> {
    let letters: Vec<char> = word.chars().skip(1).collect();
    if letters.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for (i, letter) in letters.iter().enumerate() {
        if SHORT_WITH_VALUE.contains(letter) {
            let rest: String = letters[i..].iter().collect();
            out.push(format!("-{rest}"));
            return Some(out);
        }
        out.push(format!("-{letter}"));
    }
    Some(out)
}

fn header(parsed: &mut Parsed, value: &str) {
    match value.split_once(':') {
        Some((name, value)) if !name.trim().is_empty() => {
            parsed.headers.push(Header::new(name.trim(), value.trim()));
        }
        // `-H 'X-Empty;'` sends an empty header in curl.
        _ => match value.strip_suffix(';') {
            Some(name) if !name.trim().is_empty() => {
                parsed.headers.push(Header::new(name.trim(), ""));
            }
            _ => parsed.ignored.push(format!("-H {value}")),
        },
    }
}

/// A `-F name=value` part; `name=@file` is a file, with `;type=` taken as
/// its content type, unless `files` is off (`--form-string`).
fn form(parsed: &mut Parsed, value: &str, files: bool) {
    let Some((name, rest)) = value.split_once('=') else {
        parsed.ignored.push(format!("-F {value}"));
        return;
    };
    let (rest, content_type) = match rest.rsplit_once(";type=") {
        Some((rest, kind)) if files => (rest, Some(kind.to_owned())),
        _ => (rest, None),
    };
    let value = match rest.strip_prefix('@').filter(|_| files) {
        // `;filename=` would rename it: the file keeps its own name.
        Some(path) => MultipartValue::Files(vec![
            path.split(";filename=").next().unwrap_or(path).to_owned(),
        ]),
        None => MultipartValue::Text(rest.trim_matches('"').to_owned()),
    };
    parsed.form.push(MultipartPart {
        name: name.to_owned(),
        value,
        content_type,
        enabled: true,
    });
}

fn build(parsed: Parsed) -> Result<CurlImport, CurlError> {
    let mut url = parsed.url.clone().ok_or(CurlError::NoUrl)?;
    let content_type = (parsed.headers.iter())
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase());
    let mut headers = parsed.headers.clone();
    let mut body = Body::None;
    if parsed.get && !parsed.data.is_empty() {
        // `-G`: the data goes in the query.
        let query = joined(&parsed.data);
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&query);
    } else if !parsed.form.is_empty() {
        body = Body::Multipart(parsed.form.clone());
    } else if !parsed.data.is_empty() {
        if parsed.json {
            for (name, value) in [
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
            ] {
                if !headers.iter().any(|h| h.name.eq_ignore_ascii_case(name)) {
                    headers.push(Header::new(name, value));
                }
            }
        }
        body = data_body(
            &parsed,
            content_type
                .as_deref()
                .or(parsed.json.then_some("application/json")),
        );
    }
    let has_body = !matches!(body, Body::None);
    let method = match (&parsed.method, parsed.head) {
        (Some(method), _) => method.clone(),
        (None, true) => "HEAD".into(),
        (None, false) if has_body => "POST".into(),
        (None, false) => "GET".into(),
    };
    let name = request_name(&url);
    let mut request = HttpRequest::new(name, Method::parse(&method), url);
    request.headers = headers;
    request.body = body;
    if let Some(auth) = auth(&parsed) {
        request.auth = Setting::Set(auth);
    }
    sync_params_from_url(&mut request);
    Ok(CurlImport {
        request,
        ignored: parsed.ignored,
    })
}

/// The body of the `-d` values, after the content type: JSON, XML, a form
/// (the default, as curl sends it), or text.
fn data_body(parsed: &Parsed, content_type: Option<&str>) -> Body {
    // A single `@file` is a file body.
    if let [(kind, value)] = parsed.data.as_slice()
        && matches!(kind, DataKind::Plain | DataKind::Binary)
        && let Some(path) = value.strip_prefix('@')
    {
        return Body::File {
            path: path.to_owned(),
            content_type: content_type.map(str::to_owned),
        };
    }
    let text = joined(&parsed.data);
    match content_type {
        Some(t) if t.contains("json") => Body::Json(text),
        Some(t) if t.contains("xml") => Body::Xml(text),
        None => Body::FormUrlEncoded(form_fields(&text)),
        Some(t) if t.contains("x-www-form-urlencoded") => Body::FormUrlEncoded(form_fields(&text)),
        Some(_) => Body::Text(text),
    }
}

/// The `-d` values joined with `&`, as curl sends them; `--data-urlencode`
/// ones encoded.
fn joined(data: &[(DataKind, String)]) -> String {
    let parts: Vec<String> = data
        .iter()
        .map(|(kind, value)| match kind {
            DataKind::UrlEncode => match value.split_once('=') {
                Some((name, v)) if !name.is_empty() => format!("{name}={}", encode(v)),
                Some((_, v)) => encode(v),
                None => encode(value),
            },
            _ => value.clone(),
        })
        .collect();
    parts.join("&")
}

/// `a=1&b=x%20y` as fields, decoded: apifast encodes them again when it
/// sends the form.
fn form_fields(text: &str) -> Vec<FormField> {
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            FormField {
                name: decode(name),
                value: decode(value),
                enabled: true,
            }
        })
        .collect()
}

fn auth(parsed: &Parsed) -> Option<AuthScheme> {
    if let Some(token) = &parsed.bearer {
        return Some(AuthScheme::Bearer(BearerAuth {
            token: token.clone(),
        }));
    }
    let user = parsed.user.as_ref()?;
    let (username, password) = user.split_once(':').unwrap_or((user.as_str(), ""));
    let (username, password) = (username.to_owned(), password.to_owned());
    Some(match parsed.auth_kind {
        Some("digest") => AuthScheme::Digest(DigestAuth { username, password }),
        Some("ntlm") => AuthScheme::Ntlm(NtlmAuth {
            username,
            password,
            domain: String::new(),
        }),
        _ => AuthScheme::Basic(BasicAuth { username, password }),
    })
}

/// A name for the request: the last segment of the URL's path, or its
/// host.
fn request_name(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = without_scheme.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    let host = segments.next().unwrap_or_default();
    let last = segments.next_back().unwrap_or(host);
    if last.is_empty() {
        "Imported request".into()
    } else {
        last.to_owned()
    }
}

/// Percent-encodes all but the unreserved characters, as curl does with
/// `--data-urlencode`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Decodes `%XX` and `+` (a space, in a form).
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], escaped) {
            (_, Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (b'+', None) => out.push(b' '),
            (other, None) => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
