//! Keeps a request's URL and its parameter list in step, as the editor does
//! (and Bruno does). Query parameters are sent from the URL; the list
//! mirrors them, and also keeps disabled ones, which are not in the URL.
//! Path parameters are the `:name` segments of the URL's path, with their
//! values in the list.
//!
//! Texts are templates and are never decoded or encoded here.

use crate::{HttpRequest, Param, ParamKind};

/// The URL split as `base`, query (without `?`) and fragment (with `#`).
fn split(url: &str) -> (&str, Option<&str>, &str) {
    let fragment_at = url.find('#').unwrap_or(url.len());
    let (before, fragment) = url.split_at(fragment_at);
    match before.split_once('?') {
        Some((base, query)) => (base, Some(query), fragment),
        None => (before, None, fragment),
    }
}

/// The query parameters written in `url`, in order: `a=1&b` gives
/// `[("a", "1"), ("b", "")]`.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    let (_, query, _) = split(url);
    query
        .into_iter()
        .flat_map(|q| q.split('&'))
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (name.to_owned(), value.to_owned()),
            None => (pair.to_owned(), String::new()),
        })
        .collect()
}

/// Names of the `:name` segments in the path of `url`, without repeats.
pub fn path_param_names(url: &str) -> Vec<String> {
    let (base, _, _) = split(url);
    // Skip `scheme://host`, where `:` means a port.
    let path = base.find("://").map_or(base, |at| {
        base[at + 3..].find('/').map_or("", |p| &base[at + 3 + p..])
    });
    let mut names: Vec<String> = Vec::new();
    for segment in path.split('/') {
        if let Some(name) = segment.strip_prefix(':').filter(|n| !n.is_empty())
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// After the URL changed: the list takes the URL's query parameters
/// (keeping disabled ones, which the URL does not show) and one path
/// parameter per `:name` segment (keeping the values already typed).
pub fn sync_params_from_url(request: &mut HttpRequest) {
    let old = std::mem::take(&mut request.params);
    let mut params: Vec<Param> = query_params(&request.url)
        .into_iter()
        .map(|(name, value)| Param {
            name,
            value,
            kind: ParamKind::Query,
            enabled: true,
        })
        .collect();
    params.extend(
        old.iter()
            .filter(|p| p.kind == ParamKind::Query && !p.enabled)
            .cloned(),
    );
    for name in path_param_names(&request.url) {
        let kept = old
            .iter()
            .find(|p| p.kind == ParamKind::Path && p.name == name)
            .cloned();
        params.push(kept.unwrap_or(Param {
            name,
            value: String::new(),
            kind: ParamKind::Path,
            enabled: true,
        }));
    }
    request.params = params;
}

/// After the list changed: the URL's query becomes the enabled query
/// parameters of the list, in order. The rest of the URL is kept.
pub fn sync_url_from_params(request: &mut HttpRequest) {
    let (base, _, fragment) = split(&request.url);
    let query: Vec<String> = request
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Query && p.enabled && !p.name.is_empty())
        .map(|p| {
            if p.value.is_empty() {
                p.name.clone()
            } else {
                format!("{}={}", p.name, p.value)
            }
        })
        .collect();
    let mut url = base.to_owned();
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    url.push_str(fragment);
    request.url = url;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Method;

    fn query(name: &str, value: &str, enabled: bool) -> Param {
        Param {
            name: name.into(),
            value: value.into(),
            kind: ParamKind::Query,
            enabled,
        }
    }

    fn path(name: &str, value: &str) -> Param {
        Param {
            name: name.into(),
            value: value.into(),
            kind: ParamKind::Path,
            enabled: true,
        }
    }

    #[test]
    fn query_parameters_are_read_raw_and_in_order() {
        assert_eq!(
            query_params("{{host}}/u?a=1&b&c={{x}}&&d=a=b#top?no=1"),
            [
                ("a".into(), "1".into()),
                ("b".into(), String::new()),
                ("c".into(), "{{x}}".into()),
                ("d".into(), "a=b".into()),
            ]
        );
        assert_eq!(
            query_params("{{host}}/u"),
            [] as [(std::string::String, std::string::String); 0]
        );
    }

    #[test]
    fn path_parameters_come_from_the_path_only() {
        assert_eq!(
            path_param_names("http://localhost:8080/:org/users/:id/:id?x=:q#:f"),
            ["org", "id"]
        );
        assert_eq!(path_param_names("{{host}}/users/:id"), ["id"]);
        assert_eq!(
            path_param_names("http://localhost:8080"),
            [] as [std::string::String; 0]
        );
    }

    #[test]
    fn the_list_follows_the_url_and_keeps_disabled_and_typed_values() {
        let mut request = HttpRequest::new("r", Method::Get, "{{host}}/u/:id?page=2&q=x");
        request.params = vec![
            query("page", "1", true),
            query("debug", "true", false),
            path("id", "981"),
            path("gone", "1"),
        ];
        sync_params_from_url(&mut request);
        assert_eq!(
            request.params,
            [
                query("page", "2", true),
                query("q", "x", true),
                query("debug", "true", false),
                path("id", "981"),
            ]
        );
    }

    #[test]
    fn the_url_follows_the_list_and_keeps_path_and_fragment() {
        let mut request = HttpRequest::new("r", Method::Get, "{{host}}/u/:id?old=1#top");
        request.params = vec![
            query("page", "2", true),
            query("debug", "true", false),
            query("flag", "", true),
            query("", "ignored", true),
            path("id", "981"),
        ];
        sync_url_from_params(&mut request);
        assert_eq!(request.url, "{{host}}/u/:id?page=2&flag#top");

        request.params.clear();
        sync_url_from_params(&mut request);
        assert_eq!(request.url, "{{host}}/u/:id#top");
    }

    #[test]
    fn syncing_both_ways_is_stable() {
        let mut request = HttpRequest::new("r", Method::Get, "{{host}}/u?a=1&b=2");
        sync_params_from_url(&mut request);
        let before = request.clone();
        sync_url_from_params(&mut request);
        sync_params_from_url(&mut request);
        assert_eq!(request, before);
    }
}
