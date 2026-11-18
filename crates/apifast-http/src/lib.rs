//! HTTP transport of apifast: sends a [`RenderedRequest`] with reqwest and
//! rustls, follows redirects by hand, and streams the response body into a
//! [`BodyWriter`] without holding it in memory.
//!
//! Must run inside a tokio runtime with the time driver enabled.

mod cookies;
mod digest;
mod loopback;
mod ntlm;
mod probe;
mod sigv4;
mod tls;

use std::error::Error as _;
use std::sync::Arc;

use base64::Engine as _;

pub use loopback::{LoopbackAuthorizer, open_in_browser};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use apifast_app::{
    BodyWriter, CancelToken, Cookies, HttpTransport, RenderedBody, RenderedRequest, ResponseHead,
    TimelineEvent, TimelineKind, TransportAuth, TransportError,
};
use apifast_domain::{ClientCertificate, MultipartValue, ProxyChoice, ProxyConfig, TlsTrust};
use reqwest::header::{
    AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName, HeaderValue,
    LOCATION, USER_AGENT,
};
use reqwest::{Method, Url};

/// Sent when the request does not set its own `User-Agent`.
const DEFAULT_USER_AGENT: &str = concat!("apifast/", env!("CARGO_PKG_VERSION"));

/// How often a pending send checks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// The real [`HttpTransport`].
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
    files_root: PathBuf,
    settings: tls::ClientSettings,
    /// A client per client certificate, or why it could not be made.
    certified: Vec<Certified>,
    /// The collection's cookie jar, if it sends and keeps cookies.
    cookies: Option<Arc<Cookies>>,
}

/// A client certificate and the client that presents it.
#[derive(Debug, Clone)]
struct Certified {
    certificate: ClientCertificate,
    client: Result<(reqwest::Client, tls::Identity), String>,
}

impl ReqwestTransport {
    /// Creates a transport. Relative paths of file bodies and multipart files
    /// are resolved against `files_root`, normally the collection folder.
    ///
    /// # Errors
    ///
    /// If the TLS configuration cannot be built (for example, the platform's
    /// certificate store is unavailable).
    pub fn new(files_root: impl Into<PathBuf>) -> Result<Self, TransportError> {
        Self::with_proxy(files_root, ProxyChoice::System)
    }

    /// As [`ReqwestTransport::new`], through `proxy`.
    ///
    /// # Errors
    ///
    /// If the TLS configuration cannot be built, or the proxy is not valid.
    pub fn with_proxy(
        files_root: impl Into<PathBuf>,
        proxy: ProxyChoice,
    ) -> Result<Self, TransportError> {
        Self::configured(files_root, proxy, &TlsTrust::default(), &[])
    }

    /// As [`ReqwestTransport::with_proxy`], trusting as `trust` says and
    /// presenting `certificates` to their hosts. A certificate whose files
    /// are wrong does not stop the others: sending to its host fails with
    /// why.
    ///
    /// # Errors
    ///
    /// If the TLS configuration cannot be built, a CA file is wrong, or the
    /// proxy is not valid.
    pub fn configured(
        files_root: impl Into<PathBuf>,
        proxy: ProxyChoice,
        trust: &TlsTrust,
        certificates: &[ClientCertificate],
    ) -> Result<Self, TransportError> {
        // reqwest is built without a default crypto provider so that ring is
        // used everywhere (it needs no cmake on Windows). Installing fails only
        // when a provider is already installed, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let files_root = files_root.into();
        let settings = tls::ClientSettings::new(proxy, trust)?;
        let client = build_client(reqwest::Client::builder(), &settings, None, false)?;
        let certified = (certificates.iter())
            .filter(|c| c.enabled)
            .map(|certificate| Certified {
                certificate: certificate.clone(),
                client: tls::identity(&files_root, certificate).and_then(|identity| {
                    build_client(
                        reqwest::Client::builder(),
                        &settings,
                        Some(identity.clone()),
                        false,
                    )
                    .map(|client| (client, identity))
                    .map_err(|e| e.to_string())
                }),
            })
            .collect();
        Ok(Self {
            client,
            files_root,
            settings,
            certified,
            cookies: None,
        })
    }

