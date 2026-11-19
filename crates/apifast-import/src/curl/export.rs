//! A rendered request as a cURL command (docs/12, F6: "Copiar como
//! cURL"), to run in a terminal: long options, one per line, every value
//! in single quotes so the shell takes it as it is.

use std::path::Path;

use apifast_app::{RenderedBody, RenderedRequest, TransportAuth};
use apifast_domain::{Method, MultipartValue};

/// `request` as a cURL command. Relative file paths are made absolute from
/// `files_root` (the collection's folder), so the command runs anywhere.
pub fn to_curl(request: &RenderedRequest, files_root: Option<&Path>) -> String {
    let mut lines = vec![format!("curl --request {}", request.method().as_str())];
    if matches!(request.method(), Method::Head) {
        // curl waits for a body after a HEAD unless told it is one.
        lines[0] = "curl --head".into();
    }
    lines.push(format!("--url {}", quote(request.url())));
    for (name, value) in request.headers() {
        lines.push(format!("--header {}", quote(&format!("{name}: {value}"))));
    }
    match request.auth() {
        TransportAuth::Digest { username, password } => {
            lines.push("--digest".into());
            lines.push(format!(
                "--user {}",
                quote(&format!("{username}:{password}"))
            ));
        }
        TransportAuth::Ntlm {
            username,
            password,
            domain,
        } => {
            let user = if domain.is_empty() {
                username.clone()
            } else {
                format!("{domain}\\{username}")
            };
            lines.push("--ntlm".into());
            lines.push(format!("--user {}", quote(&format!("{user}:{password}"))));
        }
        TransportAuth::AwsV4(aws) => {
            let scope = format!("aws:amz:{}:{}", aws.region, aws.service);
            lines.push(format!("--aws-sigv4 {}", quote(&scope)));
            let keys = format!("{}:{}", aws.access_key_id, aws.secret_access_key);
            lines.push(format!("--user {}", quote(&keys)));
            if !aws.session_token.is_empty() {
                let token = format!("x-amz-security-token: {}", aws.session_token);
                lines.push(format!("--header {}", quote(&token)));
            }
        }
        _ => {}
    }
    let file = |path: &str| match files_root {
        Some(root) if !Path::new(path).is_absolute() => root.join(path).display().to_string(),
        _ => path.to_owned(),
    };
    match request.body() {
        RenderedBody::Text(text) => lines.push(format!("--data-raw {}", quote(text))),
        RenderedBody::Form(fields) => {
            for (name, value) in fields {
                lines.push(format!(
                    "--data-urlencode {}",
                    quote(&format!("{name}={value}"))
                ));
            }
        }
        RenderedBody::Multipart(parts) => {
            for part in parts {
                let kind = part
                    .content_type
                    .as_ref()
                    .map(|t| format!(";type={t}"))
                    .unwrap_or_default();
                match &part.value {
                    // `--form-string`: a value starting with `@` or `<` is text.
                    MultipartValue::Text(text) if kind.is_empty() => lines.push(format!(
                        "--form-string {}",
                        quote(&format!("{}={text}", part.name))
                    )),
                    MultipartValue::Text(text) => lines.push(format!(
                        "--form {}",
                        quote(&format!("{}={text}{kind}", part.name))
                    )),
                    MultipartValue::Files(paths) => {
                        for path in paths {
                            let value = format!("{}=@{}{kind}", part.name, file(path));
                            lines.push(format!("--form {}", quote(&value)));
                        }
                    }
                }
            }
        }
        RenderedBody::File(path) => {
            lines.push(format!(
                "--data-binary {}",
                quote(&format!("@{}", file(path)))
            ));
        }
        _ => {}
    }
    if request.follow_redirects() {
        lines.push("--location".into());
    }
    lines.join(" \\\n  ")
}

