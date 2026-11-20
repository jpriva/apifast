//! Cookies (RFC 6265): reading `Set-Cookie`, the jar that keeps them, and
//! which of them go with each request (docs/07, "Cookies").
//!
//! - `Domain` must cover the host that set the cookie; one without a dot
//!   (`com`) is only accepted as the host itself. There is no public suffix
//!   list, so `example.co.uk` can still set a cookie for `co.uk`.
//! - `Secure` cookies only go over `https`, or to the machine itself
//!   (`localhost`, `127.0.0.1`, `::1`), as tough-cookie does in Bruno.
//! - `__Secure-` needs `Secure`; `__Host-` also needs no `Domain` and `/`
//!   as path.

use std::net::IpAddr;

mod date;

use date::parse_cookie_date;
pub use date::{format_expiry, parse_expiry};

/// A cookie in the jar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// Name.
    pub name: String,
    /// Value, as the server sent it.
    pub value: String,
    /// Domain, in lowercase and without a leading dot.
    pub domain: String,
    /// Sent only to `domain` itself, not to its subdomains: the server gave
    /// no `Domain`.
    pub host_only: bool,
    /// Path: the cookie goes with requests at it or below it.
    pub path: String,
    /// Sent only over a secure connection.
    pub secure: bool,
    /// Not for scripts. Kept and shown; it does not change what is sent.
    pub http_only: bool,
    /// `SameSite`. Kept and shown; it does not change what is sent.
    pub same_site: Option<SameSite>,
    /// When it expires, in milliseconds since the Unix epoch; `None` for a
    /// session cookie.
    pub expires_ms: Option<i64>,
    /// When it was first stored, to order cookies with the same path.
    pub created_ms: i64,
}

/// The `SameSite` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// `Strict`.
    Strict,
    /// `Lax`.
    Lax,
    /// `None`.
    None,
}

impl SameSite {
    /// As written in a `Set-Cookie`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }

    /// From its name, in any case.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "lax" => Some(Self::Lax),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

impl Cookie {
    /// Whether it has expired at `now_ms`.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_ms.is_some_and(|at| at <= now_ms)
    }

    /// Whether it goes with a request to `target`.
    fn goes_to(&self, target: &CookieTarget<'_>) -> bool {
        let domain_ok = if self.host_only {
            target.host == self.domain
        } else {
            domain_matches(&target.host, &self.domain)
        };
        domain_ok && path_matches(target.path, &self.path) && (!self.secure || target.secure)
    }

    fn same_slot(&self, other: &Self) -> bool {
        self.name == other.name && self.domain == other.domain && self.path == other.path
    }
}

/// Where a request goes, as cookies see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieTarget<'a> {
    host: String,
    path: &'a str,
    secure: bool,
}

impl<'a> CookieTarget<'a> {
    /// A request to `scheme://host/path` (the path without its query).
    /// `host` may be an IPv6 address in brackets.
    pub fn new(scheme: &str, host: &str, path: &'a str) -> Self {
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        let secure = scheme.eq_ignore_ascii_case("https") || is_local(&host);
        Self { host, path, secure }
    }
}

impl<'a> CookieTarget<'a> {
    /// A request to `url` (`scheme://[user@]host[:port]/path?query`), or
    /// `None` if it has no scheme or host.
    pub fn from_url(url: &'a str) -> Option<Self> {
        let (scheme, rest) = url.trim().split_once("://")?;
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = if host_port.starts_with('[') {
            host_port.split_inclusive(']').next().unwrap_or(host_port)
        } else {
            host_port.split(':').next().unwrap_or(host_port)
        };
        if scheme.is_empty() || host.is_empty() {
            return None;
        }
        let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
        Some(Self::new(scheme, host, &tail[..path_end]))
    }

    /// Its host, in lowercase.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Its path, without the query.
    pub fn path(&self) -> &str {
        self.path
    }
}

/// A cookie as a script sets it (`bru.cookies.jar().setCookie(url, …)`):
/// what it leaves out is taken from the URL, as a `Set-Cookie` would.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieSpec {
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
    /// Its domain: `None` for the URL's host alone.
    pub domain: Option<String>,
    /// Its path: `None` for the URL's, up to its last `/`.
    pub path: Option<String>,
    /// Sent only over a secure connection.
    pub secure: bool,
    /// Not for scripts.
    pub http_only: bool,
    /// `SameSite`.
    pub same_site: Option<SameSite>,
    /// When it expires, in milliseconds since the Unix epoch; `None` for a
    /// session cookie.
    pub expires_ms: Option<i64>,
}

