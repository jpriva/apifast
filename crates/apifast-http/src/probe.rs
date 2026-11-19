//! What happens below the request (docs/07, "Timeline"): the DNS lookup,
//! the connection and the TLS handshake of each new connection, with the
//! pool on. A send runs inside [`Probe::watch`]; the resolver, the layer over
//! reqwest's connector and the TLS session store note what they do in the
//! probe of the task that polls them. A request that goes on a pooled
//! connection notes nothing, and is shown as reused.
//!
//! The connection is polled inside the send's task, so its notes reach the
//! send that opened it. A connection hyper finishes in the background, after
//! the request took another one from the pool, notes nothing.

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Instant;

use apifast_domain::{DnsOverride, overridden_addresses};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rustls::client::{
    ClientSessionMemoryCache, ClientSessionStore, Tls12ClientSessionValue, Tls13ClientSessionValue,
};
use rustls::pki_types::ServerName;

tokio::task_local! {
    static PROBE: Arc<Probe>;
}

/// Something that happened on the way to the server, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Note {
    /// A name was resolved, from `start` to `end`.
    Dns {
        host: String,
        addresses: Vec<SocketAddr>,
        start: Instant,
        end: Instant,
        overridden: bool,
    },
    /// The connector started a new connection.
    ConnectStart(Instant),
    /// A TLS handshake started: the connection before it was open.
    TlsStart(Instant),
    /// The connector ended a new connection, TLS included.
    ConnectEnd(Instant),
}

/// The notes of one send.
#[derive(Debug, Default)]
pub(crate) struct Probe {
    notes: Mutex<Vec<Note>>,
}

impl Probe {
    /// Runs `send` with a new probe, and returns its result and the notes.
    pub(crate) async fn watch<F: Future>(send: F) -> (F::Output, Vec<Note>) {
        let probe = Arc::new(Self::default());
        let output = PROBE.scope(probe.clone(), send).await;
        let notes = std::mem::take(&mut *probe.lock());
        (output, notes)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Note>> {
        // Notes are only pushed: a panic elsewhere leaves them whole.
        self.notes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Notes `note` in the probe of the running send, if any.
fn note(note: Note) {
    let _ = PROBE.try_with(|probe| probe.lock().push(note));
}

/// A name resolved for a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lookup {
    pub host: String,
    pub addresses: Vec<SocketAddr>,
    pub start: Instant,
    pub end: Instant,
    /// The addresses came from the collection's DNS overrides.
    pub overridden: bool,
}

/// What the notes of a send say about its connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Connection {
    /// The lookups, in order.
    pub dns: Vec<Lookup>,
    /// When the new connection's TCP part ended and how long it took
    /// (without DNS), if a connection was opened.
    pub connected: Option<(Instant, Instant)>,
    /// When the TLS handshake started and ended.
    pub tls: Option<(Instant, Instant)>,
}

impl Connection {
    /// Reads `notes`. With a proxy over TLS there are two handshakes: the
    /// last one is the server's.
    pub(crate) fn of(notes: &[Note]) -> Self {
        let mut connection = Self::default();
        let mut started = None;
        let mut tls_start = None;
        let mut dns_end = None;
        for note in notes {
            match note {
                Note::Dns {
                    host,
                    addresses,
                    start,
                    end,
                    overridden,
                } => {
                    connection.dns.push(Lookup {
                        host: host.clone(),
                        addresses: addresses.clone(),
                        start: *start,
                        end: *end,
                        overridden: *overridden,
                    });
                    dns_end = Some(*end);
                }
                Note::ConnectStart(at) => started = Some(*at),
                Note::TlsStart(at) => tls_start = Some(*at),
                Note::ConnectEnd(end) => {
                    let Some(start) = started else { continue };
                    // DNS runs inside the connector: TCP starts after it.
                    let tcp_start = dns_end.filter(|d| *d >= start).unwrap_or(start);
                    let tcp_end = tls_start.unwrap_or(*end);
                    connection.connected = Some((tcp_start, tcp_end));
                    connection.tls = tls_start.map(|tls| (tls, *end));
                }
            }
        }
        connection
    }
}

/// Resolves names with the collection's DNS overrides, or else with the
/// system as reqwest does by default, and notes each lookup.
#[derive(Debug, Default, Clone)]
pub(crate) struct TimedResolver {
    overrides: Arc<[DnsOverride]>,
}

impl TimedResolver {
    /// A resolver that sends the hosts of `overrides` to their addresses.
    pub(crate) fn new(overrides: &[DnsOverride]) -> Self {
        Self {
            overrides: overrides.iter().filter(|o| o.enabled).cloned().collect(),
        }
    }
}

impl TimedResolver {
    /// The addresses of `host`, on `port`: those of its DNS override, or
    /// the system's.
    ///
    /// # Errors
    ///
    /// If the name cannot be resolved, or its override has no address.
    pub(crate) async fn addresses(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, apifast_app::TransportError> {
        use apifast_app::TransportError;
        match overridden_addresses(&self.overrides, host) {
            Some(ips) if ips.is_empty() => Err(TransportError::Dns(format!(
                "the DNS override of `{host}` has no valid IP address"
            ))),
            Some(ips) => Ok(ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()),
            None => tokio::net::lookup_host((host, port))
                .await
                .map(Iterator::collect)
                .map_err(|e| TransportError::Dns(format!("cannot resolve `{host}`: {e}"))),
        }
    }
}

impl Resolve for TimedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let overridden = overridden_addresses(&self.overrides, &host);
        Box::pin(async move {
            let start = Instant::now();
            let addresses: Vec<SocketAddr> = match &overridden {
                Some(ips) if ips.is_empty() => {
                    let why = format!("the DNS override of `{host}` has no valid IP address");
                    return Err(why.into());
                }
                // The port is set later by the connector.
                Some(ips) => ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect(),
                None => tokio::net::lookup_host((host.as_str(), 0)).await?.collect(),
            };
            note(Note::Dns {
                host,
                addresses: addresses.clone(),
                start,
                end: Instant::now(),
                overridden: overridden.is_some(),
            });
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}

/// A layer over reqwest's connector that notes when each new connection
/// starts and ends.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ProbeLayer;

impl<S> tower_layer::Layer<S> for ProbeLayer {
    type Service = ProbedConnector<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProbedConnector { inner }
    }
}

/// The connector, noting what it does.
#[derive(Debug, Clone)]
pub(crate) struct ProbedConnector<S> {
    inner: S,
}

impl<S, R> tower_service::Service<R> for ProbedConnector<S>
where
    S: tower_service::Service<R> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        note(Note::ConnectStart(Instant::now()));
        let connecting = self.inner.call(request);
        Box::pin(async move {
            let connected = connecting.await;
            if connected.is_ok() {
                note(Note::ConnectEnd(Instant::now()));
            }
            connected
        })
    }
}