/// `text` in single quotes for a POSIX shell; a `'` inside is closed,
/// escaped and opened again.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apifast_app::memory::{CountingRandom, FixedClock};
    use apifast_app::{AppHost, NoPrompter, render_request};
    use apifast_domain::{
        AuthScheme, BasicAuth, Body, Chain, Collection, DigestAuth, FormField, Header, HttpRequest,
        MultipartPart, ResolvedVariables, Setting, resolve,
    };

    use super::*;
    use crate::parse_curl;

    fn rendered(request: &HttpRequest) -> RenderedRequest {
        let collection = Collection::new("c");
        let resolved = resolve(Chain::new(&collection, &[], request));
        let (clock, random) = (FixedClock(0), CountingRandom::new(0));
        let mut host = AppHost::new(&clock, &random, &NoPrompter, BTreeMap::new());
        let mode = apifast_template::Mode::Strict;
        render_request(&resolved, &ResolvedVariables::default(), &mut host, mode)
            .unwrap()
            .request
    }

    /// `request` to cURL and back: what a send sends, read again.
    fn round_trip(request: &HttpRequest) -> HttpRequest {
        let command = to_curl(&rendered(request), None);
        let back = parse_curl(&command).unwrap();
        assert_eq!(back.ignored, Vec::<String>::new(), "{command}");
        back.request
    }

    #[test]
    fn a_json_post_reads_well_and_comes_back() {
        let mut request = HttpRequest::new("r", Method::Post, "https://api.test/users?x=1");
        request.headers = vec![Header::new("X-Note", "it's fine")];
        request.body = Body::Json(r#"{"name": "Ana"}"#.into());
        let command = to_curl(&rendered(&request), None);
        assert_eq!(
            command,
            "curl --request POST \\\n  --url 'https://api.test/users?x=1' \\\n  --header 'X-Note: it'\\''s fine' \\\n  --header 'Content-Type: application/json' \\\n  --data-raw '{\"name\": \"Ana\"}' \\\n  --location"
        );
        let back = round_trip(&request);
        assert_eq!((back.method, back.url), (Method::Post, request.url.clone()));
        assert_eq!(back.body, request.body);
        assert_eq!(back.headers[0], Header::new("X-Note", "it's fine"));
    }

    #[test]
    fn forms_multipart_and_files_come_back() {
        let mut form = HttpRequest::new("r", Method::Post, "http://h/login");
        form.body = Body::FormUrlEncoded(vec![FormField {
            name: "q".into(),
            value: "a b&c=d".into(),
            enabled: true,
        }]);
        let back = round_trip(&form);
        assert_eq!(back.body, form.body);

        let mut multipart = HttpRequest::new("r", Method::Post, "http://h/up");
        multipart.body = Body::Multipart(vec![
            MultipartPart {
                name: "note".into(),
                value: MultipartValue::Text("@literally".into()),
                content_type: None,
                enabled: true,
            },
            MultipartPart {
                name: "doc".into(),
                value: MultipartValue::Files(vec!["docs/a.pdf".into()]),
                content_type: Some("application/pdf".into()),
                enabled: true,
            },
        ]);
        let command = to_curl(&rendered(&multipart), Some(Path::new("/work/shop")));
        assert!(
            command.contains("--form-string 'note=@literally'"),
            "{command}"
        );
        assert!(
            command.contains("--form 'doc=@/work/shop/docs/a.pdf;type=application/pdf'"),
            "{command}"
        );
        let Body::Multipart(parts) = parse_curl(&command).unwrap().request.body else {
            panic!("{command}");
        };
        assert_eq!(parts[0].value, MultipartValue::Text("@literally".into()));
        assert_eq!(
            parts[1].value,
            MultipartValue::Files(vec!["/work/shop/docs/a.pdf".into()])
        );

        let mut file = HttpRequest::new("r", Method::Put, "http://h/put");
        file.body = Body::File {
            path: "/tmp/x.bin".into(),
            content_type: None,
        };
        let back = round_trip(&file);
        assert!(matches!(back.body, Body::File { ref path, .. } if path == "/tmp/x.bin"));
    }

    #[test]
    fn auth_comes_as_headers_or_curl_options() {
        let mut basic = HttpRequest::new("r", Method::Get, "http://h/");
        basic.auth = Setting::Set(AuthScheme::Basic(BasicAuth {
            username: "ana".into(),
            password: "p".into(),
        }));
        let command = to_curl(&rendered(&basic), None);
        assert!(
            command.contains("--header 'Authorization: Basic YW5hOnA='"),
            "{command}"
        );

        let mut digest = HttpRequest::new("r", Method::Get, "http://h/");
        digest.auth = Setting::Set(AuthScheme::Digest(DigestAuth {
            username: "ana".into(),
            password: "p".into(),
        }));
        let back = round_trip(&digest);
        assert_eq!(back.auth, digest.auth);
    }

    #[test]
    fn head_requests_and_no_redirects() {
        let mut head = HttpRequest::new("r", Method::Head, "http://h/");
        head.settings.follow_redirects = Setting::Set(false);
        let command = to_curl(&rendered(&head), None);
        assert_eq!(command, "curl --head \\\n  --url 'http://h/'");
        assert_eq!(round_trip(&head).method, Method::Head);
    }

    #[test]
    fn quoting_survives_single_quotes() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
    }
}
