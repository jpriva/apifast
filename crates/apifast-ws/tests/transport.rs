//! The WebSocket transport against a local server. No test uses the
//! external network.

#![allow(
    clippy::unwrap_used,
    clippy::result_large_err,
    reason = "test helpers: a failure fails the test; tungstenite's callback type"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use apifast_app::memory::{CountingRandom, FixedClock};
use apifast_app::{
    AppHost, CancelToken, NoPrompter, RenderedRequest, ResponseHead, TransportError, WsHandler,
    WsTransport, render_request,
};
use apifast_domain::{
    AuthScheme, BearerAuth, Chain, Collection, Header, HttpRequest, Method, ResolvedVariables,
    Setting, WsData, WsEvent, WsSide, resolve,
};
use apifast_http::ReqwestTransport;
use apifast_template::Mode;
use apifast_ws::TungsteniteTransport;
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// A server that echoes text, answers `binary` with three bytes and closes
/// with 4000 on `bye`. Keeps the handshake's headers.
async fn server() -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/live", listener.local_addr().unwrap());
    let headers = Arc::new(Mutex::new(Vec::new()));
    let kept = headers.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let kept = kept.clone();
            tokio::spawn(async move {
                let callback = |request: &Request, response: Response| {
                    *kept.lock().unwrap() = (request.headers().iter())
                        .map(|(n, v)| (n.to_string(), v.to_str().unwrap().to_owned()))
                        .collect();
                    Ok(response)
                };
                let mut socket = tokio_tungstenite::accept_hdr_async(tcp, callback)
                    .await
                    .unwrap();
                while let Some(Ok(message)) = socket.next().await {
                    let answer = match message {
                        Message::Text(text) if text.as_str() == "bye" => {
                            Message::Close(Some(CloseFrame {
                                code: CloseCode::from(4000),
                                reason: "done".into(),
                            }))
                        }
                        Message::Text(text) if text.as_str() == "binary" => {
                            Message::binary(vec![1, 2, 3])
                        }
                        Message::Text(text) => Message::Text(text),
                        _ => continue,
                    };
                    if socket.send(answer).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (url, headers)
}

fn rendered(url: &str) -> RenderedRequest {
    let mut collection = Collection::new("t");
    collection.defaults.headers = vec![Header::new("X-Client", "apifast")];
    collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
        token: "t0k3n".into(),
    }));
    let request = HttpRequest::new("socket", Method::Get, url);
    let (clock, random) = (FixedClock(0), CountingRandom::new(0));
    let mut host = AppHost::new(&clock, &random, &NoPrompter, BTreeMap::new());
    render_request(
        &resolve(Chain::new(&collection, &[], &request)),
        &ResolvedVariables::default(),
        &mut host,
        Mode::Strict,
    )
    .unwrap()
    .request
}

/// Sends what comes down its channel and keeps what happens.
struct Recorder {
    outbox: mpsc::UnboundedReceiver<WsData>,
    opened: Option<ResponseHead>,
    events: Vec<WsEvent>,
    /// Cancelled once the connection opens.
    cancel_on_open: Option<CancelToken>,
}

impl Recorder {
    fn new() -> (Self, mpsc::UnboundedSender<WsData>) {
        let (sender, outbox) = mpsc::unbounded_channel();
        let recorder = Self {
            outbox,
            opened: None,
            events: Vec::new(),
            cancel_on_open: None,
        };
        (recorder, sender)
    }
}

impl WsHandler for Recorder {
    fn opened(&mut self, head: &ResponseHead) {
        self.opened = Some(head.clone());
        if let Some(cancel) = &self.cancel_on_open {
            cancel.cancel();
        }
    }

    async fn outgoing(&mut self) -> Option<WsData> {
        self.outbox.recv().await
    }

    fn event(&mut self, event: WsEvent) {
        self.events.push(event);
    }
}

fn transport() -> TungsteniteTransport {
    TungsteniteTransport::new(ReqwestTransport::new(".").unwrap())
}

fn text(data: &str) -> WsData {
    WsData::Text(data.into())
}

#[tokio::test]
async fn messages_go_both_ways_until_the_server_closes() {
    let (url, headers) = server().await;
    let (mut recorder, outbox) = Recorder::new();
    for message in ["hello", "binary", "bye"] {
        outbox.send(text(message)).unwrap();
    }
    let head = transport()
        .connect(&rendered(&url), &mut recorder, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(head.status, 101);
    assert_eq!(head.url, url);
    // Told when it opened; the time is the whole connection's once closed.
    let opened = recorder.opened.clone().unwrap();
    assert_eq!(opened.headers, head.headers);
    assert!(opened.elapsed_ms <= head.elapsed_ms);
    let headers = headers.lock().unwrap().clone();
    assert!(headers.contains(&("x-client".into(), "apifast".into())));
    assert!(headers.contains(&("authorization".into(), "Bearer t0k3n".into())));

    let received: Vec<&WsEvent> = (recorder.events.iter())
        .filter(|e| matches!(e, WsEvent::Received(_)))
        .collect();
    assert_eq!(
        received,
        [
            &WsEvent::Received(text("hello")),
            &WsEvent::Received(WsData::Binary(vec![1, 2, 3]))
        ]
    );
    assert_eq!(
        recorder
            .events
            .iter()
            .filter(|e| matches!(e, WsEvent::Sent(_)))
            .count(),
        3
    );
    assert_eq!(
        recorder.events.last(),
        Some(&WsEvent::Closed {
            by: WsSide::Server,
            code: Some(4000),
            reason: "done".into(),
        })
    );
    drop(outbox);
}

#[tokio::test]
async fn cancelling_closes_the_connection_normally() {
    let (url, _) = server().await;
    let (mut recorder, _outbox) = Recorder::new();
    let cancel = CancelToken::new();
    recorder.cancel_on_open = Some(cancel.clone());
    let head = transport()
        .connect(&rendered(&url), &mut recorder, &cancel)
        .await
        .unwrap();
    assert_eq!(head.status, 101);
    assert_eq!(
        recorder.events,
        [WsEvent::Closed {
            by: WsSide::Client,
            code: Some(1000),
            reason: String::new(),
        }]
    );
}

#[tokio::test]
async fn a_server_that_does_not_switch_protocols_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let _ = tcp.read(&mut buffer).await;
        let _ = tcp
            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
            .await;
    });
    let (mut recorder, _outbox) = Recorder::new();
    let error = transport()
        .connect(&rendered(&url), &mut recorder, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        TransportError::Protocol(
            "the server answered 404 Not Found instead of switching protocols".into()
        )
    );
    assert!(recorder.opened.is_none());
}
