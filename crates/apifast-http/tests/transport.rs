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
    AppHost, BodyWriter, CancelToken, Cookies, HttpTransport, NoPrompter, RenderedRequest,
    ResponseHead, StoreError, StoredBody, TimelineKind, TransportError, render_request,
};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, Body, Chain, Collection, FormField, Header,
    HttpRequest, Method, MultipartPart, MultipartValue, ResolvedVariables, Setting, resolve,
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

/// The events of the exchange, without those of the connection.
fn exchange_only(timeline: &[apifast_app::TimelineEvent]) -> Vec<TimelineKind> {
    (timeline.iter())
        .map(|e| e.kind.clone())
        .filter(|k| {
            !matches!(
                k,
                TimelineKind::Dns { .. }
                    | TimelineKind::Connected { .. }
                    | TimelineKind::Reused { .. }
                    | TimelineKind::Tls { .. }
            )
        })
        .collect()
}

/// A server that keeps each connection open, answering `ok` to every
/// request without body on it, and counts the connections.
async fn keep_alive_server() -> (u16, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(Mutex::new(0));
    let counted = connections.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            *counted.lock().unwrap() += 1;
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    while let Some(end) = find(&buffer, b"\r\n\r\n") {
                        buffer.drain(..end + 4);
                        let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                        if stream.write_all(reply).await.is_err() {
                            return;
                        }
                    }
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                }
            });
        }
    });
    (port, connections)
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
async fn a_large_file_body_arrives_whole_and_a_part_may_live_outside_the_collection() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..8 * 1024 * 1024_u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    std::fs::write(dir.path().join("big.bin"), &big).unwrap();
    let elsewhere = outside.path().join("note.txt");
    std::fs::write(&elsewhere, b"from elsewhere").unwrap();
    let transport = ReqwestTransport::new(dir.path()).unwrap();
    let server = Server::start(|_| Reply::ok("")).await;

    let mut file = HttpRequest::new("f", Method::Put, server.url("/file"));
    file.body = Body::File {
        path: "big.bin".into(),
        content_type: None,
    };
    send_with(&transport, &file, &CancelToken::new())
        .await
        .0
        .unwrap();
    let mut multipart = HttpRequest::new("m", Method::Post, server.url("/multi"));
    multipart.body = Body::Multipart(vec![MultipartPart {
        name: "doc".into(),
        value: MultipartValue::Files(vec![elsewhere.display().to_string()]),
        content_type: None,
        enabled: true,
    }]);
    send_with(&transport, &multipart, &CancelToken::new())
        .await
        .0
        .unwrap();

    let received = server.received();
    assert_eq!(received[0].body.len(), big.len());
    assert!(received[0].body == big, "the bytes arrive as they are");
    // No type given: Bruno's default.
    assert_eq!(
        received[0].headers["content-type"],
        "application/octet-stream"
    );
    let body = String::from_utf8_lossy(&received[1].body);
    assert!(
        body.contains("filename=\"note.txt\"") && body.contains("from elsewhere"),
        "{body}"
    );
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
    let kinds = exchange_only(&head.timeline);
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
    let wire = gzipped.len() as u64;
    let server =
        Server::start(move |_| Reply::ok(gzipped.clone()).header("Content-Encoding", "gzip")).await;
    let (result, body) = send(&get(server.url("/gz"))).await;
    let head = result.unwrap();
    assert_eq!(body.body, b"compressed hello");
    assert_eq!(
        server.received()[0].headers["accept-encoding"],
        "zstd,gzip,deflate,br"
    );
    // Both sizes: the body as stored, and what came over the wire.
    let kinds = exchange_only(&head.timeline);
    let tail = &kinds[kinds.len() - 2..];
    assert_eq!(
        tail,
        [
            TimelineKind::Body { bytes: 16 },
            TimelineKind::Decompressed {
                encoding: "gzip".into(),
                wire_bytes: wire,
            }
        ]
    );
    // The server's headers are kept as they came.
    assert!(
        head.headers
            .contains(&("content-encoding".into(), "gzip".into()))
    );

    // The request's own Accept-Encoding is kept; with a range, none is added.
    let mut own = get(server.url("/own"));
    own.headers = vec![Header::new("Accept-Encoding", "gzip")];
    send(&own).await.0.unwrap();
    let mut range = get(server.url("/range"));
    range.headers = vec![Header::new("Range", "bytes=0-9")];
    send(&range).await.0.unwrap();
    let received = server.received();
    assert_eq!(received[1].headers["accept-encoding"], "gzip");
    assert!(!received[2].headers.contains_key("accept-encoding"));
}

