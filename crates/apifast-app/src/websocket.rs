//! WebSocket connections (docs/07, F7): a WebSocket request is rendered
//! like any other (inheritance, templates, auth) and its handshake sent by a
//! [`WsTransport`]; then the messages the user sends are rendered one by one
//! as they go, and everything that happens is written to a log, the body of
//! the connection's history entry ([`apifast_domain::WsLogEntry`]).

use std::future::Future;

use apifast_domain::{
    HttpRequest, RequestKind, ResolvedVariables, Secret, WsData, WsEvent, WsLogEntry, WsMessage,
    mask,
};
use apifast_template::{Mode, render};

use crate::use_cases::{ChainContext, Prepared, host_for, prepare};
use crate::{
    AppError, AppHost, BodyWriter, CancelToken, Clock, CollectionRepository, EnvironmentRepository,
    HttpTransport, RenderedRequest, RequestPath, ResponseHead, ResponseId, ResponseStore,
    SendOptions, SendOutcome, SendPorts, SentRequest, StoreError, TransportError,
};

/// The messages the user sends over an open connection.
pub trait WsOutbox: Send {
    /// The next message to send: pending until there is one, `None` to
    /// close the connection.
    ///
    /// Dropping the future before it finishes must lose no message: the
    /// connection waits on it and on the network at once.
    fn next(&mut self) -> impl Future<Output = Option<WsMessage>> + Send;
}

/// What a [`WsTransport`] talks to while a connection is open.
pub trait WsHandler: Send {
    /// The handshake's response arrived: the connection is open.
    fn opened(&mut self, head: &ResponseHead);

    /// The next data to send: pending until there is some, `None` to close
    /// the connection. Cancel-safe, as [`WsOutbox::next`].
    fn outgoing(&mut self) -> impl Future<Output = Option<WsData>> + Send;

    /// Something happened: a message went out or arrived, or the connection
    /// broke or closed.
    fn event(&mut self, event: WsEvent);
}

/// Opens WebSocket connections.
pub trait WsTransport {
    /// Sends the handshake of `request` and, once open, sends what
    /// `handler` gives and tells it what happens, until either side closes
    /// the connection or `cancel` asks to (which closes it normally).
    ///
    /// Returns the handshake's response, its `elapsed_ms` the time the
    /// connection was open. A connection that breaks once open is not an
    /// error: the handler is told, and the log so far kept.
    ///
    /// # Errors
    ///
    /// If the handshake fails: connection, TLS, timeout, cancellation, or a
    /// server that does not switch to WebSocket.
    fn connect<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send;
}

/// Connects to the WebSocket request at `path` and keeps the connection
/// until it closes: `outbox` gives the messages to send, which are rendered
/// with the request's variables as they go. Everything that happens is
/// written to a log in `ports.responses`, which the outcome's `body` is.
///
/// A message that does not render (a missing variable, unless
/// `allow_unresolved`) is not sent; the log says why, and the connection
/// stays open. Secret values are masked in the log.
///
/// # Errors
///
/// As [`send_request`](crate::send_request) up to the handshake, and if the
/// request is not a WebSocket one or the handshake fails.
pub async fn connect_request<C, E, T, S, W, O>(
    ports: SendPorts<'_, C, E, T, S>,
    (sockets, outbox): (&W, &mut O),
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
) -> Result<SendOutcome, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
    W: WsTransport + Sync,
    O: WsOutbox,
{
    exchange(ports, (&Socket(sockets), outbox), path, options, cancel).await
}

/// What runs an exchange of messages once the request is rendered: a
/// WebSocket connection, or a gRPC call.
pub(crate) trait Exchange {
    /// The kind of request it runs.
    const KIND: RequestKind;

    /// Runs the exchange of `request` (loaded as `loaded`), telling
    /// `handler` what happens; returns the response's head.
    fn run<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        loaded: &HttpRequest,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send;
}

/// A WebSocket connection, as an [`Exchange`].
struct Socket<'w, W>(&'w W);

impl<W: WsTransport + Sync> Exchange for Socket<'_, W> {
    const KIND: RequestKind = RequestKind::WebSocket;

    fn run<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        _: &HttpRequest,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send {
        self.0.connect(request, handler, cancel)
    }
}

