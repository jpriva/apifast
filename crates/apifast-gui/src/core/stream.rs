//! An event stream or a WebSocket connection while it lasts (docs/07, F7):
//! the send's body writer reads its events, or the connection's log, as it
//! stores them, into a list the response panel shows live, and tells the
//! core thread so the window is drawn again.
//!
//! The list is shared, not copied into each snapshot: a long stream would
//! otherwise be cloned on every event.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use apifast_app::{
    BodyWriter, ResponseHead, ResponseId, ResponseStore, StoreError, StoredBody as AppStoredBody,
};
use apifast_domain::{
    SseEvent, SseParser, WsLogEntry, WsLogParser, is_event_stream, is_websocket_handshake,
};
use apifast_history::FsResponseStore;
use tokio::sync::mpsc;

use super::Message;

/// The most events (or messages) kept in the live list: older ones leave
/// it, and stay in the stored body.
pub const MAX_LIVE_EVENTS: usize = 10_000;

/// What arrived of an event stream so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Live {
    /// The response's status and headers.
    pub head: Option<ResponseHead>,
    /// The last events of an event stream, oldest first.
    pub events: Vec<SseEvent>,
    /// The last entries of a WebSocket connection's log, oldest first.
    pub messages: Vec<WsLogEntry>,
    /// Whether it is a WebSocket connection, not an event stream.
    pub socket: bool,
    /// How many events or entries left the list, to number the rest.
    pub dropped: usize,
    /// Bytes of the body so far.
    pub bytes: u64,
}

/// A stream being received, shared by the writer and the interface.
#[derive(Clone, Default)]
pub struct LiveStream {
    live: Arc<Mutex<Live>>,
    /// The core was told about a change it has not handled yet.
    pending: Arc<AtomicBool>,
}

impl LiveStream {
    /// What arrived so far.
    pub fn read<T>(&self, read: impl FnOnce(&Live) -> T) -> T {
        read(&self.live.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn change(&self, change: impl FnOnce(&mut Live)) {
        change(&mut self.live.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Whether this is the first news since the core last handled one.
    fn first_news(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// The core handled the news.
    pub(super) fn handled(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

impl From<Live> for LiveStream {
    /// A stream with `live` arrived, for the gallery and tests.
    fn from(live: Live) -> Self {
        Self {
            live: Arc::new(Mutex::new(live)),
            pending: Arc::default(),
        }
    }
}

impl PartialEq for LiveStream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.live, &other.live)
    }
}

impl Eq for LiveStream {}

impl fmt::Debug for LiveStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.read(|live| {
            f.debug_struct("LiveStream")
                .field("events", &live.events.len())
                .field("messages", &live.messages.len())
                .field("bytes", &live.bytes)
                .finish_non_exhaustive()
        })
    }
}

/// The response store of one send: bodies go to `store`, and one that is
/// an event stream or a WebSocket log is also read into `stream`.
pub(super) struct LiveResponses {
    pub store: Arc<FsResponseStore>,
    pub stream: LiveStream,
    /// The send's number, to tell the core.
    pub id: u64,
    pub messages: mpsc::WeakUnboundedSender<Message>,
}

impl LiveResponses {
    fn tell(&self) {
        if self.stream.first_news()
            && let Some(messages) = self.messages.upgrade()
        {
            let _ = messages.send(Message::Streamed(self.id));
        }
    }
}

impl ResponseStore for LiveResponses {
    type Writer = LiveWriter;

    fn create(&self) -> Result<LiveWriter, StoreError> {
        Ok(LiveWriter {
            inner: self.store.create()?,
            parser: None,
            to: LiveResponses {
                store: self.store.clone(),
                stream: self.stream.clone(),
                id: self.id,
                messages: self.messages.clone(),
            },
        })
    }

    fn read(&self, id: &ResponseId, offset: u64, len: usize) -> Result<Vec<u8>, StoreError> {
        self.store.read(id, offset, len)
    }
}

/// How a body read live is read.
enum Parser {
    Sse(SseParser),
    Ws(WsLogParser),
}

/// Writes a body to the store and, for an event stream or a WebSocket
/// log, reads its entries.
pub(super) struct LiveWriter {
    inner: <FsResponseStore as ResponseStore>::Writer,
    parser: Option<Parser>,
    to: LiveResponses,
}

impl BodyWriter for LiveWriter {
    fn begin(&mut self, head: &ResponseHead) {
        let socket = is_websocket_handshake(head.status, &head.headers);
        let stream = (head.headers.iter()).any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type") && is_event_stream(value)
        });
        self.parser = if socket {
            Some(Parser::Ws(WsLogParser::new()))
        } else if stream {
            Some(Parser::Sse(SseParser::new()))
        } else {
            return;
        };
        self.to.stream.change(|live| {
            *live = Live {
                head: Some(head.clone()),
                socket,
                ..Live::default()
            };
        });
        self.to.tell();
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        self.inner.write(chunk)?;
        let Some(parser) = &mut self.parser else {
            return Ok(());
        };
        let over = |len: usize| len.saturating_sub(MAX_LIVE_EVENTS);
        match parser {
            Parser::Sse(parser) => {
                let events = parser.push(chunk);
                self.to.stream.change(|live| {
                    live.events.extend(events);
                    let over = over(live.events.len());
                    live.events.drain(..over);
                    live.dropped += over;
                });
            }
            Parser::Ws(parser) => {
                let entries = parser.push(chunk);
                self.to.stream.change(|live| {
                    live.messages.extend(entries);
                    let over = over(live.messages.len());
                    live.messages.drain(..over);
                    live.dropped += over;
                });
            }
        }
        self.to
            .stream
            .change(|live| live.bytes += chunk.len() as u64);
        self.to.tell();
        Ok(())
    }