/// rustls' session cache, noting when a handshake starts: rustls looks for
/// a ticket to resume as it writes its first message.
pub(crate) struct ProbedSessions(ClientSessionMemoryCache);

impl ProbedSessions {
    /// A cache as large as rustls' default.
    pub(crate) fn new() -> Self {
        Self(ClientSessionMemoryCache::new(256))
    }
}

impl fmt::Debug for ProbedSessions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProbedSessions")
    }
}

impl ClientSessionStore for ProbedSessions {
    fn set_kx_hint(&self, server_name: ServerName<'static>, group: rustls::NamedGroup) {
        self.0.set_kx_hint(server_name, group);
    }

    fn kx_hint(&self, server_name: &ServerName<'_>) -> Option<rustls::NamedGroup> {
        self.0.kx_hint(server_name)
    }

    fn set_tls12_session(&self, server_name: ServerName<'static>, value: Tls12ClientSessionValue) {
        self.0.set_tls12_session(server_name, value);
    }

    fn tls12_session(&self, server_name: &ServerName<'_>) -> Option<Tls12ClientSessionValue> {
        self.0.tls12_session(server_name)
    }

    fn remove_tls12_session(&self, server_name: &ServerName<'static>) {
        self.0.remove_tls12_session(server_name);
    }

    fn insert_tls13_ticket(
        &self,
        server_name: ServerName<'static>,
        value: Tls13ClientSessionValue,
    ) {
        self.0.insert_tls13_ticket(server_name, value);
    }

    fn take_tls13_ticket(
        &self,
        server_name: &ServerName<'static>,
    ) -> Option<Tls13ClientSessionValue> {
        // The first thing a handshake does with the store.
        note(Note::TlsStart(Instant::now()));
        self.0.take_tls13_ticket(server_name)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_notes_split_dns_tcp_and_tls() {
        let t = |ms| Instant::now() + Duration::from_millis(ms);
        let (t0, t1, t2, t3) = (t(0), t(1), t(3), t(6));
        let addr: SocketAddr = "10.0.0.1:0".parse().unwrap();
        let notes = [
            Note::ConnectStart(t0),
            Note::Dns {
                host: "api.test".into(),
                addresses: vec![addr],
                start: t0,
                end: t1,
                overridden: false,
            },
            Note::TlsStart(t2),
            Note::ConnectEnd(t3),
        ];
        let connection = Connection::of(&notes);
        assert_eq!(connection.dns.len(), 1);
        assert_eq!(connection.connected, Some((t1, t2)));
        assert_eq!(connection.tls, Some((t2, t3)));
    }

    #[test]
    fn plain_http_has_no_tls_and_a_pooled_connection_no_notes() {
        let t = |ms| Instant::now() + Duration::from_millis(ms);
        let (t0, t1) = (t(0), t(2));
        let connection = Connection::of(&[Note::ConnectStart(t0), Note::ConnectEnd(t1)]);
        assert_eq!(connection.connected, Some((t0, t1)));
        assert_eq!(connection.tls, None);
        assert_eq!(Connection::of(&[]), Connection::default());
    }

    #[tokio::test]
    async fn notes_reach_only_the_probe_of_their_task() {
        let ((), notes) = Probe::watch(async { note(Note::ConnectStart(Instant::now())) }).await;
        assert_eq!(notes.len(), 1);
        // Outside a probe, nothing is noted and nothing fails.
        note(Note::ConnectStart(Instant::now()));
    }
}
