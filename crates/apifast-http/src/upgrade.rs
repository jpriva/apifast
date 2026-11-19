//! HTTP/1.1 upgrades (docs/07, F7): the handshake of a WebSocket connection
//! goes through the same client as any request, so it has the collection's
//! proxy, TLS trust, client certificates, DNS overrides and cookies, and a
//! timeline. What follows the handshake is the caller's: the connection is
//! handed over as a byte stream.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use apifast_app::{
    CancelToken, RenderedRequest, ResponseHead, TimelineEvent, TimelineKind, TransportAuth,
    TransportError,
};
use reqwest::header::{HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, Url};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{
    Certified, DEFAULT_USER_AGENT, Outgoing, ReqwestTransport, build_client, cancelled, chain,
    classify, connection_events, cookies, header_map, origin, probe, response_head, signed_headers,
};

/// A connection after its upgrade: bytes both ways.
#[derive(Debug)]
pub struct Upgraded(reqwest::Upgraded);

impl AsyncRead for Upgraded {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Upgraded {
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

impl ReqwestTransport {
    /// Sends `request` as a `GET` asking to switch protocols, with
    /// `protocol`'s headers over its own, and hands over the connection
    /// once the server agrees (`101`). A `ws://` or `wss://` URL goes as
    /// `http://` or `https://`; one without a scheme, as `ws://`. The
    /// request's timeout covers the handshake.
    ///
    /// The response's URL keeps the scheme the request was written with.
    ///
    /// # Errors
    ///
    /// As a send, and if the server answers anything but `101`, or the auth
    /// is one that needs more than one request (Digest, NTLM).
    pub async fn upgrade(
        &self,
        request: &RenderedRequest,
        protocol: &[(&'static str, String)],
        cancel: &CancelToken,
    ) -> Result<(ResponseHead, Upgraded), TransportError> {
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let timeout_ms = request.timeout_ms();
        let expired = async {
            if timeout_ms == 0 {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
        };
        tokio::select! {
            result = self.handshake(request, protocol) => result,
            () = expired => Err(TransportError::Timeout(timeout_ms)),
            () = cancelled(cancel) => Err(TransportError::Cancelled),
        }
    }

    async fn handshake(
        &self,
        request: &RenderedRequest,
        protocol: &[(&'static str, String)],
    ) -> Result<(ResponseHead, Upgraded), TransportError> {
        let started = Instant::now();
        let (url, written) = upgrade_url(request.url())?;
        if !matches!(
            request.auth(),
            TransportAuth::None | TransportAuth::AwsV4(_)
        ) {
            return Err(TransportError::InvalidRequest(
                "Digest and NTLM auth cannot open a WebSocket connection yet".into(),
            ));
        }
        let mut headers = header_map(request.headers())?;
        if !headers.contains_key(USER_AGENT) {
            headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        }
        for (name, value) in protocol {
            let value = HeaderValue::from_str(value).map_err(|_| {
                TransportError::InvalidRequest(format!("invalid value for header `{name}`"))
            })?;
            headers.insert(HeaderName::from_static(name), value);
        }
        let first_origin = origin(&url);
        let sent = Outgoing {
            method: &Method::GET,
            url: &url,
            send_body: false,
            first_origin: &first_origin,
        };
        let mut headers = signed_headers(request, &headers, &sent)?;
        if let Some(jar) = &self.cookies {
            cookies::add_from_jar(jar, &url, &mut headers);
        }
        let identity = match self.certified_for(&url) {
            Some(Certified {
                client: Ok((_, identity)),
                ..
            }) => Some(identity.clone()),
            Some(Certified { client: Err(e), .. }) => return Err(TransportError::Tls(e.clone())),
            None => None,
        };
        // The upgrade is HTTP/1.1's: HTTP/2 must not be offered.
        let client = build_client(
            reqwest::Client::builder().http1_only(),
            &self.settings,
            identity,
            true,
        )?;
        let at = || u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let mut timeline = vec![TimelineEvent {
            at_us: 0,
            kind: TimelineKind::Request {
                method: "GET".into(),
                url: written.clone(),
            },
        }];
        let (result, notes) =
            probe::Probe::watch(client.get(url.clone()).headers(headers).send()).await;
        let response = result.map_err(|e| classify(&e, &url))?;
        connection_events(&notes, &response, started, &mut timeline);
        if let Some(jar) = &self.cookies {
            cookies::keep(jar, &url, response.headers());
        }
        let status = response.status().as_u16();
        timeline.push(TimelineEvent {
            at_us: at(),
            kind: TimelineKind::Headers { status },
        });
        if status != 101 {
            let reason = apifast_domain::reason_phrase(status).unwrap_or_default();
            return Err(TransportError::Protocol(
                format!("the server answered {status} {reason} instead of switching protocols")
                    .trim_end()
                    .to_owned(),
            ));
        }
        let mut head = response_head(&response, 0);
        head.url = written;
        let upgraded = response
            .upgrade()
            .await
            .map_err(|e| TransportError::Protocol(chain(&e)))?;
        head.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        head.timeline = timeline;
        Ok((head, Upgraded(upgraded)))
    }
}

/// The URL to send the handshake to, and the URL as the user knows it: a
/// `ws` scheme goes as `http`, `wss` as `https`.
fn upgrade_url(raw: &str) -> Result<(Url, String), TransportError> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("ws://{raw}")
    };
    let mut url =
        Url::parse(&with_scheme).map_err(|e| TransportError::InvalidUrl(e.to_string()))?;
    let written = url.to_string();
    let scheme = match url.scheme() {
        "ws" | "http" => "http",
        "wss" | "https" => "https",
        other => {
            return Err(TransportError::InvalidUrl(format!(
                "unsupported scheme `{other}`"
            )));
        }
    };
    // Both are special schemes, so the url crate lets one become the other.
    let _ = url.set_scheme(scheme);
    Ok((url, written))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_schemes_go_as_http_and_are_kept_for_the_user() {
        let (url, written) = upgrade_url(" wss://api.test/live?x=1 ").unwrap();
        assert_eq!(url.as_str(), "https://api.test/live?x=1");
        assert_eq!(written, "wss://api.test/live?x=1");
        let (url, written) = upgrade_url("localhost:8080/ws").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8080/ws");
        assert_eq!(written, "ws://localhost:8080/ws");
        assert!(upgrade_url("ftp://x").is_err());
    }
}