/// Prepares the request at `path` as `X::KIND`, and runs it with `runner`:
/// `outbox` gives the messages to send, which are rendered as they go, and
/// what happens is written to a log, the outcome's body.
pub(crate) async fn exchange<C, E, T, S, X, O>(
    ports: SendPorts<'_, C, E, T, S>,
    (runner, outbox): (&X, &mut O),
    path: &RequestPath,
    options: &SendOptions,
    cancel: &CancelToken,
) -> Result<SendOutcome, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
    X: Exchange + Sync,
    O: WsOutbox,
{
    let mut context = ChainContext::default();
    let read = |id: &ResponseId, len: usize| ports.responses.read(id, 0, len);
    let mut host = host_for(&ports, &read)?;
    let Prepared {
        loaded,
        variables,
        rendered,
        scripts,
    } = prepare(
        &ports,
        path,
        options,
        cancel,
        (&mut context, &mut host, X::KIND),
    )
    .await?;
    let mut handler = Connection {
        outbox,
        variables: &variables,
        host: &mut host,
        mode: if options.allow_unresolved {
            Mode::Lenient
        } else {
            Mode::Strict
        },
        secrets: rendered.request.secrets().to_vec(),
        log: ports.responses.create()?,
        clock: ports.clock,
        opened_ms: ports.clock.unix_millis(),
        failed: None,
        report: ports.report,
    };
    let head = runner
        .run(&rendered.request, &loaded.request, &mut handler, cancel)
        .await?;
    if let Some(error) = handler.failed {
        return Err(error.into());
    }
    let secrets = handler.secrets;
    let body = handler.log.finish()?;
    let sent = SentRequest::of(&rendered.request);
    Ok(SendOutcome {
        head,
        body,
        problems: rendered.problems,
        contains_secret: !secrets.is_empty() || rendered.request.contains_secret(),
        secrets,
        captures: Vec::new(),
        assertions: Vec::new(),
        dependencies: context.recorded,
        scripts: scripts.map(|s| s.report).unwrap_or_default(),
        sent,
    })
}

/// The open connection, as its transport sees it.
struct Connection<'c, 'h, O, L> {
    outbox: &'c mut O,
    variables: &'c ResolvedVariables,
    host: &'c mut AppHost<'h>,
    mode: Mode,
    /// The secrets the handshake and the messages carried, to mask them.
    secrets: Vec<Secret>,
    log: L,
    clock: &'c dyn Clock,
    opened_ms: i64,
    /// The first error writing the log.
    failed: Option<StoreError>,
    report: &'c (dyn Fn(&apifast_template::Problem) + Sync),
}

impl<O: WsOutbox, L: BodyWriter + Send> Connection<'_, '_, O, L> {
    fn write(&mut self, event: WsEvent) {
        if self.failed.is_some() {
            return;
        }
        let at = self.clock.unix_millis().saturating_sub(self.opened_ms);
        let event = match event {
            WsEvent::Sent(WsData::Text(text)) => {
                WsEvent::Sent(WsData::Text(mask(&text, &self.secrets).into_owned()))
            }
            other => other,
        };
        let entry = WsLogEntry {
            at_ms: u64::try_from(at).unwrap_or(0),
            event,
        };
        if let Err(error) = self.log.write(entry.to_line().as_bytes()) {
            self.failed = Some(error);
        }
    }

    /// `message` rendered, or why it cannot be.
    fn render(&mut self, message: &WsMessage) -> Result<String, String> {
        let rendered = render(&message.data, self.variables, &mut *self.host, self.mode)
            .map_err(|e| format!("not sent: {e}"))?;
        rendered.problems.iter().for_each(self.report);
        self.secrets.extend(rendered.secrets);
        Ok(rendered.text.into_owned())
    }
}

impl<O: WsOutbox, L: BodyWriter + Send> WsHandler for Connection<'_, '_, O, L> {
    fn opened(&mut self, head: &ResponseHead) {
        self.opened_ms = self.clock.unix_millis();
        self.log.begin(head);
    }

    async fn outgoing(&mut self) -> Option<WsData> {
        loop {
            let message = self.outbox.next().await?;
            match self.render(&message) {
                Ok(text) => return Some(WsData::Text(text)),
                Err(why) => self.write(WsEvent::Failed(why)),
            }
        }
    }

    fn event(&mut self, event: WsEvent) {
        self.write(event);
    }
}
