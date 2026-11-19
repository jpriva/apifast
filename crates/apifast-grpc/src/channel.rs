//! The HTTP/2 channel of a call: tonic over a connection the HTTP
//! transport opened, so it has the collection's TLS trust, client
//! certificates and DNS overrides.

use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use apifast_app::TransportError;
use apifast_http::ReqwestTransport;
use hyper_util::rt::TokioIo;
use tonic::body::Body;
use tonic::transport::{Channel, Endpoint};
use tower_service::Service;

use crate::target::Target;

/// A channel to a server, whose methods are under its target's prefix.
#[derive(Debug, Clone)]
pub(crate) struct Prefixed {
    inner: Channel,
    prefix: Arc<str>,
}

/// Opens a channel to `target`. The connection is opened first, so that
/// its errors are the transport's own.
///
/// # Errors
///
/// If the connection cannot be opened, or HTTP/2 cannot start over it.
pub(crate) async fn connect(
    http: &ReqwestTransport,
    target: &Target,
) -> Result<Prefixed, TransportError> {
    let connection = http.open(&target.host, target.port, target.tls).await?;
    let host = if target.host.contains(':') {
        format!("[{}]", target.host)
    } else {
        target.host.clone()
    };
    let invalid = |e: &dyn std::fmt::Display| TransportError::InvalidUrl(e.to_string());
    // The TLS is the connection's own: tonic is told `http`, and the
    // requests say `https`.
    let mut endpoint =
        Endpoint::from_shared(format!("http://{host}:{}", target.port)).map_err(|e| invalid(&e))?;
    if target.tls {
        let origin = format!("https://{host}:{}", target.port);
        endpoint = endpoint.origin(origin.parse().map_err(|e| invalid(&e))?);
    }
    // The channel takes this one connection; it does not reconnect.
    let once = Arc::new(Mutex::new(Some(connection)));
    let connector = tower_service_fn(move |_| {
        let taken = once.lock().unwrap_or_else(PoisonError::into_inner).take();
        async move {
            taken
                .map(TokioIo::new)
                .ok_or_else(|| std::io::Error::other("the connection was closed"))
        }
    });
    let inner = endpoint
        .connect_with_connector(connector)
        .await
        .map_err(|e| TransportError::Protocol(crate::chain(&e)))?;
    Ok(Prefixed {
        inner,
        prefix: target.prefix.as_str().into(),
    })
}

impl Service<http::Request<Body>> for Prefixed {
    type Response = <Channel as Service<http::Request<Body>>>::Response;
    type Error = <Channel as Service<http::Request<Body>>>::Error;
    type Future = <Channel as Service<http::Request<Body>>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::<http::Request<Body>>::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, mut request: http::Request<Body>) -> Self::Future {
        if !self.prefix.is_empty() {
            let mut parts = request.uri().clone().into_parts();
            let path = (parts.path_and_query.as_ref()).map_or("/", |p| p.as_str());
            if let Ok(prefixed) = format!("{}{path}", self.prefix).parse() {
                parts.path_and_query = Some(prefixed);
                if let Ok(uri) = http::Uri::from_parts(parts) {
                    *request.uri_mut() = uri;
                }
            }
        }
        self.inner.call(request)
    }
}

/// A connector from a closure, as `tower::service_fn`.
fn tower_service_fn<F, R>(f: F) -> ServiceFn<F>
where
    F: FnMut(http::Uri) -> R,
{
    ServiceFn(f)
}

#[derive(Clone)]
struct ServiceFn<F>(F);

impl<F, R, T> Service<http::Uri> for ServiceFn<F>
where
    F: FnMut(http::Uri) -> R,
    R: Future<Output = Result<T, std::io::Error>>,
{
    type Response = T;
    type Error = std::io::Error;
    type Future = R;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: http::Uri) -> R {
        (self.0)(uri)
    }
}