/// The cookie `spec` sets from `target` at `now_ms`, or `None` if a
/// `Set-Cookie` with it would be ignored: no name, or a domain the host is
/// not in (RFC 6265, 5.3).
pub fn cookie_from_spec(
    spec: CookieSpec,
    target: &CookieTarget<'_>,
    now_ms: i64,
) -> Option<Cookie> {
    if spec.name.is_empty() {
        return None;
    }
    let (domain, host_only) = match spec.domain.as_deref().map(|d| d.trim_start_matches('.')) {
        Some(domain) if !domain.is_empty() => {
            let domain = domain.to_ascii_lowercase();
            if !domain_matches(&target.host, &domain) {
                return None;
            }
            (domain, false)
        }
        _ => (target.host.clone(), true),
    };
    let path = match spec.path {
        Some(path) if path.starts_with('/') => path,
        _ => default_path(target.path),
    };
    Some(Cookie {
        name: spec.name,
        value: spec.value,
        domain,
        host_only,
        path,
        secure: spec.secure,
        http_only: spec.http_only,
        same_site: spec.same_site,
        expires_ms: spec.expires_ms,
        created_ms: now_ms,
    })
}

/// The machine itself: secure enough for `Secure` cookies.
fn is_local(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether `host` is `domain` or one of its subdomains (RFC 6265, 5.1.3).
fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
            && host.parse::<IpAddr>().is_err())
}

/// Whether a request at `path` is at or below the cookie's `cookie_path`
/// (RFC 6265, 5.1.4).
fn path_matches(path: &str, cookie_path: &str) -> bool {
    let path = if path.is_empty() { "/" } else { path };
    path == cookie_path
        || (path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || path.as_bytes()[cookie_path.len()] == b'/'))
}

/// The path a cookie gets when the server gives none: the request's, up to
/// its last `/` (RFC 6265, 5.1.4).
fn default_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(last) => path[..last].to_owned(),
    }
}

/// The cookie a `Set-Cookie` value sets, received from `target` at `now_ms`,
/// or `None` if it must be ignored (RFC 6265, 5.2 and 5.3).
pub fn parse_set_cookie(header: &str, target: &CookieTarget<'_>, now_ms: i64) -> Option<Cookie> {
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let (name, value) = (name.trim(), value.trim());
    if name.is_empty() {
        return None;
    }
    let mut cookie = Cookie {
        name: name.to_owned(),
        value: value.to_owned(),
        domain: target.host.clone(),
        host_only: true,
        path: default_path(target.path),
        secure: false,
        http_only: false,
        same_site: None,
        expires_ms: None,
        created_ms: now_ms,
    };
    let mut domain = None;
    let mut max_age = None;
    let mut expires = None;
    for attribute in parts {
        let (key, value) = attribute.split_once('=').unwrap_or((attribute, ""));
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "expires" => expires = parse_cookie_date(value).or(expires),
            "max-age" => max_age = parse_max_age(value, now_ms).or(max_age),
            "domain" if !value.is_empty() => {
                domain = Some(value.trim_start_matches('.').to_ascii_lowercase());
            }
            "path" => {
                cookie.path = if value.starts_with('/') {
                    value.to_owned()
                } else {
                    default_path(target.path)
                };
            }
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            "samesite" => cookie.same_site = SameSite::parse(value),
            _ => {}
        }
    }
    cookie.expires_ms = max_age.or(expires);
    let had_domain = domain.is_some();
    if let Some(domain) = domain {
        let covers = domain_matches(&target.host, &domain);
        let too_wide = !domain.contains('.') && domain != target.host;
        if !covers || too_wide {
            return None;
        }
        cookie.host_only = false;
        cookie.domain = domain;
    }
    prefixes_hold(&cookie, had_domain).then_some(cookie)
}

/// `Max-Age`: seconds from now; zero or less means already expired.
fn parse_max_age(value: &str, now_ms: i64) -> Option<i64> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Too many digits for an i64 is a very long time.
    let seconds = value.parse::<i64>().unwrap_or(i64::MAX);
    Some(if seconds <= 0 {
        i64::MIN
    } else {
        now_ms.saturating_add(seconds.saturating_mul(1000))
    })
}

/// The `__Secure-` and `__Host-` rules.
fn prefixes_hold(cookie: &Cookie, has_domain: bool) -> bool {
    if cookie.name.starts_with("__Host-") {
        cookie.secure && !has_domain && cookie.path == "/"
    } else if cookie.name.starts_with("__Secure-") {
        cookie.secure
    } else {
        true
    }
}

