//! The transport against a local HTTP server written for these tests. No
//! test uses the external network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use apifast_app::memory::{CountingRandom, FixedClock};
use apifast_app::{
    AppHost, BodyWriter, CancelToken, HttpTransport, NoPrompter, RenderedRequest, ResponseHead,
    StoreError, StoredBody, TimelineKind, TransportError, render_request,
};
use apifast_domain::{
    Body, Chain, Collection, FormField, Header, HttpRequest, Method, MultipartPart, MultipartValue,
    ResolvedVariables, Setting, resolve,
};
use apifast_http::ReqwestTransport;
use apifast_template::Mode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// --- A tiny HTTP/1.1 server ---------------------------------------------------

#[derive(Debug, Clone)]
struct Received {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Reply {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
    delay: Duration,
}

impl Reply {
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok("")
        }
    }

    fn header(mut self, name: &str, value: impl Into<Vec<u8>>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    fn after(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn redirect(status: u16, location: &str) -> Self {
        Self::status(status).header("Location", location)
    }
}

type Handler = Arc<dyn Fn(&Received) -> Reply + Send + Sync>;

struct Server {
    addr: SocketAddr,
    received: Arc<Mutex<Vec<Received>>>,
}

impl Server {
    async fn start(handler: impl Fn(&Received) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);
        let log = received.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (handler, log) = (handler.clone(), log.clone());
                tokio::spawn(async move { serve(stream, &*handler, &log).await });
            }
        });
        Self { addr, received }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}

async fn serve(
    mut stream: TcpStream,
    handler: &(dyn Fn(&Received) -> Reply + Send + Sync),
    log: &Mutex<Vec<Received>>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = find(&buffer, b"\r\n\r\n") {
            break i;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default().to_owned();
    let path = request_line.next().unwrap_or_default().to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_lowercase(), v.trim().to_owned()))
        .collect();
    let mut rest = buffer[head_end + 4..].to_vec();

    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|t| t.contains("chunked"))
    {
        loop {
            if let Some(body) = dechunk(&rest) {
                break body;
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => rest.extend_from_slice(&chunk[..n]),
            }
        }
    } else {
        let length: usize = headers
            .get("content-length")
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        while rest.len() < length {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => rest.extend_from_slice(&chunk[..n]),
            }
        }
        rest
    };

    let received = Received {
        method,
        path,
        headers,
        body,
    };
    log.lock().unwrap().push(received.clone());
    let reply = handler(&received);
    tokio::time::sleep(reply.delay).await;

    let mut out = format!(
        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    )
    .into_bytes();
    for (name, value) in &reply.headers {
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&reply.body);
    let _ = stream.write_all(&out).await;
    let _ = stream.shutdown().await;
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The body of a complete chunked message, or `None` if more bytes are needed.
fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_end = find(data, b"\r\n")?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&data[..line_end])
                .ok()?
                .split(';')
                .next()?
                .trim(),
            16,
        )
        .ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(body);
        }
        if data.len() < size + 2 {
            return None;
        }
        body.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

// --- Helpers ----------------------------------------------------------------

/// Collects the body and how it arrived.
#[derive(Default)]
struct Collect {
    body: Vec<u8>,
    writes: usize,
    largest_write: usize,
}

impl BodyWriter for Collect {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        self.body.extend_from_slice(chunk);
        self.writes += 1;
        self.largest_write = self.largest_write.max(chunk.len());
        Ok(())
    }

    fn finish(self) -> Result<StoredBody, StoreError> {
        unreachable!("the transport never finishes the writer")
    }
}

/// Renders a request with no variables, as the use cases would.
fn rendered(request: &HttpRequest) -> RenderedRequest {
    let collection = Collection::new("test");
    let resolved = resolve(Chain::new(&collection, &[], request));
    let (clock, random) = (FixedClock(0), CountingRandom::new(0));
    let mut host = AppHost::new(&clock, &random, &NoPrompter, BTreeMap::new());
    render_request(
        &resolved,
        &ResolvedVariables::default(),
        &mut host,
        Mode::Strict,
    )
    .unwrap()
    .request
}

fn transport() -> ReqwestTransport {
    ReqwestTransport::new(std::env::temp_dir()).unwrap()
}

async fn send(request: &HttpRequest) -> (Result<ResponseHead, TransportError>, Collect) {
    send_with(&transport(), request, &CancelToken::new()).await
}

async fn send_with(
    transport: &ReqwestTransport,
    request: &HttpRequest,
    cancel: &CancelToken,
) -> (Result<ResponseHead, TransportError>, Collect) {
    let mut collect = Collect::default();
    let result = transport
        .send(&rendered(request), &mut collect, cancel)
        .await;
    (result, collect)
}

fn get(url: String) -> HttpRequest {
    HttpRequest::new("r", Method::Get, url)
}