    /// The same transport, sending cookies from `cookies` and keeping in it
    /// those that responses set.
    #[must_use]
    pub fn with_cookies(mut self, cookies: Arc<Cookies>) -> Self {
        self.cookies = Some(cookies);
        self
    }

    /// The client certificate for `url`'s host, if one is for it.
    fn certified_for(&self, url: &Url) -> Option<&Certified> {
        let host = url.host_str()?;
        let port = url.port_or_known_default()?;
        (self.certified.iter()).find(|c| c.certificate.matches(host, port))
    }

    /// The client for `url`: the one with its host's certificate, or the
    /// default.
    ///
    /// # Errors
    ///
    /// If its host's certificate could not be loaded.
    fn client_for(&self, url: &Url) -> Result<&reqwest::Client, TransportError> {
        match self.certified_for(url) {
            Some(Certified {
                client: Ok((client, _)),
                ..
            }) => Ok(client),
            Some(Certified { client: Err(e), .. }) => Err(TransportError::Tls(e.clone())),
            None => Ok(&self.client),
        }
    }

    async fn exchange<W: BodyWriter + Send>(
        &self,
        ntlm: Option<&reqwest::Client>,
        request: &RenderedRequest,
        writer: &mut W,
        started: Instant,
    ) -> Result<ResponseHead, TransportError> {
        let (mut url, mut method, mut headers) = first_request(request)?;
        start_handshake(request, &mut headers);
        let mut send_body = true;
        let mut redirects = 0;
        let first_origin = origin(&url);
        // The URL whose Digest challenge was answered: answered once.
        let mut answered: Option<Url> = None;
        let mut timeline = Vec::new();
        let at = || u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);

