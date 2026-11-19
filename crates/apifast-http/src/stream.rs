//! Raw connections (docs/07, F8): a TCP connection to a host, in TLS when
//! asked, offering only HTTP/2. It has the collection's TLS trust, client
//! certificates and DNS overrides; what goes over it is the caller's (gRPC
//! speaks HTTP/2 over it).

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use apifast_app::TransportError;
use reqwest::Url;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::{Certified, ReqwestTransport};

/// An open connection: bytes both ways.
#[derive(Debug)]
pub struct Connection(Inner);

#[derive(Debug)]
enum Inner {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl ReqwestTransport {
    /// Connects to `host` on `port`, in TLS with `tls`, where ALPN offers
    /// only HTTP/2.
    ///
    /// # Errors
    ///
    /// If the name cannot be resolved, no address takes the connection,
    /// or the TLS handshake fails (or the host's client certificate could
    /// not be loaded).
    pub async fn open(
        &self,
        host: &str,
        port: u16,
        tls: bool,
    ) -> Result<Connection, TransportError> {
        let addresses = self.settings.resolver.addresses(host, port).await?;
        let tcp = connect(host, &addresses).await?;
        let _ = tcp.set_nodelay(true);
        if !tls {
            return Ok(Connection(Inner::Plain(tcp)));
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
        let name = ServerName::try_from(host.to_owned())
            .map_err(|e| TransportError::InvalidUrl(format!("`{host}`: {e}")))?;
        let stream = TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        Ok(Connection(Inner::Tls(Box::new(stream))))
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
        match &mut self.0 {
            Inner::Plain(tcp) => Pin::new(tcp).poll_read(cx, buf),
            Inner::Tls(tls) => Pin::new(tls.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.0 {
            Inner::Plain(tcp) => Pin::new(tcp).poll_write(cx, buf),
            Inner::Tls(tls) => Pin::new(tls.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.0 {
            Inner::Plain(tcp) => Pin::new(tcp).poll_flush(cx),
            Inner::Tls(tls) => Pin::new(tls.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.0 {
            Inner::Plain(tcp) => Pin::new(tcp).poll_shutdown(cx),
            Inner::Tls(tls) => Pin::new(tls.as_mut()).poll_shutdown(cx),
        }
    }
}