#[tokio::test]
async fn a_broken_compressed_body_is_a_protocol_error_and_unknown_encodings_stay() {
    let server = Server::start(|r| {
        if r.path == "/broken" {
            Reply::ok("not gzip").header("Content-Encoding", "gzip")
        } else {
            Reply::ok("raw").header("Content-Encoding", "compress")
        }
    })
    .await;
    let error = send(&get(server.url("/broken"))).await.0.unwrap_err();
    assert!(matches!(error, TransportError::Protocol(_)), "{error:?}");
    assert!(error.to_string().contains("not valid gzip"), "{error}");
    let (result, body) = send(&get(server.url("/other"))).await;
    let head = result.unwrap();
    assert_eq!(body.body, b"raw");
    assert!(
        !head
            .timeline
            .iter()
            .any(|e| matches!(e.kind, TimelineKind::Decompressed { .. }))
    );
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
async fn encode_url_encodes_as_bruno_and_off_sends_the_url_as_written() {
    let server = Server::start(|_| Reply::ok("")).await;
    let url = server.url("/a b/c%20d?q=a,b&r=%2F");

    let mut on = get(url.clone());
    on.auth = Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
        key: "api key".into(),
        value: "k/1".into(),
        placement: ApiKeyPlacement::Query,
    }));
    send(&on).await.0.unwrap();
    let mut off = get(url);
    off.settings.encode_url = Setting::Set(false);
    send(&off).await.0.unwrap();

    let received = server.received();
    // The API key is added after the URL is encoded: encoded once.
    assert_eq!(
        received[0].path,
        "/a%20b/c%20d?q=a%2Cb&r=%252F&api%20key=k%2F1"
    );
    // Only the URL parser's normalisation, as axios in Bruno.
    assert_eq!(received[1].path, "/a%20b/c%20d?q=a,b&r=%2F");
}

#[tokio::test]
async fn cookies_set_on_a_redirect_go_with_the_next_requests_to_their_host() {
    let server = Server::start(|r| match r.path.as_str() {
        "/login" => Reply::redirect(302, "/me")
            .header("Set-Cookie", "sid=abc; Path=/; HttpOnly")
            .header("Set-Cookie", "theme=dark"),
        "/away" => Reply::redirect(302, &r.headers["x-away"]),
        _ => Reply::ok(""),
    })
    .await;
    let cookies = Arc::new(Cookies::new());
    let transport = transport().with_cookies(cookies.clone());
    let cancel = CancelToken::new();

    send_with(&transport, &get(server.url("/login")), &cancel)
        .await
        .0
        .unwrap();
    let mut written = get(server.url("/me"));
    written.headers = vec![Header::new("Cookie", "theme=light; mine=1")];
    send_with(&transport, &written, &cancel).await.0.unwrap();
    // The same server under another host name gets none.
    let mut away = get(server.url("/away"));
    let other_host = server.url("/elsewhere").replace("127.0.0.1", "localhost");
    away.headers = vec![Header::new("X-Away", &other_host)];
    send_with(&transport, &away, &cancel).await.0.unwrap();

    let received = server.received();
    assert!(!received[0].headers.contains_key("cookie"));
    assert_eq!(received[1].path, "/me");
    assert_eq!(received[1].headers["cookie"], "sid=abc; theme=dark");
    // The jar's value wins over the written one, as in Bruno.
    assert_eq!(received[2].headers["cookie"], "theme=dark; mine=1; sid=abc");
    assert_eq!(received[3].headers["cookie"], "sid=abc; theme=dark");
    assert_eq!(received[4].path, "/elsewhere");
    assert!(!received[4].headers.contains_key("cookie"));
    assert_eq!(cookies.jar().cookies().len(), 2);
}