        loop {
            let sent = Outgoing {
                method: &method,
                url: &url,
                send_body,
                first_origin: &first_origin,
            };
            let sent_headers = signed_headers(request, &headers, &sent)?;
            let response = self
                .hop(
                    ntlm,
                    request,
                    (&sent, sent_headers),
                    (started, &mut timeline),
                )
                .await?;
            let status = response.status().as_u16();

            let response = if status == 401 && answered.as_ref() != Some(&url) {
                match answer_challenge(request, response, &sent).await? {
                    Challenged::Answer(answer) => {
                        // Sent again, body and all, with the answer.
                        headers.insert(AUTHORIZATION, answer);
                        answered = Some(url.clone());
                        timeline.push(TimelineEvent {
                            at_us: at(),
                            kind: TimelineKind::Headers { status },
                        });
                        continue;
                    }
                    Challenged::No(response) => response,
                }
            } else {
                response
            };

            if request.follow_redirects()
                && matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response.headers().get(LOCATION)
            {
                if redirects >= request.max_redirects() {
                    return Err(TransportError::TooManyRedirects(redirects));
                }
                let (next, location) = redirect_target(&url, location)?;
                timeline.push(TimelineEvent {
                    at_us: at(),
                    kind: TimelineKind::Redirect { status, location },
                });
                if matches!(status, 301..=303) {
                    // Browsers and Bruno turn these into a GET without body.
                    as_get(&mut method, &mut headers);
                    send_body = false;
                }
                if origin(&next) != origin(&url) {
                    // Credentials for one origin must not leak to another.
                    headers.remove(AUTHORIZATION);
                    headers.remove(COOKIE);
                }
                url = next;
                redirects += 1;
                if origin(&url) == first_origin {
                    start_handshake(request, &mut headers);
                }
                continue;
            }

            let mut head = response_head(&response, redirects);
            timeline.push(TimelineEvent {
                at_us: at(),
                kind: TimelineKind::Headers { status },
            });
            let bytes = stream_body(response, writer, &url).await?;
            timeline.push(TimelineEvent {
                at_us: at(),
                kind: TimelineKind::Body { bytes },
            });
            head.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            head.timeline = timeline;
            return Ok(head);
        }
    }

    /// Sends one request of the exchange, `sent` with its `headers`, and
    /// notes it and its connection in `timeline`. Cookies go from the jar
    /// and come back to it.
    async fn hop(
        &self,
        ntlm: Option<&reqwest::Client>,
        request: &RenderedRequest,
        (sent, mut headers): (&Outgoing<'_>, HeaderMap),
        (started, timeline): (Instant, &mut Vec<TimelineEvent>),
    ) -> Result<reqwest::Response, TransportError> {
        let url = sent.url;
        if let Some(jar) = &self.cookies {
            cookies::add_from_jar(jar, url, &mut headers);
        }
        let client = match ntlm {
            Some(client) => client,
            None => self.client_for(url)?,
        };
        let mut builder = client
            .request(sent.method.clone(), url.clone())
            .headers(headers);
        if sent.send_body {
            builder = self.attach_body(builder, request.body()).await?;
        }
        timeline.push(TimelineEvent {
            at_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            kind: TimelineKind::Request {
                method: sent.method.as_str().to_owned(),
                url: url.to_string(),
            },
        });
        let (result, notes) = probe::Probe::watch(builder.send()).await;
        let response = result.map_err(|e| classify(&e, url))?;
        connection_events(&notes, &response, started, timeline);
        if let Some(jar) = &self.cookies {
            cookies::keep(jar, url, response.headers());
        }
        Ok(response)
    }

    async fn attach_body(
        &self,
        builder: reqwest::RequestBuilder,
        body: &RenderedBody,
    ) -> Result<reqwest::RequestBuilder, TransportError> {
        Ok(match body {
            RenderedBody::Text(text) => builder.body(text.clone()),
            RenderedBody::Form(fields) => builder.body(form(fields)),
            RenderedBody::Multipart(parts) => {
                let mut form = reqwest::multipart::Form::new();
                for part in parts {
                    let pieces = match &part.value {
                        MultipartValue::Text(text) => {
                            vec![reqwest::multipart::Part::text(text.clone())]
                        }
                        MultipartValue::Files(paths) => {
                            let mut files = Vec::with_capacity(paths.len());
                            for path in paths {
                                let full = self.resolve(path);
                                files.push(
                                    reqwest::multipart::Part::file(&full)
                                        .await
                                        .map_err(|e| unreadable(path, &e))?,
                                );
                            }
                            files
                        }
                    };
                    for mut piece in pieces {
                        if let Some(content_type) = &part.content_type {
                            piece = piece.mime_str(content_type).map_err(|_| {
                                TransportError::InvalidRequest(format!(
                                    "invalid content type for part `{}`",
                                    part.name
                                ))
                            })?;
                        }
                        form = form.part(part.name.clone(), piece);
                    }
                }
                builder.multipart(form)
            }
            RenderedBody::File(path) => {
                let full = self.resolve(path);
                let file = tokio::fs::File::open(&full)
                    .await
                    .map_err(|e| unreadable(path, &e))?;
                let length = file
                    .metadata()
                    .await
                    .map_err(|e| unreadable(path, &e))?
                    .len();
                // Streamed from disk, never loaded whole.
                builder.header(CONTENT_LENGTH, length).body(file)
            }
            // `None`, and kinds added later (which `render_request` rejects).
            _ => builder,
        })
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.files_root.join(path)
        }
    }
}

