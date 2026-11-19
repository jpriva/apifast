//! `apifast send` on a WebSocket request (docs/07, F7): it connects, sends
//! the request's messages in order and prints what arrives as it arrives,
//! until the server closes the connection or Ctrl+C closes it. A gRPC
//! request (F8) goes the same way: it is called with its messages, and
//! what the server answers is printed until it ends the call.
//!
//! Each message received goes to stdout on a line of its own (binary ones
//! in hexadecimal), so the output can be piped. What was sent, why a
//! message was not, and how the connection closed go to stderr.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use apifast_app::{BodyWriter, ResponseId, ResponseStore, StoreError, StoredBody, WsOutbox};
use apifast_domain::{WsData, WsEvent, WsLogParser, WsMessage, WsSide};
use apifast_history::{FsBodyWriter, FsResponseStore};

/// The messages of the request, in order; then nothing more, until the
/// connection closes, or, with `.1` (a gRPC call), the end of the client's
/// messages.
pub struct Messages(pub VecDeque<WsMessage>, pub bool);

impl WsOutbox for Messages {
    async fn next(&mut self) -> Option<WsMessage> {
        match self.0.pop_front() {
            Some(message) => Some(message),
            None if self.1 => None,
            None => std::future::pending().await,
        }
    }
}

/// How a call or connection ended, as the log said: the code and reason
/// of its close.
pub type Ended = Arc<Mutex<Option<(Option<u16>, String)>>>;

/// The response store, printing the connection's log as it is written.
pub struct Printing<'a> {
    pub store: &'a FsResponseStore,
    /// Only the messages received, and errors.
    pub quiet: bool,
    /// A gRPC call: its end is told by the summary, not on its own line.
    pub grpc: bool,
    /// Where the end is kept.
    pub ended: Ended,
}

impl ResponseStore for Printing<'_> {
    type Writer = PrintingWriter;

    fn create(&self) -> Result<PrintingWriter, StoreError> {
        Ok(PrintingWriter {
            inner: self.store.create()?,
            parser: WsLogParser::new(),
            quiet: self.quiet,
            grpc: self.grpc,
            ended: self.ended.clone(),
        })
    }

    fn read(&self, id: &ResponseId, offset: u64, len: usize) -> Result<Vec<u8>, StoreError> {
        self.store.read(id, offset, len)
    }
}

/// Writes the log to the store and prints its entries.
pub struct PrintingWriter {
    inner: FsBodyWriter,
    parser: WsLogParser,
    quiet: bool,
    grpc: bool,
    ended: Ended,
}

impl BodyWriter for PrintingWriter {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        self.inner.write(chunk)?;
        for entry in self.parser.push(chunk) {
            if let WsEvent::Closed { code, reason, .. } = &entry.event {
                *self.ended.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((*code, reason.clone()));
            }
            print(&entry.event, self.quiet, self.grpc);
        }
        Ok(())
    }

    fn finish(self) -> Result<StoredBody, StoreError> {
        self.inner.finish()
    }
}

fn print(event: &WsEvent, quiet: bool, grpc: bool) {
    match event {
        WsEvent::Received(data) => {
            let mut out = std::io::stdout().lock();
            // A closed pipe (as in `| head`) only stops the printing.
            let _ = writeln!(out, "{}", line(data)).and_then(|()| out.flush());
        }
        WsEvent::Sent(data) if !quiet => eprintln!("> {}", line(data)),
        WsEvent::Failed(why) => eprintln!("warning: {why}"),
        WsEvent::Closed { by, code, reason } if !quiet && !grpc => {
            let by = match by {
                WsSide::Client => "closed",
                WsSide::Server => "closed by the server",
            };
            let code = code.map(|c| format!(" {c}")).unwrap_or_default();
            let reason = if reason.is_empty() {
                String::new()
            } else {
                format!(" {reason}")
            };
            eprintln!("{by}{code}{reason}");
        }
        _ => {}
    }
}

/// A message on one line: text as it is, binary in hexadecimal.
fn line(data: &WsData) -> String {
    match data {
        WsData::Text(text) => text.clone(),
        WsData::Binary(bytes) => bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        }),
    }
}
