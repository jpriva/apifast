//! Raw connections (docs/07, F8): a TCP connection to a host, in TLS when
//! asked, offering only HTTP/2. It has the collection's proxy, TLS trust,
//! client certificates and DNS overrides; what goes over it is the
//! caller's (gRPC speaks HTTP/2 over it).
//!
//! The proxy is chosen by the same rules as reqwest's (the `HTTP_PROXY`
//! family of variables, or the manual one with its exclusions), and
//! crossed with `CONNECT` (over HTTP or HTTPS), SOCKS5 or SOCKS4 (`4a`).

use std::fmt::{self, Write as _};
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use apifast_app::TransportError;
use apifast_domain::{ProxyChoice, ProxyConfig};
use base64::Engine as _;
use hyper_util::client::proxy::matcher::Matcher;
use reqwest::Url;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::{Certified, ReqwestTransport};

/// What a connection is made of: TCP, perhaps a tunnel, perhaps TLS.
trait Io: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

/// An open connection: bytes both ways.
pub struct Connection(Box<dyn Io>);

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

/// The proxy a connection goes through, and its credentials.
struct Through {
    kind: Kind,
    host: String,
    port: u16,
    credentials: Option<(String, String)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Http,
    Https,
    Socks5,
    Socks4,
}

/// Longest answer to a `CONNECT` read, in bytes.
const MAX_CONNECT_ANSWER: usize = 16 * 1024;

impl ReqwestTransport {
    /// Connects to `host` on `port`, through the proxy if one applies, in
    /// TLS with `tls`, where ALPN offers only HTTP/2.
    ///
    /// # Errors
    ///
    /// If a name cannot be resolved, no address takes the connection, the
    /// proxy refuses it, or the TLS handshake fails (or the host's client
    /// certificate could not be loaded).
    pub async fn open(
        &self,
        host: &str,
        port: u16,
        tls: bool,
    ) -> Result<Connection, TransportError> {
        let through = proxy_for(&self.settings.proxy, host, port, tls)?;
        let stream: Box<dyn Io> = match &through {
            None => Box::new(self.tcp(host, port).await?),
            Some(proxy) => self.tunnel(proxy, host, port).await?,
        };
        if !tls {
            return Ok(Connection(stream));
        }
        let url = Url::parse(&format!("https://{host}:{port}/"))
            .map_err(|e| TransportError::InvalidUrl(e.to_string()))?;
        let identity = match self.certified_for(&url) {
            Some(Certified {
                client: Ok((_, identity)),
                ..
            }) => Some(identity.clone()),
            Some(Certified { client: Err(e), .. }) => return Err(TransportError::Tls(e.clone())),
            None => None,
        };
        let mut config = self.settings.rustls_config(identity, false)?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        let stream = tls_over(stream, host, config).await?;
        Ok(Connection(Box::new(stream)))
    }

    /// A TCP connection to `host`, with the DNS overrides.
    async fn tcp(&self, host: &str, port: u16) -> Result<TcpStream, TransportError> {
        let addresses = self.settings.resolver.addresses(host, port).await?;
        let tcp = connect(host, &addresses).await?;
        let _ = tcp.set_nodelay(true);
        Ok(tcp)
    }

    /// A connection to `host` through `proxy`.
    async fn tunnel(
        &self,
        proxy: &Through,
        host: &str,
        port: u16,
    ) -> Result<Box<dyn Io>, TransportError> {
        let tcp = self.tcp(&proxy.host, proxy.port).await?;
        let refused =
            |why: String| TransportError::Connect(format!("the proxy `{}`: {why}", proxy.host));
        Ok(match proxy.kind {
            Kind::Http => {
                let mut tcp = tcp;
                connect_tunnel(&mut tcp, host, port, proxy.credentials.as_ref())
                    .await
                    .map_err(refused)?;
                Box::new(tcp)
            }
            Kind::Https => {
                // To the proxy itself, HTTP/1.1 in TLS, with the same trust.
                let mut config = self.settings.rustls_config(None, true)?;
                config.alpn_protocols = vec![b"http/1.1".to_vec()];
                let mut stream = tls_over(Box::new(tcp), &proxy.host, config).await?;
                connect_tunnel(&mut stream, host, port, proxy.credentials.as_ref())
                    .await
                    .map_err(refused)?;
                Box::new(stream)
            }
            Kind::Socks5 => {
                let mut tcp = tcp;
                socks5(&mut tcp, host, port, proxy.credentials.as_ref())
                    .await
                    .map_err(refused)?;
                Box::new(tcp)
            }
            Kind::Socks4 => {
                let mut tcp = tcp;
                socks4a(&mut tcp, host, port, proxy.credentials.as_ref())
                    .await
                    .map_err(refused)?;
                Box::new(tcp)
            }
        })
    }
}