impl HttpTransport for ReqwestTransport {
    async fn send<W: BodyWriter + Send>(
        &self,
        request: &RenderedRequest,
        body: &mut W,
        cancel: &CancelToken,
    ) -> Result<ResponseHead, TransportError> {
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let started = Instant::now();
        let timeout_ms = request.timeout_ms();
        let own;
        let ntlm = if matches!(request.auth(), TransportAuth::Ntlm { .. }) {
            let url = parse_url(request.url())?;
            let identity = match self.certified_for(&url) {
                Some(Certified {
                    client: Ok((_, identity)),
                    ..
                }) => Some(identity.clone()),
                Some(Certified { client: Err(e), .. }) => {
                    return Err(TransportError::Tls(e.clone()));
                }
                None => None,
            };
            own = ntlm_client(&self.settings, identity)?;
            Some(&own)
        } else {
            None
        };
        let exchange = self.exchange(ntlm, request, body, started);
        let limited = async {
            if timeout_ms == 0 {
                exchange.await
            } else {
                tokio::time::timeout(Duration::from_millis(timeout_ms), exchange)
                    .await
                    .unwrap_or(Err(TransportError::Timeout(timeout_ms)))
            }
        };
        tokio::select! {
            result = limited => result,
            () = cancelled(cancel) => Err(TransportError::Cancelled),
        }
    }
}

/// A client with apifast's settings on top of `builder`, through `proxy`.
fn build_client(
    builder: reqwest::ClientBuilder,
    settings: &tls::ClientSettings,
    identity: Option<tls::Identity>,
    http1_only: bool,
) -> Result<reqwest::Client, TransportError> {
    // Our own rustls configuration, and a resolver and a layer over the
    // connector, so the timeline shows DNS, connection and TLS (probe.rs).
    let builder = builder
        .tls_backend_preconfigured(settings.rustls_config(identity, http1_only)?)
        .tls_info(true)
        .dns_resolver(probe::TimedResolver)
        .connector_layer(probe::ProbeLayer);
    let builder = match &settings.proxy {
        // reqwest reads the HTTP_PROXY family of variables by default.
        ProxyChoice::System => builder,
        ProxyChoice::Off => builder.no_proxy(),
        ProxyChoice::Manual(config) => builder.proxy(manual_proxy(config)?),
    };
    builder
        // Redirects are followed by hand, to apply apifast's rules and count
        // them.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| TransportError::Other(format!("cannot build the HTTP client: {}", chain(&e))))
}

/// A client of its own for an NTLM exchange: NTLM authenticates the
/// connection, so its messages must go on one. HTTP/1.1 keeps one
/// connection per request in turn, and with the responses read whole the
/// pool hands the same connection back.
fn ntlm_client(
    settings: &tls::ClientSettings,
    identity: Option<tls::Identity>,
) -> Result<reqwest::Client, TransportError> {
    build_client(
        reqwest::Client::builder()
            .http1_only()
            .pool_max_idle_per_host(1),
        settings,
        identity,
        true,
    )
}

/// The timeline events of the connection `response` came on: DNS, the
/// new connection and its TLS handshake, as `notes` say; or that it was
/// reused, when there are none.
fn connection_events(
    notes: &[probe::Note],
    response: &reqwest::Response,
    started: Instant,
    timeline: &mut Vec<TimelineEvent>,
) {
    let since = |at: Instant| u64::try_from(at.saturating_duration_since(started).as_micros());
    let span = |from: Instant, to: Instant| {
        u64::try_from(to.saturating_duration_since(from).as_micros()).unwrap_or(u64::MAX)
    };
    let address = response.remote_addr().map(|a| a.to_string());
    let connection = probe::Connection::of(notes);
    let mut events = Vec::new();
    for (host, addresses, start, end) in &connection.dns {
        let mut ips: Vec<String> = addresses.iter().map(|a| a.ip().to_string()).collect();
        ips.dedup();
        events.push((
            *end,
            TimelineKind::Dns {
                host: host.clone(),
                addresses: ips,
                duration_us: span(*start, *end),
            },
        ));
    }
    match connection.connected {
        Some((start, end)) => events.push((
            end,
            TimelineKind::Connected {
                address,
                duration_us: span(start, end),
            },
        )),
        None => events.push((Instant::now(), TimelineKind::Reused { address })),
    }
    if let Some((start, end)) = connection.tls {
        let version = (response.extensions().get::<reqwest::tls::TlsInfo>())
            .and_then(reqwest::tls::TlsInfo::version)
            .map(tls_version);
        events.push((
            end,
            TimelineKind::Tls {
                version,
                duration_us: span(start, end),
            },
        ));
    }
    for (at, kind) in events {
        timeline.push(TimelineEvent {
            at_us: since(at).unwrap_or(u64::MAX),
            kind,
        });
    }
}

