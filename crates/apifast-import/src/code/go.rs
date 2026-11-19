//! Go with `net/http`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use apifast_app::{RenderedBody, RenderedRequest};
use apifast_domain::{MultipartValue, Settings};

use super::{Code, Files, auth_note, quote};

pub(super) fn code(request: &RenderedRequest, files: &Files<'_>) -> Code {
    let notes: Vec<String> = auth_note(request.auth(), "net/http").into_iter().collect();
    let mut imports: BTreeSet<&str> = ["fmt", "io", "net/http"].into();
    let (body_code, body, multipart) = body(request.body(), files, &mut imports);
    let custom_timeout = request.timeout_ms() != Settings::DEFAULT_TIMEOUT_MS;
    if custom_timeout {
        imports.insert("time");
    }
    let mut text = String::from("package main\n\nimport (\n");
    for import in &imports {
        let _ = writeln!(text, "\t{}", quote(import));
    }
    text.push_str(")\n\nfunc main() {\n");
    text.push_str(&body_code);
    let _ = write!(
        text,
        "\treq, err := http.NewRequest({}, {}, {body})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n",
        quote(request.method().as_str()),
        quote(request.url())
    );
    for (name, value) in request.headers() {
        let _ = writeln!(text, "\treq.Header.Add({}, {})", quote(name), quote(value));
    }
    if multipart {
        text.push_str("\treq.Header.Set(\"Content-Type\", writer.FormDataContentType())\n");
    }
    if custom_timeout {
        let _ = writeln!(
            text,
            "\tclient := &http.Client{{Timeout: {} * time.Millisecond}}",
            request.timeout_ms()
        );
    } else {
        text.push_str("\tclient := &http.Client{}\n");
    }
    if !request.follow_redirects() {
        text.push_str(
            "\tclient.CheckRedirect = func(*http.Request, []*http.Request) error {\n\t\treturn http.ErrUseLastResponse\n\t}\n",
        );
    }
    text.push_str(
        "\tres, err := client.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer res.Body.Close()\n\tdata, _ := io.ReadAll(res.Body)\n\tfmt.Println(res.Status, string(data))\n}\n",
    );
    Code { text, notes }
}

/// The code that builds `body`, the expression it is sent with, and
/// whether it is multipart (whose `Content-Type` the writer gives).
fn body(
    body: &RenderedBody,
    files: &Files<'_>,
    imports: &mut BTreeSet<&'static str>,
) -> (String, String, bool) {
    let mut body_code = String::new();
    let mut multipart = false;
    let body = match body {
        RenderedBody::Text(text) => {
            imports.insert("strings");
            format!("strings.NewReader({})", quote(text))
        }
        RenderedBody::Form(fields) => {
            imports.extend(["net/url", "strings"]);
            body_code.push_str("\tform := url.Values{}\n");
            for (name, value) in fields {
                let _ = writeln!(body_code, "\tform.Add({}, {})", quote(name), quote(value));
            }
            "strings.NewReader(form.Encode())".into()
        }
        RenderedBody::Multipart(parts) => {
            multipart = true;
            imports.extend(["bytes", "mime/multipart"]);
            body_code.push_str("\tvar body bytes.Buffer\n\twriter := multipart.NewWriter(&body)\n");
            for part in parts {
                match &part.value {
                    MultipartValue::Text(text) => {
                        let _ = writeln!(
                            body_code,
                            "\twriter.WriteField({}, {})",
                            quote(&part.name),
                            quote(text)
                        );
                    }
                    MultipartValue::Files(paths) => {
                        imports.extend(["os", "path/filepath"]);
                        for path in paths {
                            let path = quote(&files.path(path));
                            let _ = write!(
                                body_code,
                                "\tif file, err := os.Open({path}); err == nil {{\n\
                                 \t\tpart, _ := writer.CreateFormFile({}, filepath.Base({path}))\n\
                                 \t\tio.Copy(part, file)\n\
                                 \t\tfile.Close()\n\
                                 \t}}\n",
                                quote(&part.name)
                            );
                        }
                    }
                }
            }
            body_code.push_str("\twriter.Close()\n");
            "&body".into()
        }
        RenderedBody::File(path) => {
            imports.insert("os");
            let _ = write!(
                body_code,
                "\tfile, err := os.Open({})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n\tdefer file.Close()\n",
                quote(&files.path(path))
            );
            "file".into()
        }
        _ => "nil".into(),
    };
    (body_code, body, multipart)
}