#[tokio::test]
async fn cookies_are_neither_sent_nor_kept_when_turned_off() {
    let server = Server::start(|_| Reply::ok("").header("Set-Cookie", "fresh=1")).await;
    let cookies = Arc::new(Cookies::new());
    cookies.receive(
        ["old=1"],
        &apifast_domain::CookieTarget::new("http", "127.0.0.1", "/"),
        i64::MAX / 2,
    );
    cookies.set_policy(false, false);
    let transport = transport().with_cookies(cookies.clone());
    send_with(&transport, &get(server.url("/")), &CancelToken::new())
        .await
        .0
        .unwrap();
    assert!(!server.received()[0].headers.contains_key("cookie"));
    let names: Vec<String> = cookies
        .jar()
        .cookies()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["old"]);
}

#[tokio::test]
async fn the_timeline_shows_dns_and_the_connection_then_its_reuse() {
    let (port, connections) = keep_alive_server().await;
    let transport = transport();
    let url = format!("http://localhost:{port}/a");
    let cancel = CancelToken::new();
    let (first, _) = send_with(&transport, &get(url.clone()), &cancel).await;
    let (second, _) = send_with(&transport, &get(url), &cancel).await;
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_eq!(
        *connections.lock().unwrap(),
        1,
        "the pool keeps the connection"
    );

    let kinds = |head: &ResponseHead| -> Vec<TimelineKind> {
        head.timeline.iter().map(|e| e.kind.clone()).collect()
    };
    let first_kinds = kinds(&first);
    let TimelineKind::Dns {
        host, addresses, ..
    } = &first_kinds[1]
    else {
        panic!("{first_kinds:?}");
    };
    assert_eq!(host, "localhost");
    assert!(addresses.iter().any(|a| a == "127.0.0.1"), "{addresses:?}");
    let TimelineKind::Connected { address, .. } = &first_kinds[2] else {
        panic!("{first_kinds:?}");
    };
    let address = address.clone().unwrap();
    assert!(address.ends_with(&format!(":{port}")), "{address}");
    assert!(
        !first_kinds
            .iter()
            .any(|k| matches!(k, TimelineKind::Tls { .. }))
    );
    // The events are in time order.
    assert!(first.timeline.windows(2).all(|w| w[0].at_us <= w[1].at_us));

    // No lookup and no new connection the second time.
    assert_eq!(
        kinds(&second)[1],
        TimelineKind::Reused {
            address: Some(address)
        }
    );
    assert!(
        !kinds(&second)
            .iter()
            .any(|k| matches!(k, TimelineKind::Dns { .. }))
    );
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

#[tokio::test]
async fn aws_signature_v4_signs_each_request_to_the_same_origin() {
    use apifast_domain::{AuthScheme, AwsV4Auth, Setting};
    let other = Server::start(|_| Reply::ok("")).await;
    let target = other.url("/elsewhere");
    let server = Server::start(
        move |r| match r.path.split('?').next().unwrap_or_default() {
            "/start" => Reply::redirect(307, "/signed"),
            "/signed" => Reply::redirect(307, &target),
            _ => Reply::ok(""),
        },
    )
    .await;
    let mut request = HttpRequest::new("s", Method::Post, server.url("/start?b=2&a=1"));
    request.body = Body::Json(r#"{"a":1}"#.into());
    request.auth = Setting::Set(AuthScheme::AwsV4(Box::new(AwsV4Auth {
        access_key_id: "AKIDEXAMPLE".into(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
        session_token: "the-token".into(),
        service: "execute-api".into(),
        region: "eu-west-1".into(),
        profile_name: String::new(),
    })));
    send(&request).await.0.unwrap();

    let received = server.received();
    for hop in &received {
        let authorization = &hop.headers["authorization"];
        assert!(
            authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
            "{authorization}"
        );
        assert!(authorization.contains("/eu-west-1/execute-api/aws4_request, "));
        assert!(authorization.contains(
            "SignedHeaders=content-type;host;x-amz-date;x-amz-security-token, Signature="
        ));
        let date = &hop.headers["x-amz-date"];
        assert_eq!(date.len(), 16, "{date}");
        assert!(date.ends_with('Z') && date.as_bytes()[8] == b'T', "{date}");
        assert_eq!(hop.headers["x-amz-security-token"], "the-token");
        // The secret key itself never travels.
        assert!(!format!("{:?}", hop.headers).contains("EXAMPLEKEY"));
    }
    // The second hop has another path, so another signature.
    assert_ne!(
        received[0].headers["authorization"],
        received[1].headers["authorization"]
    );
    // Another origin gets no signature.
    let landed = &other.received()[0];
    assert!(!landed.headers.contains_key("authorization"));
    assert!(!landed.headers.contains_key("x-amz-security-token"));
    assert!(!landed.headers.contains_key("x-amz-date"));
}

#[test]
fn aws_credentials_never_show_in_debug_output() {
    use apifast_domain::{AuthScheme, AwsV4Auth, Setting};
    let mut request = get("http://h/".into());
    request.auth = Setting::Set(AuthScheme::AwsV4(Box::new(AwsV4Auth {
        access_key_id: "AKID".into(),
        secret_access_key: "very-secret".into(),
        session_token: "token-too".into(),
        service: "s3".into(),
        region: "us-east-1".into(),
        profile_name: String::new(),
    })));
    let shown = format!("{:?}", rendered(&request));
    assert!(shown.contains("AKID"), "{shown}");
    assert!(
        !shown.contains("very-secret") && !shown.contains("token-too"),
        "{shown}"
    );
}

/// MD5 in hex, for the test's own Digest server.
fn md5_hex(text: &str) -> String {
    use md5::{Digest, Md5};
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in Md5::digest(text.as_bytes()) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A Digest server (RFC 2617, MD5, qop=auth) written apart from the
/// transport's code: it challenges, then checks the answer itself.
fn digest_server(r: &Received) -> Reply {
    const NONCE: &str = "dcd98b7102dd2f0e8b11d0f600bfb0c093";
    let challenge = || {
        Reply::status(401)
            .header(
                "WWW-Authenticate",
                format!("Digest realm=\"apifast\", qop=\"auth\", nonce=\"{NONCE}\", opaque=\"op\""),
            )
            .header("WWW-Authenticate", "Basic realm=\"apifast\"")
    };
    let Some(header) = r.headers.get("authorization") else {
        return challenge();
    };
    let field = |name: &str| {
        header
            .split(", ")
            .find_map(|part| {
                part.trim_start_matches("Digest ")
                    .strip_prefix(&format!("{name}="))
            })
            .map(|v| v.trim_matches('"').to_owned())
            .unwrap_or_default()
    };
    let ha1 = md5_hex("ana:apifast:s3cret");
    let ha2 = md5_hex(&format!("{}:{}", r.method, field("uri")));
    let expected = md5_hex(&format!(
        "{ha1}:{NONCE}:{}:{}:auth:{ha2}",
        field("nc"),
        field("cnonce")
    ));
    if field("uri") == r.path && field("opaque") == "op" && field("response") == expected {
        Reply::ok("welcome")
    } else {
        challenge()
    }
}

fn with_digest(url: String, password: &str) -> HttpRequest {
    use apifast_domain::{AuthScheme, DigestAuth, Setting};
    let mut request = HttpRequest::new("d", Method::Post, url);
    request.body = Body::Json(r#"{"a":1}"#.into());
    request.auth = Setting::Set(AuthScheme::Digest(DigestAuth {
        username: "ana".into(),
        password: password.into(),
    }));
    request
}

#[tokio::test]
async fn digest_answers_the_challenge_and_sends_the_body_again() {
    let server = Server::start(digest_server).await;
    let (result, body) = send(&with_digest(server.url("/private?x=1"), "s3cret")).await;
    let head = result.unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(body.body, b"welcome");
    let received = server.received();
    assert_eq!(received.len(), 2, "the challenge, then the answer");
    assert!(!received[0].headers.contains_key("authorization"));
    assert_eq!(received[1].body, br#"{"a":1}"#, "the body goes again");
    // The timeline shows both requests and the 401 between them.
    let kinds: Vec<String> = exchange_only(&head.timeline)
        .iter()
        .map(|k| format!("{k:?}"))
        .collect();
    assert!(kinds[1].contains("status: 401"), "{kinds:?}");
}

#[tokio::test]
async fn a_wrong_digest_password_ends_in_the_401_without_looping() {
    let server = Server::start(digest_server).await;
    let (result, _) = send(&with_digest(server.url("/private"), "wrong")).await;
    assert_eq!(result.unwrap().status, 401);
    assert_eq!(server.received().len(), 2, "answered once, not again");
}

// --- NTLM -------------------------------------------------------------------

/// What the NTLM server saw: for each request, its connection number and
/// what it was.
type NtlmLog = Arc<Mutex<Vec<(usize, &'static str)>>>;

const NTLM_NONCE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// An NTLM server written apart from the transport's code. It challenges a
/// Negotiate, and accepts an Authenticate only on the connection that got
/// the challenge, checking the NTLMv2 proof itself.
async fn ntlm_server() -> (String, NtlmLog) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let log: NtlmLog = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        let mut connection = 0;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            connection += 1;
            tokio::spawn(ntlm_connection(stream, connection, seen.clone()));
        }
    });
    (format!("http://{addr}"), log)
}

async fn ntlm_connection(mut stream: TcpStream, connection: usize, log: NtlmLog) {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut challenged = false;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        // One request: its head, then as many body bytes as it says.
        let head_end = loop {
            if let Some(i) = find(&buffer, b"\r\n\r\n") {
                break i;
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            }
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_lowercase();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        while buffer.len() < head_end + 4 + length {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            }
        }
        let raw = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        buffer.drain(..head_end + 4 + length);
        let token = raw
            .lines()
            .find_map(|l| {
                l.strip_prefix("authorization: NTLM ")
                    .or_else(|| l.strip_prefix("Authorization: NTLM "))
            })
            .and_then(|t| b64.decode(t.trim()).ok());
        let reply = match token.as_deref().map(|m| (m.get(8).copied(), m)) {
            Some((Some(1), _)) => {
                log.lock().unwrap().push((connection, "negotiate"));
                challenged = true;
                let challenge = b64.encode(ntlm_challenge());
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM {challenge}\r\nContent-Length: 0\r\n\r\n"
                )
            }
            Some((Some(3), message)) if challenged && ntlm_proof_holds(message) => {
                log.lock().unwrap().push((connection, "authenticated"));
                "HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nwelcome".to_owned()
            }
            Some((Some(3), _)) => {
                log.lock().unwrap().push((connection, "rejected"));
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM\r\nContent-Length: 0\r\n\r\n"
                    .to_owned()
            }
            _ => {
                log.lock().unwrap().push((connection, "anonymous"));
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM\r\nContent-Length: 0\r\n\r\n"
                    .to_owned()
            }
        };
        if stream.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
    }
}

/// A Challenge message with `NTLM_NONCE` and no target info.
fn ntlm_challenge() -> Vec<u8> {
    let mut message = b"NTLMSSP\0".to_vec();
    message.extend_from_slice(&2u32.to_le_bytes());
    message.extend_from_slice(&[0; 8]);
    message.extend_from_slice(&0xa208_8207u32.to_le_bytes());
    message.extend_from_slice(&NTLM_NONCE);
    message.extend_from_slice(&[0; 16]);
    message
}

/// Checks the NTLMv2 proof of an Authenticate message for user `ana`,
/// password `s3cret`, domain `CORP`.
fn ntlm_proof_holds(message: &[u8]) -> bool {
    use hmac::{Hmac, KeyInit, Mac};
    use md4::{Digest, Md4};
    let field = |at: usize| {
        let len = usize::from(u16::from_le_bytes([message[at], message[at + 1]]));
        let offset = u32::from_le_bytes(message[at + 4..at + 8].try_into().unwrap()) as usize;
        message
            .get(offset..offset + len)
            .unwrap_or_default()
            .to_vec()
    };
    let utf16 = |t: &str| -> Vec<u8> { t.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let hmac = |key: &[u8], parts: &[&[u8]]| {
        let mut mac = Hmac::<md5::Md5>::new_from_slice(key).unwrap();
        for part in parts {
            mac.update(part);
        }
        mac.finalize().into_bytes().to_vec()
    };
    let nt = field(20);
    if field(36) != utf16("ana") || field(28) != utf16("CORP") || nt.len() < 16 {
        return false;
    }
    let nt_hash = Md4::digest(utf16("s3cret"));
    let key = hmac(&nt_hash, &[&utf16("ANACORP")]);
    hmac(&key, &[&NTLM_NONCE, &nt[16..]]) == nt[..16]
}

fn with_ntlm(url: String, username: &str, password: &str, domain: &str) -> HttpRequest {
    use apifast_domain::{AuthScheme, NtlmAuth, Setting};
    let mut request = HttpRequest::new("n", Method::Post, url);
    request.body = Body::Json(r#"{"a":1}"#.into());
    request.auth = Setting::Set(AuthScheme::Ntlm(NtlmAuth {
        username: username.into(),
        password: password.into(),
        domain: domain.into(),
    }));
    request
}

#[tokio::test]
async fn ntlm_authenticates_on_one_connection() {
    let (base, log) = ntlm_server().await;
    let (result, body) = send(&with_ntlm(format!("{base}/api"), "ana", "s3cret", "CORP")).await;
    assert_eq!(result.unwrap().status, 200);
    assert_eq!(body.body, b"welcome");
    // Both messages on the first connection: the server would reject the
    // answer on another.
    assert_eq!(
        *log.lock().unwrap(),
        [(1, "negotiate"), (1, "authenticated")]
    );

    // `DOMAIN\user` gives the domain too.
    let (base, log) = ntlm_server().await;
    let (result, _) = send(&with_ntlm(format!("{base}/api"), "CORP\\ana", "s3cret", "")).await;
    assert_eq!(result.unwrap().status, 200);
    assert_eq!(log.lock().unwrap().last(), Some(&(1, "authenticated")));
}

#[tokio::test]
async fn a_wrong_ntlm_password_ends_in_the_401() {
    let (base, log) = ntlm_server().await;
    let (result, _) = send(&with_ntlm(format!("{base}/api"), "ana", "wrong", "CORP")).await;
    assert_eq!(result.unwrap().status, 401);
    assert_eq!(*log.lock().unwrap(), [(1, "negotiate"), (1, "rejected")]);
}

// --- OAuth 2.0 loopback -------------------------------------------------------

/// A port free right now.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A browser that asks for a favicon, then follows the redirect to `to`.
fn fake_browser(to: String) -> impl Fn(&str) -> std::io::Result<()> + Send + Sync {
    move |_url: &str| {
        let to = to.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (host, path) = to.trim_start_matches("http://").split_once('/').unwrap();
            for target in ["/favicon.ico".to_owned(), format!("/{path}")] {
                let mut stream = std::net::TcpStream::connect(host).unwrap();
                write!(stream, "GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
                let mut reply = String::new();
                stream.read_to_string(&mut reply).ok();
            }
        });
        Ok(())
    }
}

#[tokio::test]
async fn the_loopback_catches_the_redirect_and_its_parameters() {
    use apifast_app::Authorizer as _;
    let port = free_port();
    let callback = format!("http://localhost:{port}/callback");
    let browser = fake_browser(format!("127.0.0.1:{port}/callback?code=c%201&state=s"));
    let authorizer = apifast_http::LoopbackAuthorizer::new(Box::new(browser));
    let answer = authorizer
        .authorize("http://auth/authorize?x", &callback, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(
        answer,
        [
            ("code".to_owned(), "c 1".to_owned()),
            ("state".to_owned(), "s".to_owned())
        ]
    );
}

#[tokio::test]
async fn the_loopback_refuses_remote_callbacks_and_gives_up_in_time() {
    use apifast_app::Authorizer as _;
    let authorizer = apifast_http::LoopbackAuthorizer::new(Box::new(|_: &str| Ok(())))
        .waiting(Duration::from_millis(100));
    let error = authorizer
        .authorize(
            "http://auth/a",
            "https://app.example.com/cb",
            &CancelToken::new(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("must be http://localhost"), "{error}");
    let callback = format!("http://127.0.0.1:{}/cb", free_port());
    let error = authorizer
        .authorize("http://auth/a", &callback, &CancelToken::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("in time"), "{error}");
}

// --- Proxies ------------------------------------------------------------------

/// An HTTP proxy that answers every request itself, recording its request
/// line and `Proxy-Authorization`.
async fn http_proxy() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut chunk = [0u8; 4096];
                while find(&head, b"\r\n\r\n").is_none() {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&chunk[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&head).into_owned();
                let line = text.lines().next().unwrap_or_default().to_owned();
                let auth = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("proxy-authorization:"))
                    .and_then(|l| l.split_once(':'))
                    .map(|(_, v)| v.trim().to_owned())
                    .unwrap_or_default();
                seen.lock().unwrap().push(format!("{line} | {auth}"));
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nvia proxy")
                    .await;
            });
        }
    });
    (port, log)
}