/// A TLS version as people write it.
fn tls_version(version: reqwest::tls::Version) -> String {
    use reqwest::tls::Version;
    match version {
        Version::TLS_1_3 => "TLSv1.3",
        Version::TLS_1_2 => "TLSv1.2",
        Version::TLS_1_1 => "TLSv1.1",
        Version::TLS_1_0 => "TLSv1.0",
        _ => "TLS",
    }
    .to_owned()
}

/// A proxy for every scheme, with its credentials and exclusions.
fn manual_proxy(config: &ProxyConfig) -> Result<reqwest::Proxy, TransportError> {
    let url = config
        .url()
        .ok_or_else(|| TransportError::InvalidRequest("the proxy has no host".into()))?;
    let mut proxy = reqwest::Proxy::all(&url)
        .map_err(|e| TransportError::InvalidRequest(format!("invalid proxy `{url}`: {e}")))?;
    if config.auth && !config.username.is_empty() {
        proxy = proxy.basic_auth(&config.username, &config.password);
    }
    Ok(proxy.no_proxy(reqwest::NoProxy::from_string(&config.bypass)))
}

/// Completes once `token` is cancelled.
async fn cancelled(token: &CancelToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// Parses the URL, adding `http://` when there is no scheme, as Bruno does.
fn parse_url(raw: &str) -> Result<Url, TransportError> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    // The url crate's messages do not repeat the URL, which may hold secrets.
    let url = Url::parse(&with_scheme).map_err(|e| TransportError::InvalidUrl(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(TransportError::InvalidUrl(format!(
            "unsupported scheme `{}`",
            url.scheme()
        )));
    }
    Ok(url)
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, TransportError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let header = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| TransportError::InvalidRequest(format!("invalid header name `{name}`")))?;
        // The value is not quoted in the message: it may be a secret.
        let value = HeaderValue::from_str(value).map_err(|_| {
            TransportError::InvalidRequest(format!("invalid value for header `{name}`"))
        })?;
        map.append(header, value);
    }
    Ok(map)
}

/// Status, version, URL and headers of `response`; its times are set
/// once the body has arrived.
fn response_head(response: &reqwest::Response, redirects: u32) -> ResponseHead {
    ResponseHead {
        status: response.status().as_u16(),
        version: format!("{:?}", response.version()),
        url: response.url().to_string(),
        redirects,
        headers: response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect(),
        elapsed_ms: 0,
        timeline: Vec::new(),
    }
}

/// Where a request of the exchange goes, to decide how to authenticate it.
struct Outgoing<'a> {
    method: &'a Method,
    url: &'a Url,
    send_body: bool,
    /// The origin of the URL the user wrote.
    first_origin: &'a Origin,
}

/// The headers of one request that goes out: `headers`, signed when the
/// auth is a signature. Only requests to the origin the user wrote are
/// signed, so a signature never leaks to another.
fn signed_headers(
    request: &RenderedRequest,
    headers: &HeaderMap,
    out: &Outgoing<'_>,
) -> Result<HeaderMap, TransportError> {
    let mut headers = headers.clone();
    if let TransportAuth::AwsV4(credentials) = request.auth()
        && origin(out.url) == *out.first_origin
    {
        let payload = if out.send_body {
            payload_hash(request.body())
        } else {
            sigv4::sha256_hex(b"")
        };
        sigv4::sign(
            out.method.as_str(),
            out.url,
            &mut headers,
            &payload,
            credentials,
            &amz_date(),
        )?;
    }
    Ok(headers)
}

