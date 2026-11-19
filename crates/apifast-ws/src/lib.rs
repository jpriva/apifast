//! WebSocket transport of apifast (docs/07, F7): the handshake goes through
//! the HTTP transport ([`ReqwestTransport::upgrade`]), so it has the
//! collection's proxy, TLS, certificates, DNS overrides and cookies; then
//! tungstenite reads and writes the frames.
//!
//! Must run inside a tokio runtime with the time driver enabled.

use std::time::{Duration, Instant};

use apifast_app::{
    CancelToken, RenderedRequest, ResponseHead, TransportError, WsHandler, WsTransport,
};
use apifast_domain::{WsData, WsEvent, WsSide};
use apifast_http::{ReqwestTransport, Upgraded};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

/// How long a closing connection waits for the other side's close.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// How often an open connection checks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// The real [`WsTransport`].
#[derive(Debug, Clone)]
pub struct TungsteniteTransport {
    http: ReqwestTransport,
}

impl TungsteniteTransport {
    /// A transport whose handshakes go through `http`, with its settings.
    pub fn new(http: ReqwestTransport) -> Self {
        Self { http }
    }
}

impl WsTransport for TungsteniteTransport {
    async fn connect<H: WsHandler>(
        &self,
        request: &RenderedRequest,
        handler: &mut H,
        cancel: &CancelToken,
    ) -> Result<ResponseHead, TransportError> {
        let started = Instant::now();
        let key = generate_key();
        let protocol = [
            ("connection", "Upgrade".to_owned()),
            ("upgrade", "websocket".to_owned()),
            ("sec-websocket-version", "13".to_owned()),
            ("sec-websocket-key", key.clone()),
        ];
        let (mut head, upgraded) = self.http.upgrade(request, &protocol, cancel).await?;
        let accept = (head.headers.iter())
            .find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-accept"))
            .map(|(_, value)| value.trim());
        if accept != Some(derive_accept_key(key.as_bytes()).as_str()) {
            return Err(TransportError::Protocol(
                "the server switched protocols without accepting the WebSocket key".into(),
            ));
        }
        let stream = WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await;
        handler.opened(&head);
        exchange(stream, handler, cancel).await;
        head.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(head)
    }
}

/// What woke the connection up.
enum Step {
    /// Data to send, or `None` to close.
    Send(Option<WsData>),
    /// What the network gave: a message, an error, or the end.
    Receive(Option<Result<Message, WsError>>),
}

/// Sends what `handler` gives and tells it what arrives, until the
/// connection closes.
async fn exchange<H: WsHandler>(
    mut stream: WebSocketStream<Upgraded>,
    handler: &mut H,
    cancel: &CancelToken,
) {
    loop {
        let step = tokio::select! {
            data = handler.outgoing() => Step::Send(data),
            message = stream.next() => Step::Receive(message),
            () = cancelled(cancel) => Step::Send(None),
        };
        match step {
            Step::Send(Some(data)) => {
                let message = match &data {
                    WsData::Text(text) => Message::text(text.as_str()),
                    WsData::Binary(bytes) => Message::binary(bytes.clone()),
                };
                if let Err(error) = stream.send(message).await {
                    broken(handler, &error);
                    return;
                }
                handler.event(WsEvent::Sent(data));
            }
            Step::Send(None) => {
                let frame = CloseFrame {
                    code: CloseCode::Normal,
                    reason: "".into(),
                };
                if let Err(error) = stream.close(Some(frame)).await {
                    broken(handler, &error);
                    return;
                }
                drain(&mut stream).await;
                handler.event(WsEvent::Closed {
                    by: WsSide::Client,
                    code: Some(CloseCode::Normal.into()),
                    reason: String::new(),
                });
                return;
            }
            Step::Receive(Some(Ok(message))) => match message {
                Message::Text(text) => {
                    handler.event(WsEvent::Received(WsData::Text(text.as_str().to_owned())));
                }
                Message::Binary(bytes) => {
                    handler.event(WsEvent::Received(WsData::Binary(bytes.to_vec())));
                }
                Message::Close(frame) => {
                    // tungstenite answers the close; it goes out as the
                    // stream is read to its end.
                    drain(&mut stream).await;
                    handler.event(WsEvent::Closed {
                        by: WsSide::Server,
                        code: frame.as_ref().map(|f| f.code.into()),
                        reason: frame
                            .map(|f| f.reason.as_str().to_owned())
                            .unwrap_or_default(),
                    });
                    return;
                }
                // Pings are answered by tungstenite.
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            },
            Step::Receive(Some(Err(error))) => {
                broken(handler, &error);
                return;
            }
            Step::Receive(None) => {
                handler.event(WsEvent::Closed {
                    by: WsSide::Server,
                    code: None,
                    reason: String::new(),
                });
                return;
            }
        }
    }
}

/// The connection broke: the handler is told why, and that it closed.
fn broken<H: WsHandler>(handler: &mut H, error: &WsError) {
    if !matches!(error, WsError::ConnectionClosed | WsError::AlreadyClosed) {
        handler.event(WsEvent::Failed(error.to_string()));
    }
    handler.event(WsEvent::Closed {
        by: WsSide::Server,
        code: None,
        reason: String::new(),
    });
}

/// Reads the stream to its end, for the close handshake to finish, for at
/// most [`CLOSE_WAIT`].
async fn drain(stream: &mut WebSocketStream<Upgraded>) {
    let _ = tokio::time::timeout(CLOSE_WAIT, async {
        while let Some(Ok(_)) = stream.next().await {}
    })
    .await;
}

async fn cancelled(token: &CancelToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}
