//! Python with `requests`.

use std::fmt::Write as _;

use apifast_app::{RenderedBody, RenderedRequest, TransportAuth};
use apifast_domain::MultipartValue;

use super::{Code, Files, quote};

pub(super) fn code(request: &RenderedRequest, files: &Files<'_>) -> Code {
    let mut notes = Vec::new();
    let mut imports = vec!["import requests".to_owned()];
    let mut arguments = vec![quote(request.method().as_str()), quote(request.url())];
    if !request.headers().is_empty() {
        let mut headers = String::from("headers={\n");
        for (name, value) in request.headers() {
            let _ = writeln!(headers, "        {}: {},", quote(name), quote(value));
        }
        headers.push_str("    }");
        arguments.push(headers);
    }
    arguments.extend(body_argument(request.body(), files));
    arguments.extend(auth_argument(request.auth(), &mut imports, &mut notes));
    if !request.follow_redirects() {
        arguments.push("allow_redirects=False".into());
    }
    // `requests` waits forever without one.
    #[expect(clippy::cast_precision_loss, reason = "a timeout in ms fits an f64")]
    let seconds = request.timeout_ms() as f64 / 1000.0;
    arguments.push(format!("timeout={seconds:?}"));
    let mut text = imports.join("\n");
    text.push_str("\n\nresponse = requests.request(\n");
    for argument in arguments {
        let _ = writeln!(text, "    {argument},");
    }
    text.push_str(")\n\nprint(response.status_code, response.text)\n");
    Code { text, notes }
}

/// The argument that sends `body`, if any.
fn body_argument(body: &RenderedBody, files: &Files<'_>) -> Option<String> {
    match body {
        RenderedBody::Text(text) => Some(format!("data={}", quote(text))),
        RenderedBody::Form(fields) => {
            let pairs: Vec<String> = (fields.iter())
                .map(|(name, value)| format!("({}, {})", quote(name), quote(value)))
                .collect();
            Some(format!("data=[{}]", pairs.join(", ")))
        }
        RenderedBody::Multipart(parts) => {
            let mut list = String::from("files=[\n");
            for part in parts {
                let kind = part
                    .content_type
                    .as_ref()
                    .map(|t| format!(", {}", quote(t)));
                match &part.value {
                    MultipartValue::Text(text) => {
                        let _ = writeln!(
                            list,
                            "        ({}, (None, {}{})),",
                            quote(&part.name),
                            quote(text),
                            kind.as_deref().unwrap_or_default()
                        );
                    }
                    MultipartValue::Files(paths) => {
                        for path in paths {
                            let path = files.path(path);
                            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
                            let _ = writeln!(
                                list,
                                "        ({}, ({}, open({}, \"rb\"){})),",
                                quote(&part.name),
                                quote(&name),
                                quote(&path),
                                kind.as_deref().unwrap_or_default()
                            );
                        }
                    }
                }
            }
            list.push_str("    ]");
            Some(list)
        }
        RenderedBody::File(path) => {
            Some(format!("data=open({}, \"rb\")", quote(&files.path(path))))
        }
        _ => None,
    }
}

/// The argument for an auth `requests` does with a helper, and its import.
fn auth_argument(
    auth: &TransportAuth,
    imports: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Option<String> {
    match auth {
        TransportAuth::Digest { username, password } => {
            imports.push("from requests.auth import HTTPDigestAuth".into());
            Some(format!(
                "auth=HTTPDigestAuth({}, {})",
                quote(username),
                quote(password)
            ))
        }
        TransportAuth::Ntlm {
            username,
            password,
            domain,
        } => {
            imports.push("from requests_ntlm import HttpNtlmAuth".into());
            let user = if domain.is_empty() {
                username.clone()
            } else {
                format!("{domain}\\{username}")
            };
            notes.push("NTLM needs the `requests_ntlm` package".into());
            Some(format!(
                "auth=HttpNtlmAuth({}, {})",
                quote(&user),
                quote(password)
            ))
        }
        TransportAuth::AwsV4(aws) => {
            imports.push("from requests_aws4auth import AWS4Auth".into());
            let mut auth = format!(
                "auth=AWS4Auth({}, {}, {}, {}",
                quote(&aws.access_key_id),
                quote(&aws.secret_access_key),
                quote(&aws.region),
                quote(&aws.service)
            );
            if !aws.session_token.is_empty() {
                let _ = write!(auth, ", session_token={}", quote(&aws.session_token));
            }
            auth.push(')');
            notes.push("AWS Signature V4 needs the `requests-aws4auth` package".into());
            Some(auth)
        }
        _ => None,
    }
}
