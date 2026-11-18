//! The response's Cookies tab: the cookies its `Set-Cookie` headers set,
//! read as the jar reads them (docs/09, "Panel de respuesta").

use apifast_domain::{Cookie, CookieTarget, format_expiry, parse_set_cookie};
use eframe::egui::{self, RichText};

use crate::organisms::cookies_window::flags;
use crate::strings::Strings;
use crate::theme;

/// The cookies the response headers set, received from `url` at `now_ms`.
/// The history keeps `Set-Cookie` hidden, so an old response gives none.
pub fn set_by(headers: &[(String, String)], url: &str, now_ms: i64) -> Vec<Cookie> {
    let (scheme, host, path) = parts(url);
    let target = CookieTarget::new(scheme, host, path);
    (headers.iter())
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, value)| parse_set_cookie(value, &target, now_ms))
        .collect()
}

/// How many `Set-Cookie` headers there are.
pub fn count(headers: &[(String, String)]) -> usize {
    (headers.iter())
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .count()
}

/// Scheme, host (without its port) and path of `url`.
fn parts(url: &str) -> (&str, &str, &str) {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, after) = rest.split_at(authority_end);
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        host_port
            .split_once(']')
            .map_or(host_port, |(h, _)| &h[1..])
    } else {
        host_port.split_once(':').map_or(host_port, |(h, _)| h)
    };
    let path_end = after.find(['?', '#']).unwrap_or(after.len());
    (scheme, host, &after[..path_end])
}

/// Draws the tab.
pub fn show(ui: &mut egui::Ui, cookies: &[Cookie], strings: &Strings) {
    let palette = theme::palette(ui);
    if cookies.is_empty() {
        ui.label(RichText::new(strings.response_no_cookies).color(palette.text_muted));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("response-cookies")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("response-cookies-grid")
                .num_columns(6)
                .striped(true)
                .show(ui, |ui| {
                    for header in [
                        strings.cookies_name,
                        strings.cookies_value,
                        strings.cookies_domain,
                        strings.cookies_path,
                        strings.cookies_expires,
                        strings.cookies_flags,
                    ] {
                        ui.label(RichText::new(header).color(palette.text_muted).small());
                    }
                    ui.end_row();
                    for cookie in cookies {
                        ui.label(RichText::new(&cookie.name).strong().monospace());
                        ui.add(
                            egui::Label::new(RichText::new(&cookie.value).monospace()).truncate(),
                        )
                        .on_hover_text(&cookie.value);
                        ui.label(RichText::new(&cookie.domain).monospace());
                        ui.label(RichText::new(&cookie.path).monospace());
                        ui.label(match cookie.expires_ms {
                            None => strings.cookies_session.to_owned(),
                            Some(at) => format_expiry(at),
                        });
                        ui.label(RichText::new(flags(cookie, strings)).small());
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_gives_scheme_host_and_path() {
        assert_eq!(
            parts("https://u:p@API.test:8443/a/b?q=1#f"),
            ("https", "API.test", "/a/b")
        );
        assert_eq!(parts("http://[::1]:80/x"), ("http", "::1", "/x"));
        assert_eq!(parts("http://h"), ("http", "h", ""));
        assert_eq!(parts("h:1/p"), ("http", "h", "/p"));
    }

    #[test]
    fn set_cookie_headers_are_read_as_the_jar_reads_them() {
        let headers = [
            (
                "Set-Cookie".to_owned(),
                "sid=1; Path=/; HttpOnly".to_owned(),
            ),
            ("set-cookie".to_owned(), "bad".to_owned()),
            ("Content-Type".to_owned(), "text/plain".to_owned()),
        ];
        let cookies = set_by(&headers, "https://api.test:8443/login", 0);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].domain, "api.test");
        assert!(cookies[0].http_only);
        assert_eq!(count(&headers), 2);
    }
}