fn through(proxy: apifast_domain::ProxyConfig) -> ReqwestTransport {
    ReqwestTransport::with_proxy(
        std::env::temp_dir(),
        apifast_domain::ProxyChoice::Manual(proxy),
    )
    .unwrap()
}

#[tokio::test]
async fn requests_go_through_a_manual_proxy_with_its_credentials() {
    let (port, log) = http_proxy().await;
    let transport = through(apifast_domain::ProxyConfig {
        hostname: "127.0.0.1".into(),
        port: port.to_string(),
        auth: true,
        username: "ana".into(),
        password: "s3cret".into(),
        ..apifast_domain::ProxyConfig::default()
    });
    let (result, body) = send_with(
        &transport,
        &get("http://api.example.invalid/orders?x=1".into()),
        &CancelToken::new(),
    )
    .await;
    assert_eq!(result.unwrap().status, 200);
    assert_eq!(body.body, b"via proxy");
    assert_eq!(
        *log.lock().unwrap(),
        // ana:s3cret in Basic.
        ["GET http://api.example.invalid/orders?x=1 HTTP/1.1 | Basic YW5hOnMzY3JldA=="]
    );
}

#[tokio::test]
async fn excluded_hosts_go_direct() {
    let (port, log) = http_proxy().await;
    let server = Server::start(|_| Reply::ok("direct")).await;
    let transport = through(apifast_domain::ProxyConfig {
        hostname: "127.0.0.1".into(),
        port: port.to_string(),
        bypass: "localhost, 127.0.0.1".into(),
        ..apifast_domain::ProxyConfig::default()
    });
    let (result, body) = send_with(&transport, &get(server.url("/x")), &CancelToken::new()).await;
    assert_eq!(result.unwrap().status, 200);
    assert_eq!(body.body, b"direct");
    assert_eq!(*log.lock().unwrap(), Vec::<String>::new());
}