/// The proxy a connection to `host` on `port` goes through, by `choice`:
/// none when it is off or the host is excluded.
///
/// # Errors
///
/// If the proxy's URL is not valid.
fn proxy_for(
    choice: &ProxyChoice,
    host: &str,
    port: u16,
    tls: bool,
) -> Result<Option<Through>, TransportError> {
    let matcher = match choice {
        ProxyChoice::Off => return Ok(None),
        ProxyChoice::System => Matcher::from_env(),
        ProxyChoice::Manual(config) => manual(config)?,
    };
    let scheme = if tls { "https" } else { "http" };
    let host_part = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let Ok(destination) = format!("{scheme}://{host_part}:{port}/").parse::<http::Uri>() else {
        return Err(TransportError::InvalidUrl(format!("`{host}`")));
    };
    let Some(intercept) = matcher.intercept(&destination) else {
        return Ok(None);
    };
    let uri = intercept.uri();
    let invalid = || TransportError::InvalidRequest(format!("invalid proxy `{uri}`"));
    let kind = match uri.scheme_str().unwrap_or("http") {
        "http" => Kind::Http,
        "https" => Kind::Https,
        "socks5" | "socks5h" => Kind::Socks5,
        "socks4" | "socks4a" => Kind::Socks4,
        _ => return Err(invalid()),
    };
    let proxy_host = uri
        .host()
        .ok_or_else(invalid)?
        .trim_matches(['[', ']'])
        .to_owned();
    let default_port = match kind {
        Kind::Http => 80,
        Kind::Https => 443,
        Kind::Socks5 | Kind::Socks4 => 1080,
    };
    let credentials = match choice {
        ProxyChoice::Manual(config) if config.auth && !config.username.is_empty() => {
            Some((config.username.clone(), config.password.clone()))
        }
        ProxyChoice::Manual(_) => None,
        _ => intercept
            .raw_auth()
            .map(|(user, pass)| (user.to_owned(), pass.to_owned())),
    };
    Ok(Some(Through {
        kind,
        host: proxy_host,
        port: uri.port_u16().unwrap_or(default_port),
        credentials,
    }))
}

/// The rules of a manual proxy: it for every host, but those excluded.
fn manual(config: &ProxyConfig) -> Result<Matcher, TransportError> {
    let url = config
        .url()
        .ok_or_else(|| TransportError::InvalidRequest("the proxy has no host".into()))?;
    Ok(Matcher::builder()
        .all(url)
        .no(config.bypass.clone())
        .build())
}

/// `stream` in TLS to `host`, with `config`.
async fn tls_over(
    stream: Box<dyn Io>,
    host: &str,
    config: rustls::ClientConfig,
) -> Result<tokio_rustls::client::TlsStream<Box<dyn Io>>, TransportError> {
    let name = ServerName::try_from(host.to_owned())
        .map_err(|e| TransportError::InvalidUrl(format!("`{host}`: {e}")))?;
    TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|e| TransportError::Tls(e.to_string()))
}

/// Asks an HTTP proxy, over `stream`, for a tunnel to `host` on `port`.
///
/// # Errors
///
/// What the proxy answered instead of `200`, or why it could not be read.
async fn connect_tunnel<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<&(String, String)>,
) -> Result<(), String> {
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut ask = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some((user, pass)) = credentials {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        let _ = write!(ask, "Proxy-Authorization: Basic {token}\r\n");
    }
    ask.push_str("\r\n");
    let io = |e: io::Error| e.to_string();
    stream.write_all(ask.as_bytes()).await.map_err(io)?;
    stream.flush().await.map_err(io)?;
    let mut answer = Vec::new();
    while !answer.ends_with(b"\r\n\r\n") {
        if answer.len() >= MAX_CONNECT_ANSWER {
            return Err("its answer to CONNECT is too long".into());
        }
        let byte = stream
            .read_u8()
            .await
            .map_err(|_| "it closed the connection".to_owned())?;
        answer.push(byte);
    }
    let text = String::from_utf8_lossy(&answer);
    let status = text.lines().next().unwrap_or_default().trim();
    match status.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        Some("407") => Err(format!("it asks for credentials ({status})")),
        _ => Err(format!("it answered `{status}`")),
    }
}