/// The `Authorization` answering a `Digest` challenge in `response`, when
/// the request uses Digest and goes to the origin the user wrote.
///
/// # Errors
///
/// If the challenge asks for something apifast does not support.
fn digest_answer(
    request: &RenderedRequest,
    response: &reqwest::Response,
    out: &Outgoing<'_>,
) -> Result<Option<HeaderValue>, TransportError> {
    let TransportAuth::Digest { username, password } = request.auth() else {
        return Ok(None);
    };
    if origin(out.url) != *out.first_origin {
        return Ok(None);
    }
    let offered = response
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok());
    let Some(challenge) = digest::Challenge::find(offered) else {
        return Ok(None);
    };
    let mut uri = out.url.path().to_owned();
    if let Some(query) = out.url.query() {
        uri.push('?');
        uri.push_str(query);
    }
    let header = digest::authorization(
        &challenge,
        &digest::Answer {
            username,
            password,
            method: out.method.as_str(),
            uri: &uri,
            cnonce: &cnonce(),
        },
    )
    .map_err(TransportError::Other)?;
    HeaderValue::from_str(&header)
        .map(Some)
        .map_err(|_| TransportError::InvalidRequest("invalid Digest credentials".into()))
}

/// A fresh client nonce: 16 hex digits from the standard library's randomly
/// seeded hasher and the time. A nonce only needs to be unpredictable
/// enough not to repeat.
fn cnonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{:016x}", hasher.finish())
}

/// Writes the body of `response` to `writer` as it arrives; returns its size.
async fn stream_body<W: BodyWriter>(
    mut response: reqwest::Response,
    writer: &mut W,
    url: &Url,
) -> Result<u64, TransportError> {
    let mut bytes = 0;
    while let Some(chunk) = response.chunk().await.map_err(|e| classify(&e, url))? {
        writer
            .write(&chunk)
            .map_err(|e| TransportError::Other(format!("cannot store the response body: {e}")))?;
        bytes += chunk.len() as u64;
    }
    Ok(bytes)
}

/// The URL, method and headers of the first request of an exchange.
fn first_request(request: &RenderedRequest) -> Result<(Url, Method, HeaderMap), TransportError> {
    let url = parse_url(request.url())?;
    let method = Method::from_bytes(request.method().as_str().as_bytes())
        .map_err(|_| TransportError::InvalidRequest("invalid method".into()))?;
    let mut headers = header_map(request.headers())?;
    if !headers.contains_key(USER_AGENT) {
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    }
    Ok((url, method, headers))
}

/// What a 401 led to.
enum Challenged {
    /// The `Authorization` that answers it: send again with it.
    Answer(HeaderValue),
    /// Nothing to answer: the response stands.
    No(reqwest::Response),
}

/// Answers a 401's challenge when the auth is Digest or NTLM and the
/// request goes to the origin the user wrote.
///
/// # Errors
///
/// If the challenge asks for something apifast does not support.
async fn answer_challenge(
    request: &RenderedRequest,
    response: reqwest::Response,
    out: &Outgoing<'_>,
) -> Result<Challenged, TransportError> {
    if origin(out.url) != *out.first_origin {
        return Ok(Challenged::No(response));
    }
    if let Some(answer) = digest_answer(request, &response, out)? {
        return Ok(Challenged::Answer(answer));
    }
    let TransportAuth::Ntlm {
        username,
        password,
        domain,
    } = request.auth()
    else {
        return Ok(Challenged::No(response));
    };
    let challenge = response
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| v.strip_prefix("NTLM "))
        .and_then(|b64| {
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .ok()
        })
        .and_then(|bytes| ntlm::Challenge::parse(&bytes));
    let Some(challenge) = challenge else {
        return Ok(Challenged::No(response));
    };
    // Read whole, so the connection goes back to the pool for the answer.
    let _ = response.bytes().await;
    let mut client_challenge = [0u8; 8];
    client_challenge.copy_from_slice(&cnonce().as_bytes()[..8]);
    let message = ntlm::authenticate(
        &challenge,
        &ntlm::Credentials {
            username,
            password,
            domain,
            workstation: "",
            client_challenge,
            now: filetime_now(),
        },
    );
    let header = format!(
        "NTLM {}",
        base64::engine::general_purpose::STANDARD.encode(message)
    );
    HeaderValue::from_str(&header)
        .map(Challenged::Answer)
        .map_err(|_| TransportError::InvalidRequest("invalid NTLM credentials".into()))
}