// --- Tests ----------------------------------------------------------------

#[tokio::test]
async fn sends_headers_and_streams_the_response() {
    let server = Server::start(|_| Reply::ok("hello").header("X-Reply", "yes")).await;
    let mut request = get(server.url("/hi?a=1&b=two"));
    request.headers = vec![
        Header::new("X-Test", "1"),
        Header::new("Accept", "text/plain"),
    ];

    let (result, body) = send(&request).await;
    let head = result.unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.version, "HTTP/1.1");
    assert_eq!(head.redirects, 0);
    assert_eq!(head.url, server.url("/hi?a=1&b=two"));
    assert!(head.headers.contains(&("x-reply".into(), "yes".into())));
    assert_eq!(body.body, b"hello");

    let received = &server.received()[0];
    assert_eq!(received.method, "GET");
    assert_eq!(received.path, "/hi?a=1&b=two");
    assert_eq!(received.headers["x-test"], "1");
    assert_eq!(received.headers["accept"], "text/plain");
    assert!(received.headers["user-agent"].starts_with("apifast/"));
}

#[tokio::test]
async fn an_explicit_user_agent_is_kept() {
    let server = Server::start(|_| Reply::ok("")).await;
    let mut request = get(server.url("/"));
    request.headers = vec![Header::new("User-Agent", "mine/1")];
    send(&request).await.0.unwrap();
    assert_eq!(server.received()[0].headers["user-agent"], "mine/1");
}

#[tokio::test]
async fn sends_text_and_form_bodies() {
    let server = Server::start(|_| Reply::ok("")).await;
    let mut json = HttpRequest::new("j", Method::Post, server.url("/json"));
    json.body = Body::Json(r#"{"a": "ñ"}"#.into());
    send(&json).await.0.unwrap();

    let mut form = HttpRequest::new("f", Method::Post, server.url("/form"));
    form.body = Body::FormUrlEncoded(vec![
        FormField {
            name: "a".into(),
            value: "1".into(),
            enabled: true,
        },
        FormField {
            name: "b".into(),
            value: "x y&z=ñ".into(),
            enabled: true,
        },
        FormField {
            name: "off".into(),
            value: "no".into(),
            enabled: false,
        },
    ]);
    send(&form).await.0.unwrap();

    let received = server.received();
    assert_eq!(received[0].body, r#"{"a": "ñ"}"#.as_bytes());
    assert_eq!(received[0].headers["content-type"], "application/json");
    assert_eq!(received[1].body, b"a=1&b=x+y%26z%3D%C3%B1");
    assert_eq!(
        received[1].headers["content-type"],
        "application/x-www-form-urlencoded"
    );
}

#[tokio::test]
async fn streams_file_bodies_and_multipart_files_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("upload.bin"), b"FILE-CONTENT").unwrap();
    std::fs::write(dir.path().join("a.txt"), b"part-a").unwrap();
    let transport = ReqwestTransport::new(dir.path()).unwrap();
    let server = Server::start(|_| Reply::ok("")).await;

    let mut file = HttpRequest::new("f", Method::Put, server.url("/file"));
    file.body = Body::File {
        path: "upload.bin".into(),
        content_type: Some("application/octet-stream".into()),
    };
    send_with(&transport, &file, &CancelToken::new())
        .await
        .0
        .unwrap();

    let mut multipart = HttpRequest::new("m", Method::Post, server.url("/multi"));
    multipart.body = Body::Multipart(vec![
        MultipartPart {
            name: "note".into(),
            value: MultipartValue::Text("hi".into()),
            content_type: None,
            enabled: true,
        },
        MultipartPart {
            name: "doc".into(),
            value: MultipartValue::Files(vec!["a.txt".into()]),
            content_type: Some("text/plain".into()),
            enabled: true,
        },
    ]);
    send_with(&transport, &multipart, &CancelToken::new())
        .await
        .0
        .unwrap();

    let received = server.received();
    assert_eq!(received[0].body, b"FILE-CONTENT");
    assert_eq!(received[0].headers["content-length"], "12");
    assert_eq!(
        received[0].headers["content-type"],
        "application/octet-stream"
    );

    let body = String::from_utf8_lossy(&received[1].body);
    assert!(received[1].headers["content-type"].starts_with("multipart/form-data; boundary="));
    assert!(
        body.contains("name=\"note\"") && body.contains("hi"),
        "{body}"
    );
    assert!(
        body.contains("name=\"doc\"; filename=\"a.txt\"") && body.contains("part-a"),
        "{body}"
    );
    assert!(body.contains("Content-Type: text/plain"), "{body}");
}

