use super::*;

fn parse(command: &str) -> CurlImport {
    parse_curl(command).unwrap()
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    (request.headers.iter())
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

#[test]
fn a_chrome_copy_as_curl_becomes_a_get_with_its_headers_and_query() {
    let import = parse(
        "curl 'https://api.example.com/v1/users?page=2&sort=name' \\\n  -H 'accept: application/json' \\\n  -H 'authorization: Bearer abc.def' \\\n  --compressed",
    );
    let request = &import.request;
    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.url,
        "https://api.example.com/v1/users?page=2&sort=name"
    );
    assert_eq!(request.name, "users");
    assert_eq!(header(request, "accept"), Some("application/json"));
    assert_eq!(header(request, "authorization"), Some("Bearer abc.def"));
    let params: Vec<(&str, &str)> = (request.params.iter())
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(params, [("page", "2"), ("sort", "name")]);
    assert_eq!(request.body, Body::None);
    assert!(import.ignored.is_empty(), "{:?}", import.ignored);
}

#[test]
fn json_bodies_follow_their_content_type_and_make_a_post() {
    let raw = parse(
        r#"curl https://api.test/users -H 'Content-Type: application/json' --data-raw '{"name":"Ana"}'"#,
    );
    assert_eq!(raw.request.method, Method::Post);
    assert_eq!(raw.request.body, Body::Json(r#"{"name":"Ana"}"#.into()));

    let json = parse(r#"curl --json '{"a":1}' api.test/x"#);
    assert_eq!(json.request.method, Method::Post);
    assert_eq!(json.request.body, Body::Json(r#"{"a":1}"#.into()));
    assert_eq!(
        header(&json.request, "content-type"),
        Some("application/json")
    );
    assert_eq!(header(&json.request, "accept"), Some("application/json"));
    // Without a scheme the URL stays so: it goes over http, as in curl.
    assert_eq!(json.request.url, "api.test/x");

    let xml = parse("curl -H 'content-type: text/xml' -d '<a/>' http://h/x");
    assert_eq!(xml.request.body, Body::Xml("<a/>".into()));
    let text = parse("curl -H 'Content-Type: text/plain' -d 'hello' http://h/x");
    assert_eq!(text.request.body, Body::Text("hello".into()));
}

#[test]
fn data_without_a_type_is_a_form_decoded_and_joined_like_curl() {
    let form =
        parse("curl -d 'user=ana%40x.com' -d 'note=a+b' --data-urlencode 'q=c d&e' http://h/login");
    assert_eq!(form.request.method, Method::Post);
    let Body::FormUrlEncoded(fields) = &form.request.body else {
        panic!("{:?}", form.request.body);
    };
    let pairs: Vec<(&str, &str)> = (fields.iter())
        .map(|f| (f.name.as_str(), f.value.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [("user", "ana@x.com"), ("note", "a b"), ("q", "c d&e")]
    );
}

#[test]
fn the_method_is_explicit_head_or_moved_to_the_query() {
    let put = parse("curl -X put -d 'a=1' http://h/x");
    assert_eq!(put.request.method, Method::Put);
    let joined = parse("curl -XDELETE http://h/x");
    assert_eq!(joined.request.method, Method::Delete);
    let head = parse("curl -I http://h/x");
    assert_eq!(head.request.method, Method::Head);
    let get = parse("curl -G -d 'q=rust' -d 'page=2' 'http://h/search?lang=es'");
    assert_eq!(get.request.method, Method::Get);
    assert_eq!(get.request.url, "http://h/search?lang=es&q=rust&page=2");
    assert_eq!(get.request.body, Body::None);
    assert_eq!(get.request.params.len(), 3);
}

#[test]
fn multipart_parts_keep_their_files_and_types() {
    let import = parse(
        "curl -F 'name=Ana' -F 'photo=@/tmp/me.png;type=image/png' --form-string 'raw=@not-a-file' http://h/up",
    );
    assert_eq!(import.request.method, Method::Post);
    let Body::Multipart(parts) = &import.request.body else {
        panic!("{:?}", import.request.body);
    };
    assert_eq!(parts[0].value, MultipartValue::Text("Ana".into()));
    assert_eq!(
        parts[1].value,
        MultipartValue::Files(vec!["/tmp/me.png".into()])
    );
    assert_eq!(parts[1].content_type.as_deref(), Some("image/png"));
    assert_eq!(parts[2].value, MultipartValue::Text("@not-a-file".into()));
}

#[test]
fn a_file_is_sent_as_a_file_body() {
    let import = parse(
        "curl --data-binary @payload.bin -H 'Content-Type: application/octet-stream' http://h/put",
    );
    assert_eq!(
        import.request.body,
        Body::File {
            path: "payload.bin".into(),
            content_type: Some("application/octet-stream".into()),
        }
    );
    // `--data-raw` keeps the `@`.
    let raw = parse("curl --data-raw '@me' -H 'Content-Type: text/plain' http://h/x");
    assert_eq!(raw.request.body, Body::Text("@me".into()));
}

#[test]
fn credentials_become_basic_digest_ntlm_or_bearer() {
    let auth = |command: &str| parse(command).request.auth;
    assert_eq!(
        auth("curl -u ana:p:ss http://h"),
        Setting::Set(AuthScheme::Basic(BasicAuth {
            username: "ana".into(),
            password: "p:ss".into(),
        }))
    );
    assert!(matches!(
        auth("curl --digest -u ana:x http://h"),
        Setting::Set(AuthScheme::Digest(_))
    ));
    assert!(matches!(
        auth("curl -u 'CORP\\ana:x' --ntlm http://h"),
        Setting::Set(AuthScheme::Ntlm(NtlmAuth { username, .. })) if username == "CORP\\ana"
    ));
    assert_eq!(
        auth("curl --oauth2-bearer t0k http://h"),
        Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "t0k".into()
        }))
    );
    assert_eq!(auth("curl http://h"), Setting::Inherit);
}

#[test]
fn other_headers_options_and_cookies() {
    let import = parse(
        "curl -sSL -A 'agent/1' -e https://ref -b 'sid=1; theme=dark' -H 'Host: a:8080' -H 'X-Empty;' --url=http://h/x",
    );
    let request = &import.request;
    assert_eq!(request.url, "http://h/x");
    assert_eq!(header(request, "user-agent"), Some("agent/1"));
    assert_eq!(header(request, "referer"), Some("https://ref"));
    assert_eq!(header(request, "cookie"), Some("sid=1; theme=dark"));
    assert_eq!(header(request, "host"), Some("a:8080"));
    assert_eq!(header(request, "x-empty"), Some(""));
    assert!(import.ignored.is_empty(), "{:?}", import.ignored);
}

#[test]
fn what_is_not_kept_is_listed() {
    let import = parse(
        "curl -k -o out.json --proxy http://p:3128 -b cookies.txt --http3-only --frobnicate http://h/x extra",
    );
    assert_eq!(
        import.ignored,
        [
            "-k",
            "--proxy http://p:3128",
            "-b cookies.txt",
            "--http3-only",
            "--frobnicate",
            "extra"
        ]
    );
}

#[test]
fn bad_commands_say_why() {
    assert_eq!(parse_curl("wget http://h"), Err(CurlError::NotCurl));
    assert_eq!(parse_curl(""), Err(CurlError::NotCurl));
    assert_eq!(parse_curl("curl 'http://h"), Err(CurlError::Unclosed('\'')));
    assert_eq!(parse_curl("curl -s"), Err(CurlError::NoUrl));
    assert_eq!(
        parse_curl("curl http://h -H"),
        Err(CurlError::MissingValue("-H".into()))
    );
    assert!(parse_curl("/usr/bin/curl http://h").is_ok());
}

#[test]
fn names_come_from_the_path() {
    assert_eq!(request_name("https://api.test/v1/users/42?x=1"), "42");
    assert_eq!(request_name("https://api.test/"), "api.test");
    assert_eq!(request_name("api.test"), "api.test");
    assert_eq!(request_name(""), "Imported request");
}