/// Starts the NTLM handshake: the Negotiate message, when the auth is NTLM.
fn start_handshake(request: &RenderedRequest, headers: &mut HeaderMap) {
    if let TransportAuth::Ntlm { .. } = request.auth() {
        let negotiate = format!(
            "NTLM {}",
            base64::engine::general_purpose::STANDARD.encode(ntlm::negotiate())
        );
        if let Ok(value) = HeaderValue::from_str(&negotiate) {
            headers.insert(AUTHORIZATION, value);
        }
    }
}

/// Now, in 100 ns intervals since 1601, as Windows counts time.
fn filetime_now() -> u64 {
    const UNIX_EPOCH_IN_FILETIME: u64 = 116_444_736_000_000_000;
    let since_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos() / 100).unwrap_or(u64::MAX));
    UNIX_EPOCH_IN_FILETIME.saturating_add(since_unix)
}

/// Where a redirect goes, and its `Location` as written.
fn redirect_target(url: &Url, location: &HeaderValue) -> Result<(Url, String), TransportError> {
    let location = location
        .to_str()
        .map_err(|_| TransportError::Protocol("the redirect location is not valid text".into()))?;
    let next = url
        .join(location)
        .map_err(|e| TransportError::Protocol(format!("invalid redirect location: {e}")))?;
    Ok((next, location.to_owned()))
}

/// Turns the request into a GET without body, as a 301 to 303 asks.
fn as_get(method: &mut Method, headers: &mut HeaderMap) {
    if *method != Method::HEAD {
        *method = Method::GET;
    }
    headers.remove(CONTENT_TYPE);
    headers.remove(CONTENT_LENGTH);
}

/// A form body as it is sent.
fn form(fields: &[(String, String)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().map(|(n, v)| (n.as_str(), v.as_str())))
        .finish()
}

/// The SHA-256 of the body as it is sent, for a signature; streamed bodies
/// are not read twice and go unsigned.
fn payload_hash(body: &RenderedBody) -> String {
    match body {
        RenderedBody::Text(text) => sigv4::sha256_hex(text.as_bytes()),
        RenderedBody::Form(fields) => sigv4::sha256_hex(form(fields).as_bytes()),
        RenderedBody::Multipart(_) | RenderedBody::File(_) => sigv4::UNSIGNED_PAYLOAD.into(),
        _ => sigv4::sha256_hex(b""),
    }
}

/// Now as SigV4 writes it: `20150830T123600Z`.
fn amz_date() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    // `2015-08-30T12:36:00.000Z` without separators and milliseconds.
    let iso = apifast_template::iso_timestamp(millis);
    let mut out: String = iso.chars().filter(|c| !matches!(c, '-' | ':')).collect();
    if let Some(dot) = out.find('.') {
        out.replace_range(dot.., "Z");
    }
    out
}

/// Scheme, host and port.
type Origin = (String, Option<String>, Option<u16>);

/// Scheme, host and port: what decides whether credentials may follow.
fn origin(url: &Url) -> Origin {
    (
        url.scheme().to_owned(),
        url.host_str().map(str::to_owned),
        url.port_or_known_default(),
    )
}

fn unreadable(path: &str, error: &std::io::Error) -> TransportError {
    TransportError::InvalidRequest(format!("cannot read `{path}`: {error}"))
}

