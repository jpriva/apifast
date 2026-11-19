//! JavaScript with `fetch`.

use std::fmt::Write as _;

use apifast_app::{RenderedBody, RenderedRequest};
use apifast_domain::{MultipartValue, Settings};

use super::{Code, Files, auth_note, quote};

pub(super) fn code(request: &RenderedRequest, files: &Files<'_>) -> Code {
    let mut notes: Vec<String> = auth_note(request.auth(), "fetch").into_iter().collect();
    let mut prelude = String::new();
    let mut uses_fs = false;
    let body = match request.body() {
        RenderedBody::Text(text) => Some(quote(text)),
        RenderedBody::Form(fields) => {
            let pairs: Vec<String> = (fields.iter())
                .map(|(name, value)| format!("[{}, {}]", quote(name), quote(value)))
                .collect();
            Some(format!("new URLSearchParams([{}])", pairs.join(", ")))
        }
        RenderedBody::Multipart(parts) => {
            prelude.push_str("const form = new FormData();\n");
            for part in parts {
                match &part.value {
                    MultipartValue::Text(text) => {
                        let value = match &part.content_type {
                            Some(kind) => {
                                format!("new Blob([{}], {{ type: {} }})", quote(text), quote(kind))
                            }
                            None => quote(text),
                        };
                        let _ = writeln!(prelude, "form.append({}, {value});", quote(&part.name));
                    }
                    MultipartValue::Files(paths) => {
                        uses_fs = true;
                        for path in paths {
                            let path = files.path(path);
                            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
                            let _ = writeln!(
                                prelude,
                                "form.append({}, await fs.openAsBlob({}), {});",
                                quote(&part.name),
                                quote(&path),
                                quote(&name)
                            );
                        }
                    }
                }
            }
            prelude.push('\n');
            Some("form".into())
        }
        RenderedBody::File(path) => {
            uses_fs = true;
            Some(format!("await fs.openAsBlob({})", quote(&files.path(path))))
        }
        _ => None,
    };
    let mut text = String::new();
    if uses_fs {
        text.push_str("import fs from \"node:fs\";\n\n");
        notes.push("files are read with Node's `fs`; in a browser, take them from an input".into());
    }
    text.push_str(&prelude);
    let _ = writeln!(
        text,
        "const response = await fetch({}, {{",
        quote(request.url())
    );
    let _ = writeln!(text, "  method: {},", quote(request.method().as_str()));
    if !request.headers().is_empty() {
        text.push_str("  headers: {\n");
        for (name, value) in request.headers() {
            let _ = writeln!(text, "    {}: {},", quote(name), quote(value));
        }
        text.push_str("  },\n");
    }
    if let Some(body) = body {
        let _ = writeln!(text, "  body: {body},");
    }
    if !request.follow_redirects() {
        text.push_str("  redirect: \"manual\",\n");
    }
    if request.timeout_ms() != Settings::DEFAULT_TIMEOUT_MS {
        let _ = writeln!(
            text,
            "  signal: AbortSignal.timeout({}),",
            request.timeout_ms()
        );
    }
    text.push_str("});\n\nconsole.log(response.status, await response.text());\n");
    Code { text, notes }
}