    fn finish(self) -> Result<AppStoredBody, StoreError> {
        self.inner.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(content_type: &str) -> ResponseHead {
        ResponseHead {
            status: 200,
            version: "HTTP/1.1".into(),
            url: "http://localhost/".into(),
            redirects: 0,
            headers: vec![("content-type".into(), content_type.into())],
            elapsed_ms: 0,
            timeline: Vec::new(),
        }
    }

    #[test]
    fn a_stream_is_read_live_and_told_once_until_handled() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let responses = LiveResponses {
            store: Arc::new(FsResponseStore::new(dir.path().to_path_buf())),
            stream: LiveStream::default(),
            id: 7,
            messages: sender.downgrade(),
        };
        let mut writer = responses.create().unwrap();
        writer.begin(&head("text/event-stream"));
        writer.write(b"data: a\n\nda").unwrap();
        writer.write(b"ta: b\n\n").unwrap();
        let told = std::iter::from_fn(|| receiver.try_recv().ok()).count();
        assert_eq!(told, 1);
        responses.stream.handled();
        writer.write(b"data: c\n\n").unwrap();
        assert!(matches!(receiver.try_recv(), Ok(Message::Streamed(7))));
        let data: Vec<String> = responses
            .stream
            .read(|live| live.events.iter().map(|e| e.data.clone()).collect());
        assert_eq!(data, ["a", "b", "c"]);
        let stored = writer.finish().unwrap();
        let body = responses.read(&stored.id, 0, 100).unwrap();
        assert_eq!(body, b"data: a\n\ndata: b\n\ndata: c\n\n");
    }

    #[test]
    fn a_websocket_log_is_read_live() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, _receiver) = mpsc::unbounded_channel();
        let responses = LiveResponses {
            store: Arc::new(FsResponseStore::new(dir.path().to_path_buf())),
            stream: LiveStream::default(),
            id: 2,
            messages: sender.downgrade(),
        };
        let mut writer = responses.create().unwrap();
        let mut handshake = head("text/plain");
        handshake.status = 101;
        handshake.headers = vec![("upgrade".into(), "websocket".into())];
        writer.begin(&handshake);
        writer
            .write(b"{\"at\":1,\"type\":\"received\",\"text\":\"hi\"}\n{\"at\":2,")
            .unwrap();
        assert!(responses.stream.read(|live| live.socket));
        assert_eq!(responses.stream.read(|live| live.messages.len()), 1);
        writer
            .write(b"\"type\":\"sent\",\"text\":\"yo\"}\n")
            .unwrap();
        let events: Vec<apifast_domain::WsEvent> = responses
            .stream
            .read(|live| live.messages.iter().map(|m| m.event.clone()).collect());
        assert_eq!(
            events,
            [
                apifast_domain::WsEvent::Received(apifast_domain::WsData::Text("hi".into())),
                apifast_domain::WsEvent::Sent(apifast_domain::WsData::Text("yo".into())),
            ]
        );
    }

    #[test]
    fn another_body_is_only_stored() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let responses = LiveResponses {
            store: Arc::new(FsResponseStore::new(dir.path().to_path_buf())),
            stream: LiveStream::default(),
            id: 1,
            messages: sender.downgrade(),
        };
        let mut writer = responses.create().unwrap();
        writer.begin(&head("text/plain"));
        writer.write(b"data: a\n\n").unwrap();
        assert!(receiver.try_recv().is_err());
        assert_eq!(responses.stream.read(|live| live.events.len()), 0);
    }
}