#[tokio::test]
async fn a_missing_upload_file_is_an_invalid_request() {
    let server = Server::start(|_| Reply::ok("")).await;
    let mut request = HttpRequest::new("f", Method::Put, server.url("/"));
    request.body = Body::File {
        path: "does-not-exist-apifast.bin".into(),
        content_type: None,
    };
    let (result, _) = send(&request).await;
    assert!(
        matches!(result, Err(TransportError::InvalidRequest(m)) if m.contains("does-not-exist-apifast.bin"))
    );
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn redirects_301_to_303_become_get_without_body() {
    let server = Server::start(|r| match r.path.as_str() {
        "/start" => Reply::redirect(302, "/middle"),
        "/middle" => Reply::redirect(303, "end?x=1"),
        _ => Reply::ok("done"),
    })
    .await;
    let mut request = HttpRequest::new("p", Method::Post, server.url("/start"));
    request.body = Body::Json("{}".into());

    let (result, body) = send(&request).await;
    let head = result.unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.redirects, 2);
    assert_eq!(head.url, server.url("/end?x=1"));
    assert_eq!(body.body, b"done");

    let received = server.received();
    let steps: Vec<_> = received
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str(), r.body.len()))
        .collect();
    assert_eq!(
        steps,
        [
            ("POST", "/start", 2),
            ("GET", "/middle", 0),
            ("GET", "/end?x=1", 0)
        ]
    );
    assert!(!received[1].headers.contains_key("content-type"));

    // The timeline tells each hop, in order and with growing times.
    let kinds: Vec<_> = head.timeline.iter().map(|e| e.kind.clone()).collect();
    let request = |method: &str, path: &str| TimelineKind::Request {
        method: method.into(),
        url: server.url(path),
    };
    let redirect = |status, location: &str| TimelineKind::Redirect {
        status,
        location: location.into(),
    };
    assert_eq!(
        kinds,
        [
            request("POST", "/start"),
            redirect(302, "/middle"),
            request("GET", "/middle"),
            redirect(303, "end?x=1"),
            request("GET", "/end?x=1"),
            TimelineKind::Headers { status: 200 },
            TimelineKind::Body { bytes: 4 },
        ]
    );
    assert!(head.timeline.windows(2).all(|w| w[0].at_us <= w[1].at_us));
}

#[tokio::test]
async fn redirects_307_and_308_keep_method_and_body() {
    let server = Server::start(|r| match r.path.as_str() {
        "/a" => Reply::redirect(307, "/b"),
        "/b" => Reply::redirect(308, "/c"),
        _ => Reply::ok(""),
    })
    .await;
    let mut request = HttpRequest::new("p", Method::Put, server.url("/a"));
    request.body = Body::Text("payload".into());
    send(&request).await.0.unwrap();

    let steps: Vec<_> = server
        .received()
        .iter()
        .map(|r| (r.method.clone(), r.body.clone()))
        .collect();
    assert!(
        steps.iter().all(|(m, b)| m == "PUT" && b == b"payload"),
        "{steps:?}"
    );
    assert_eq!(steps.len(), 3);
}

#[tokio::test]
async fn redirects_can_be_turned_off_or_limited() {
    let server = Server::start(|r| {
        let n: u32 = r.path.trim_start_matches('/').parse().unwrap_or(0);
        Reply::redirect(302, &format!("/{}", n + 1))
    })
    .await;

    let mut no_follow = get(server.url("/0"));
    no_follow.settings.follow_redirects = Setting::Set(false);
    let head = send(&no_follow).await.0.unwrap();
    assert_eq!(head.status, 302);
    assert!(head.headers.contains(&("location".into(), "/1".into())));

    let mut limited = get(server.url("/0"));
    limited.settings.max_redirects = Setting::Set(3);
    assert_eq!(
        send(&limited).await.0,
        Err(TransportError::TooManyRedirects(3))
    );
    // The first request and three redirects.
    assert_eq!(server.received().len(), 1 + 4);
}

#[tokio::test]
async fn credentials_do_not_follow_a_redirect_to_another_origin() {
    let other = Server::start(|_| Reply::ok("")).await;
    let target = other.url("/landing");
    let server = Server::start(move |r| match r.path.as_str() {
        "/same" => Reply::redirect(302, "/next"),
        "/next" => Reply::redirect(302, &target),
        _ => Reply::ok(""),
    })
    .await;
    let mut request = get(server.url("/same"));
    request.headers = vec![
        Header::new("Authorization", "Bearer s3cr3t"),
        Header::new("Cookie", "session=1"),
        Header::new("X-Other", "kept"),
    ];
    send(&request).await.0.unwrap();

    let same = &server.received()[1];
    assert_eq!(same.headers["authorization"], "Bearer s3cr3t");
    let landed = &other.received()[0];
    assert!(!landed.headers.contains_key("authorization"));
    assert!(!landed.headers.contains_key("cookie"));
    assert_eq!(landed.headers["x-other"], "kept");
}