/// A SOCKS5 tunnel to `host` on `port`, by name (the proxy resolves it).
///
/// # Errors
///
/// What the proxy refused.
async fn socks5<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<&(String, String)>,
) -> Result<(), String> {
    let io = |e: io::Error| e.to_string();
    let methods: &[u8] = if credentials.is_some() { &[0, 2] } else { &[0] };
    let mut hello = vec![5, u8::try_from(methods.len()).unwrap_or(1)];
    hello.extend_from_slice(methods);
    stream.write_all(&hello).await.map_err(io)?;
    let mut chosen = [0u8; 2];
    stream.read_exact(&mut chosen).await.map_err(io)?;
    match (chosen, credentials) {
        ([5, 0], _) => {}
        ([5, 2], Some((user, pass))) => {
            let (user, pass) = (user.as_bytes(), pass.as_bytes());
            let too_long = || "the user name or password is longer than 255 bytes".to_owned();
            let mut auth = vec![1, u8::try_from(user.len()).map_err(|_| too_long())?];
            auth.extend_from_slice(user);
            auth.push(u8::try_from(pass.len()).map_err(|_| too_long())?);
            auth.extend_from_slice(pass);
            stream.write_all(&auth).await.map_err(io)?;
            let mut status = [0u8; 2];
            stream.read_exact(&mut status).await.map_err(io)?;
            if status[1] != 0 {
                return Err("it refused the user name and password".into());
            }
        }
        ([5, 0xff], _) => return Err("it accepts none of the ways to authenticate offered".into()),
        _ => return Err("it does not speak SOCKS5".into()),
    }
    let name = host.as_bytes();
    let length = u8::try_from(name.len()).map_err(|_| "the host name is too long".to_owned())?;
    let mut ask = vec![5, 1, 0, 3, length];
    ask.extend_from_slice(name);
    ask.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&ask).await.map_err(io)?;
    let mut reply = [0u8; 4];
    stream.read_exact(&mut reply).await.map_err(io)?;
    if reply[1] != 0 {
        return Err(socks5_refusal(reply[1]).into());
    }
    // The address it bound, which is not needed.
    let skip = match reply[3] {
        1 => 4,
        4 => 16,
        3 => usize::from(stream.read_u8().await.map_err(io)?),
        _ => return Err("it gave an unknown address type".into()),
    };
    let mut bound = vec![0u8; skip + 2];
    stream.read_exact(&mut bound).await.map_err(io)?;
    Ok(())
}

fn socks5_refusal(code: u8) -> &'static str {
    match code {
        2 => "its rules do not allow the connection",
        3 => "the network is unreachable",
        4 => "the host is unreachable",
        5 => "the host refused the connection",
        6 => "the time to live expired",
        _ => "it could not make the connection",
    }
}

/// A SOCKS4 tunnel (version `4a`) to `host` on `port`, by name; the user name, if any,
/// is its user id.
///
/// # Errors
///
/// What the proxy refused.
async fn socks4a<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<&(String, String)>,
) -> Result<(), String> {
    let io = |e: io::Error| e.to_string();
    let mut ask = vec![4, 1];
    ask.extend_from_slice(&port.to_be_bytes());
    // 0.0.0.1: the name follows (SOCKS4a).
    ask.extend_from_slice(&[0, 0, 0, 1]);
    if let Some((user, _)) = credentials {
        ask.extend_from_slice(user.as_bytes());
    }
    ask.push(0);
    ask.extend_from_slice(host.as_bytes());
    ask.push(0);
    stream.write_all(&ask).await.map_err(io)?;
    let mut reply = [0u8; 8];
    stream.read_exact(&mut reply).await.map_err(io)?;
    if reply[1] == 0x5a {
        Ok(())
    } else {
        Err("it refused the connection".into())
    }
}

/// The first of `addresses` that takes the connection.
async fn connect(host: &str, addresses: &[SocketAddr]) -> Result<TcpStream, TransportError> {
    let mut last = None;
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last = Some(e),
        }
    }
    Err(TransportError::Connect(match last {
        Some(e) => format!("cannot connect to `{host}`: {e}"),
        None => format!("`{host}` has no address"),
    }))
}

impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
