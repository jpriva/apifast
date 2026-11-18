//! The collection's cookie jar on each request that goes out: its cookies
//! join the request's `Cookie` header, and each response's `Set-Cookie`
//! go into it, also on redirects and challenges (docs/07, "Cookies").

use std::time::{SystemTime, UNIX_EPOCH};

use apifast_app::Cookies;
use apifast_domain::{CookieTarget, merge_cookie_header};
use reqwest::Url;
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, SET_COOKIE};

/// Adds the jar's cookies for `url` to `headers`, merged with any `Cookie`
/// the request has: a cookie in both takes the jar's value, as in Bruno.
pub(crate) fn add_from_jar(cookies: &Cookies, url: &Url, headers: &mut HeaderMap) {
    let Some(from_jar) = cookies.header_for(&target(url), now_ms()) else {
        return;
    };
    let written: Vec<&str> = (headers.get_all(COOKIE).iter())
        .filter_map(|v| v.to_str().ok())
        .collect();
    let merged = if written.is_empty() {
        from_jar
    } else {
        merge_cookie_header(&written.join("; "), &from_jar)
    };
    // A value the server set that cannot go in a header is left out.
    if let Ok(value) = HeaderValue::from_str(&merged) {
        headers.insert(COOKIE, value);
    }
}

/// Keeps in the jar the cookies `response_headers`, from `url`, set.
pub(crate) fn keep(cookies: &Cookies, url: &Url, response_headers: &HeaderMap) {
    let set = (response_headers.get_all(SET_COOKIE).iter()).filter_map(|v| v.to_str().ok());
    cookies.receive(set, &target(url), now_ms());
}

fn target(url: &Url) -> CookieTarget<'_> {
    CookieTarget::new(url.scheme(), url.host_str().unwrap_or_default(), url.path())
}

fn now_ms() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
}
