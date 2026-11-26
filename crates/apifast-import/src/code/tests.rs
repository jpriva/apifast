use std::collections::BTreeMap;

use apifast_app::memory::{CountingRandom, FixedClock};
use apifast_app::{AppHost, NoPrompter, render_request};
use apifast_domain::{
    AuthScheme, Body, Chain, Collection, DigestAuth, FormField, Header, HttpRequest, Method,
    MultipartPart, MultipartValue, ResolvedVariables, Setting, resolve,
};

use super::*;

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

fn json_post() -> HttpRequest {
    let mut request = HttpRequest::new("r", Method::Post, "https://api.test/users?x=1");
    request.headers = vec![Header::new("X-Quote", "say \"hi\"")];
    request.body = Body::Json("{\"name\": \"Ana\"}".into());
    request
}

fn upload() -> HttpRequest {
    let mut request = HttpRequest::new("u", Method::Post, "https://api.test/up");
    request.body = Body::Multipart(vec![
        MultipartPart {
            name: "note".into(),
            value: MultipartValue::Text("hi".into()),
            content_type: None,
            enabled: true,
        },
        MultipartPart {
            name: "doc".into(),
            value: MultipartValue::Files(vec!["files/a.pdf".into()]),
            content_type: None,
            enabled: true,
        },
    ]);
    request
}

#[test]
fn javascript_uses_fetch() {
    let code = to_code(CodeLanguage::JavaScript, &rendered(&json_post()), None);
    let text = &code.text;
    assert!(
        text.starts_with("const response = await fetch(\"https://api.test/users?x=1\", {"),
        "{text}"
    );
    assert!(text.contains("  method: \"POST\",\n"), "{text}");
    assert!(
        text.contains("    \"X-Quote\": \"say \\\"hi\\\"\",\n"),
        "{text}"
    );
    assert!(
        text.contains("    \"Content-Type\": \"application/json\",\n"),
        "{text}"
    );
    assert!(
        text.contains("  body: \"{\\\"name\\\": \\\"Ana\\\"}\",\n"),
        "{text}"
    );
    assert!(!text.contains("redirect:"), "follows by default: {text}");
    assert_eq!(code.notes, Vec::<String>::new());

    let code = to_code(
        CodeLanguage::JavaScript,
        &rendered(&upload()),
        Some(Path::new("/c")),
    );
    assert!(
        code.text.starts_with("import fs from \"node:fs\";"),
        "{}",
        code.text
    );
    // As this system joins it: `/c\\files\\a.pdf` (escaped) on Windows.
    let pdf = Path::new("/c").join("files").join("a.pdf");
    let pdf = serde_json::to_string(&pdf.display().to_string()).unwrap();
    assert!(
        code.text.contains(&format!(
            "form.append(\"doc\", await fs.openAsBlob({pdf}), \"a.pdf\");"
        )),
        "{}",
        code.text
    );
    assert!(code.text.contains("  body: form,\n"), "{}", code.text);
    assert_eq!(code.notes.len(), 1);
}

#[test]
fn python_uses_requests_with_a_timeout() {
    let mut request = json_post();
    request.settings.follow_redirects = Setting::Set(false);
    request.settings.timeout_ms = Setting::Set(2500);
    let text = to_code(CodeLanguage::Python, &rendered(&request), None).text;
    assert!(
        text.starts_with("import requests\n\nresponse = requests.request(\n    \"POST\",\n"),
        "{text}"
    );
    assert!(
        text.contains("    data=\"{\\\"name\\\": \\\"Ana\\\"}\",\n"),
        "{text}"
    );
    assert!(
        text.contains("    allow_redirects=False,\n    timeout=2.5,\n)"),
        "{text}"
    );

    let mut form = HttpRequest::new("f", Method::Post, "https://api.test/login");
    form.body = Body::FormUrlEncoded(vec![FormField {
        name: "user".into(),
        value: "ana".into(),
        enabled: true,
    }]);
    form.auth = Setting::Set(AuthScheme::Digest(DigestAuth {
        username: "u".into(),
        password: "p".into(),
    }));
    let code = to_code(CodeLanguage::Python, &rendered(&form), None);
    assert!(
        code.text
            .contains("from requests.auth import HTTPDigestAuth"),
        "{}",
        code.text
    );
    assert!(
        code.text.contains("    data=[(\"user\", \"ana\")],\n"),
        "{}",
        code.text
    );
    assert!(
        code.text
            .contains("    auth=HTTPDigestAuth(\"u\", \"p\"),\n"),
        "{}",
        code.text
    );
    assert_eq!(code.notes, Vec::<String>::new(), "requests does Digest");

    let text = to_code(CodeLanguage::Python, &rendered(&upload()), None).text;
    assert!(text.contains("(\"note\", (None, \"hi\")),"), "{text}");
    assert!(
        text.contains("(\"doc\", (\"a.pdf\", open(\"files/a.pdf\", \"rb\"))),"),
        "{text}"
    );
}

#[test]
fn go_uses_net_http_with_only_the_imports_it_needs() {
    let text = to_code(CodeLanguage::Go, &rendered(&json_post()), None).text;
    assert!(
        text.starts_with(
            "package main\n\nimport (\n\t\"fmt\"\n\t\"io\"\n\t\"net/http\"\n\t\"strings\"\n)\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "http.NewRequest(\"POST\", \"https://api.test/users?x=1\", strings.NewReader("
        ),
        "{text}"
    );
    assert!(
        text.contains("\treq.Header.Add(\"X-Quote\", \"say \\\"hi\\\"\")\n"),
        "{text}"
    );
    assert!(
        !text.contains("CheckRedirect") && !text.contains("time"),
        "{text}"
    );

    let mut digest = upload();
    digest.auth = Setting::Set(AuthScheme::Digest(DigestAuth {
        username: "u".into(),
        password: "p".into(),
    }));
    digest.settings.follow_redirects = Setting::Set(false);
    let code = to_code(CodeLanguage::Go, &rendered(&digest), None);
    assert!(
        code.text.contains("writer.CreateFormFile(\"doc\""),
        "{}",
        code.text
    );
    assert!(
        code.text
            .contains("req.Header.Set(\"Content-Type\", writer.FormDataContentType())"),
        "{}",
        code.text
    );
    assert!(
        code.text.contains("http.ErrUseLastResponse"),
        "{}",
        code.text
    );
    assert_eq!(
        code.notes,
        ["Digest auth is not done by net/http: add it with a library"]
    );
}

#[test]
fn every_language_has_a_label_and_curl_is_the_cli_command() {
    for language in CodeLanguage::ALL {
        assert_ne!(language.label(), "");
    }
    let request = rendered(&json_post());
    assert_eq!(
        to_code(CodeLanguage::Curl, &request, None).text,
        crate::to_curl(&request, None)
    );
}