#[tokio::test]
async fn the_timeout_covers_the_whole_exchange() {
    let server = Server::start(|_| Reply::ok("late").after(Duration::from_secs(5))).await;
    let mut request = get(server.url("/slow"));
    request.settings.timeout_ms = Setting::Set(200);
    let started = Instant::now();
    assert_eq!(send(&request).await.0, Err(TransportError::Timeout(200)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn cancelling_stops_a_pending_send() {
    let server = Server::start(|_| Reply::ok("late").after(Duration::from_secs(5))).await;
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let (result, _) = send_with(&transport(), &get(server.url("/slow")), &cancel).await;
    assert_eq!(result, Err(TransportError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(2));

    // Already cancelled: nothing is sent.
    let quiet = Server::start(|_| Reply::ok("")).await;
    let (result, _) = send_with(&transport(), &get(quiet.url("/")), &cancel).await;
    assert_eq!(result, Err(TransportError::Cancelled));
    assert!(quiet.received().is_empty());
}

#[tokio::test]
async fn large_bodies_arrive_in_chunks() {
    let size = 8 * 1024 * 1024;
    let server = Server::start(move |_| Reply::ok(vec![b'x'; size])).await;
    let (result, body) = send(&get(server.url("/big"))).await;
    result.unwrap();
    assert_eq!(body.body.len(), size);
    assert!(body.writes > 10, "only {} writes", body.writes);
    assert!(
        body.largest_write <= 1024 * 1024,
        "a chunk of {} bytes",
        body.largest_write
    );
}

#[tokio::test]
async fn compressed_responses_are_decompressed() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"compressed hello").unwrap();
    let gzipped = encoder.finish().unwrap();
    let server =
        Server::start(move |_| Reply::ok(gzipped.clone()).header("Content-Encoding", "gzip")).await;
    let (result, body) = send(&get(server.url("/gz"))).await;
    result.unwrap();
    assert_eq!(body.body, b"compressed hello");
    assert!(server.received()[0].headers["accept-encoding"].contains("gzip"));
}

#[tokio::test]
async fn non_utf8_response_headers_are_kept_lossily() {
    let server = Server::start(|_| Reply::ok("").header("X-Bytes", vec![b'a', 0xff, b'b'])).await;
    let head = send(&get(server.url("/"))).await.0.unwrap();
    assert!(
        head.headers
            .contains(&("x-bytes".into(), "a\u{fffd}b".into()))
    );
}

#[tokio::test]
async fn urls_without_a_scheme_use_http() {
    let server = Server::start(|_| Reply::ok("")).await;
    let head = send(&get(format!("{}/plain", server.addr)))
        .await
        .0
        .unwrap();
    assert_eq!(head.url, server.url("/plain"));
}

#[tokio::test]
async fn a_refused_connection_is_classified_and_hides_the_query() {
    // Bind and drop, so nothing listens on the port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (result, _) = send(&get(format!("http://127.0.0.1:{port}/x?token=SECRET"))).await;
    let error = result.unwrap_err();
    assert!(matches!(error, TransportError::Connect(_)), "{error:?}");
    assert!(
        error.to_string().contains(&format!("127.0.0.1:{port}")),
        "{error}"
    );
    assert!(!error.to_string().contains("SECRET"), "{error}");
}

#[tokio::test]
async fn an_unresolvable_host_is_a_dns_error() {
    // `.invalid` never resolves (RFC 2606).
    let (result, _) = send(&get("http://apifast-test.invalid/?token=SECRET".into())).await;
    let error = result.unwrap_err();
    assert!(matches!(error, TransportError::Dns(_)), "{error:?}");
    assert!(
        error.to_string().contains("apifast-test.invalid"),
        "{error}"
    );
    assert!(!error.to_string().contains("SECRET"), "{error}");
}

#[tokio::test]
async fn https_to_a_plain_server_is_a_tls_error() {
    // A server that answers plain HTTP right away, like a misconfigured port.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let _ = stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                .await;
            let _ = stream.shutdown().await;
        }
    });
    let started = Instant::now();
    let (result, _) = send(&get(format!("https://{addr}/"))).await;
    assert!(matches!(result, Err(TransportError::Tls(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn invalid_requests_fail_before_sending() {
    let (result, _) = send(&get("http://exa mple.test/".into())).await;
    assert!(
        matches!(result, Err(TransportError::InvalidUrl(_))),
        "{result:?}"
    );

    let server = Server::start(|_| Reply::ok("")).await;
    let mut bad_header = get(server.url("/"));
    bad_header.headers = vec![Header::new("bad name", "v")];
    let (result, _) = send(&bad_header).await;
    assert!(
        matches!(result, Err(TransportError::InvalidRequest(_))),
        "{result:?}"
    );
    assert!(server.received().is_empty());
}