/// Sorts a reqwest error into the classes the UI explains differently.
fn classify(error: &reqwest::Error, url: &Url) -> TransportError {
    let host = url.host_str().unwrap_or_default();
    let detail = chain(error);
    if has_tls_error(error) {
        return TransportError::Tls(detail);
    }
    if error.is_connect() {
        let lower = detail.to_lowercase();
        if lower.contains("dns error") || lower.contains("failed to lookup address") {
            return TransportError::Dns(format!("cannot resolve `{host}`: {detail}"));
        }
        let port = url.port_or_known_default().unwrap_or_default();
        return TransportError::Connect(format!("cannot connect to `{host}:{port}`: {detail}"));
    }
    if error.is_timeout() {
        return TransportError::Protocol(format!("the server stopped answering: {detail}"));
    }
    if error.is_body() || error.is_decode() || error.is_request() {
        return TransportError::Protocol(detail);
    }
    TransportError::Other(detail)
}

/// Whether a rustls error is somewhere in the chain. hyper and tokio-rustls
/// nest it inside `io::Error`s, whose `source()` skips the wrapped error, so
/// each `io::Error` is opened with `get_ref` too.
fn has_tls_error(error: &(dyn std::error::Error + 'static)) -> bool {
    if error.downcast_ref::<rustls::Error>().is_some() {
        return true;
    }
    if let Some(io) = error.downcast_ref::<std::io::Error>()
        && let Some(inner) = io.get_ref()
        && has_tls_error(inner)
    {
        return true;
    }
    error.source().is_some_and(has_tls_error)
}

/// The error and its causes, without the URL (which may hold secrets).
fn chain(error: &reqwest::Error) -> String {
    let mut parts: Vec<String> = Vec::new();
    // reqwest's own message is generic ("error sending request"); the causes
    // say what happened.
    let mut current = error.source();
    while let Some(e) = current {
        let text = e.to_string();
        if !parts.iter().any(|p| p.contains(&text)) {
            parts.push(text);
        }
        current = e.source();
    }
    if parts.is_empty() {
        let without_url = error.to_string();
        // `Display` of a reqwest error appends ` for url (...)`.
        parts.push(
            without_url
                .split(" for url (")
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_without_scheme_get_http() {
        assert_eq!(
            parse_url("localhost:8080/x").unwrap().as_str(),
            "http://localhost:8080/x"
        );
        assert_eq!(
            parse_url(" https://a.test/ ").unwrap().as_str(),
            "https://a.test/"
        );
    }

    #[test]
    fn invalid_urls_do_not_echo_the_url() {
        let error = parse_url("http://exa mple.test/?token=SECRET").unwrap_err();
        assert!(matches!(error, TransportError::InvalidUrl(_)));
        assert!(!error.to_string().contains("SECRET"), "{error}");
        assert!(matches!(
            parse_url("ftp://a.test/"),
            Err(TransportError::InvalidUrl(_))
        ));
    }

    #[test]
    fn invalid_headers_name_the_header_but_not_the_value() {
        let error = header_map(&[("x bad".into(), "v".into())]).unwrap_err();
        assert_eq!(
            error,
            TransportError::InvalidRequest("invalid header name `x bad`".into())
        );
        let error = header_map(&[("x-key".into(), "SECRET\nnewline".into())]).unwrap_err();
        assert!(!error.to_string().contains("SECRET"), "{error}");
    }

    #[test]
    fn repeated_headers_are_all_sent() {
        let map =
            header_map(&[("accept".into(), "a".into()), ("Accept".into(), "b".into())]).unwrap();
        assert_eq!(map.get_all("accept").iter().count(), 2);
    }

    #[test]
    fn origins_compare_scheme_host_and_port() {
        let url = |s| Url::parse(s).unwrap();
        assert_eq!(
            origin(&url("https://a.test/x")),
            origin(&url("https://a.test:443/y"))
        );
        assert_ne!(
            origin(&url("https://a.test/")),
            origin(&url("http://a.test/"))
        );
        assert_ne!(
            origin(&url("https://a.test/")),
            origin(&url("https://b.test/"))
        );
        assert_ne!(
            origin(&url("http://a.test:1/")),
            origin(&url("http://a.test:2/"))
        );
    }
}