/// The cookies of a collection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    /// An empty jar.
    pub fn new() -> Self {
        Self::default()
    }

    /// A jar with `cookies`, leaving out those expired at `now_ms`. Of two
    /// with the same name, domain and path, the later stays.
    pub fn with_cookies(cookies: impl IntoIterator<Item = Cookie>, now_ms: i64) -> Self {
        let mut jar = Self::new();
        for cookie in cookies {
            jar.set(cookie, now_ms);
        }
        jar
    }

    /// The cookies, by domain, then path, then name.
    pub fn cookies(&self) -> &[Cookie] {
        &self.cookies
    }

    /// Whether it has no cookies.
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// Keeps what the `Set-Cookie` value `header`, received from `target`,
    /// sets. Returns whether the jar changed.
    pub fn receive(&mut self, header: &str, target: &CookieTarget<'_>, now_ms: i64) -> bool {
        parse_set_cookie(header, target, now_ms).is_some_and(|cookie| self.set(cookie, now_ms))
    }

    /// Puts `cookie` in its place (same name, domain and path), keeping the
    /// creation time of the one it replaces. An expired cookie removes the
    /// one in its place. Returns whether the jar changed.
    pub fn set(&mut self, mut cookie: Cookie, now_ms: i64) -> bool {
        let existing = self.cookies.iter().position(|c| c.same_slot(&cookie));
        if cookie.is_expired(now_ms) {
            return existing.map(|i| self.cookies.remove(i)).is_some();
        }
        if let Some(i) = existing {
            cookie.created_ms = self.cookies[i].created_ms;
            let changed = self.cookies[i] != cookie;
            self.cookies[i] = cookie;
            return changed;
        }
        let at = self.cookies.partition_point(|c| order(c) < order(&cookie));
        self.cookies.insert(at, cookie);
        true
    }

    /// The `Cookie` header for a request to `target` at `now_ms`, if any
    /// cookie goes with it: longer paths first, then the older ones.
    pub fn header_for(&self, target: &CookieTarget<'_>, now_ms: i64) -> Option<String> {
        let going = self.going_to(target, now_ms);
        if going.is_empty() {
            return None;
        }
        let pairs: Vec<String> = going
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        Some(pairs.join("; "))
    }

    /// The cookies that go with a request to `target` at `now_ms`, in the
    /// order they go: longer paths first, then the older ones.
    pub fn going_to(&self, target: &CookieTarget<'_>, now_ms: i64) -> Vec<&Cookie> {
        let mut going: Vec<&Cookie> = (self.cookies.iter())
            .filter(|c| !c.is_expired(now_ms) && c.goes_to(target))
            .collect();
        going.sort_by_key(|c| (std::cmp::Reverse(c.path.len()), c.created_ms));
        going
    }

    /// Removes the cookie `name` of `domain` at `path`. Returns whether it
    /// was there.
    pub fn remove(&mut self, domain: &str, path: &str, name: &str) -> bool {
        let before = self.cookies.len();
        (self.cookies).retain(|c| !(c.domain == domain && c.path == path && c.name == name));
        self.cookies.len() != before
    }

    /// Removes every cookie of `domain`. Returns whether there was any.
    pub fn remove_domain(&mut self, domain: &str) -> bool {
        let before = self.cookies.len();
        self.cookies.retain(|c| c.domain != domain);
        self.cookies.len() != before
    }

    /// Removes every cookie.
    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Removes the cookies expired at `now_ms`. Returns whether there was any.
    pub fn remove_expired(&mut self, now_ms: i64) -> bool {
        let before = self.cookies.len();
        self.cookies.retain(|c| !c.is_expired(now_ms));
        self.cookies.len() != before
    }
}

fn order(cookie: &Cookie) -> (&str, &str, &str) {
    (&cookie.domain, &cookie.path, &cookie.name)
}

/// `existing`, the `Cookie` header of a request, with the cookies of the
/// jar's `from_jar` added; a cookie in both takes the jar's value, as in
/// Bruno.
pub fn merge_cookie_header(existing: &str, from_jar: &str) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for pair in existing.split(';').chain(from_jar.split(';')) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            continue;
        }
        match pairs.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => value.clone_into(v),
            None => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    let joined: Vec<String> = pairs.iter().map(|(n, v)| format!("{n}={v}")).collect();
    joined.join("; ")
}

#[cfg(test)]
mod tests;