/// A SOCKS5 proxy without authentication that connects to whatever it is
/// asked and relays. Counts the connections it made.
async fn socks5_proxy() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut greeting = [0u8; 2];
                client.read_exact(&mut greeting).await.unwrap();
                let mut methods = vec![0u8; usize::from(greeting[1])];
                client.read_exact(&mut methods).await.unwrap();
                client.write_all(&[5, 0]).await.unwrap();
                let mut request = [0u8; 4];
                client.read_exact(&mut request).await.unwrap();
                let host = match request[3] {
                    1 => {
                        let mut ip = [0u8; 4];
                        client.read_exact(&mut ip).await.unwrap();
                        std::net::Ipv4Addr::from(ip).to_string()
                    }
                    3 => {
                        let mut len = [0u8; 1];
                        client.read_exact(&mut len).await.unwrap();
                        let mut name = vec![0u8; usize::from(len[0])];
                        client.read_exact(&mut name).await.unwrap();
                        String::from_utf8(name).unwrap()
                    }
                    _ => return,
                };
                let mut port = [0u8; 2];
                client.read_exact(&mut port).await.unwrap();
                let port = u16::from_be_bytes(port);
                seen.lock().unwrap().push(format!("{host}:{port}"));
                let mut upstream = TcpStream::connect((host.as_str(), port)).await.unwrap();
                client
                    .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            });
        }
    });
    (port, log)
}

#[tokio::test]
async fn socks5_proxies_relay_the_connection() {
    let (port, log) = socks5_proxy().await;
    let server = Server::start(|_| Reply::ok("over socks")).await;
    let transport = through(apifast_domain::ProxyConfig {
        protocol: apifast_domain::ProxyProtocol::Socks5,
        hostname: "127.0.0.1".into(),
        port: port.to_string(),
        ..apifast_domain::ProxyConfig::default()
    });
    let (result, body) = send_with(&transport, &get(server.url("/x")), &CancelToken::new()).await;
    assert_eq!(result.unwrap().status, 200);
    assert_eq!(body.body, b"over socks");
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn a_proxy_without_host_is_refused() {
    let error = ReqwestTransport::with_proxy(
        std::env::temp_dir(),
        apifast_domain::ProxyChoice::Manual(apifast_domain::ProxyConfig::default()),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "invalid request: the proxy has no host");
}
