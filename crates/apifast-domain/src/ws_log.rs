//! The log of a WebSocket connection (docs/07, F7): what was sent and
//! received, and how it ended, one JSON object per line.
//!
//! The log is the body of the connection's history entry, so it is kept on
//! disk like any response body and read back in pieces:
//!
//! ```text
//! {"at":0,"type":"sent","text":"{\"op\":\"subscribe\"}"}
//! {"at":12,"type":"received","binary":"00ff"}
//! {"at":40,"type":"error","message":"a variable is missing"}
//! {"at":95,"type":"closed","by":"server","code":1000,"reason":"bye"}
//! ```
//!
//! `at` is in milliseconds since the connection opened. Binary data is
//! written in hexadecimal.

use std::fmt::Write as _;

use crate::Json;

/// The `Upgrade` header value of a WebSocket handshake, which tells a
/// connection's history entry from a response's.
pub const WEBSOCKET_UPGRADE: &str = "websocket";

/// The data of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsData {
    /// A text frame.
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

impl WsData {
    /// Its size in bytes.
    pub fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Binary(bytes) => bytes.len(),
        }
    }

    /// Whether it has no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Which side ended the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsSide {
    /// apifast: the user disconnected.
    Client,
    /// The server, or the network.
    Server,
}

/// Something that happened on a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    /// A message went out.
    Sent(WsData),
    /// A message arrived.
    Received(WsData),
    /// A message could not be sent, or the connection broke; the text says
    /// why.
    Failed(String),
    /// The connection closed.
    Closed {
        /// Who closed it.
        by: WsSide,
        /// The close code, when one was given.
        code: Option<u16>,
        /// The close reason; empty when there was none.
        reason: String,
    },
}

/// A line of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsLogEntry {
    /// Milliseconds since the connection opened.
    pub at_ms: u64,
    /// What happened.
    pub event: WsEvent,
}

impl WsLogEntry {
    /// The entry as a line of the log, with its line feed.
    pub fn to_line(&self) -> String {
        let text = |s: &str| Json::String(s.to_owned()).to_string();
        let mut line = format!("{{\"at\":{},\"type\":", self.at_ms);
        let data = |line: &mut String, data: &WsData| match data {
            WsData::Text(t) => {
                let _ = write!(line, ",\"text\":{}", text(t));
            }
            WsData::Binary(bytes) => {
                line.push_str(",\"binary\":\"");
                for byte in bytes {
                    let _ = write!(line, "{byte:02x}");
                }
                line.push('"');
            }
        };
        match &self.event {
            WsEvent::Sent(d) => {
                line.push_str("\"sent\"");
                data(&mut line, d);
            }
            WsEvent::Received(d) => {
                line.push_str("\"received\"");
                data(&mut line, d);
            }
            WsEvent::Failed(message) => {
                let _ = write!(line, "\"error\",\"message\":{}", text(message));
            }
            WsEvent::Closed { by, code, reason } => {
                let by = match by {
                    WsSide::Client => "client",
                    WsSide::Server => "server",
                };
                let _ = write!(line, "\"closed\",\"by\":\"{by}\"");
                if let Some(code) = code {
                    let _ = write!(line, ",\"code\":{code}");
                }
                let _ = write!(line, ",\"reason\":{}", text(reason));
            }
        }
        line.push_str("}\n");
        line
    }

    /// Reads a line of the log, without its line feed; `None` if it is not
    /// one.
    pub fn parse_line(line: &str) -> Option<Self> {
        let json = Json::parse(line).ok()?;
        let at_ms = whole(json.get("at")?)?;
        let string = |key: &str| match json.get(key) {
            Some(Json::String(s)) => Some(s.clone()),
            _ => None,
        };
        let data = || {
            if let Some(text) = string("text") {
                return Some(WsData::Text(text));
            }
            from_hex(&string("binary")?).map(WsData::Binary)
        };
        let event = match string("type")?.as_str() {
            "sent" => WsEvent::Sent(data()?),
            "received" => WsEvent::Received(data()?),
            "error" => WsEvent::Failed(string("message")?),
            "closed" => WsEvent::Closed {
                by: match string("by")?.as_str() {
                    "client" => WsSide::Client,
                    _ => WsSide::Server,
                },
                code: (json.get("code").and_then(whole)).and_then(|c| u16::try_from(c).ok()),
                reason: string("reason").unwrap_or_default(),
            },
            _ => return None,
        };
        Some(Self { at_ms, event })
    }
}

/// A JSON number that is a whole number of at most 15 digits.
fn whole(json: &Json) -> Option<u64> {
    match json {
        Json::Number(text) if text.len() <= 15 => text.parse().ok(),
        _ => None,
    }
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Reads a log given in chunks of any size.
#[derive(Debug, Default)]
pub struct WsLogParser {
    /// The unfinished line.
    line: Vec<u8>,
}

impl WsLogParser {
    /// A parser at the start of a log.
    pub fn new() -> Self {
        Self::default()
    }

    /// The entries the lines that `chunk` ends hold. Lines that are not
    /// entries are skipped.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<WsLogEntry> {
        let mut entries = Vec::new();
        let mut rest = chunk;
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            self.line.extend_from_slice(&rest[..end]);
            rest = &rest[end + 1..];
            if let Some(entry) = std::str::from_utf8(&self.line)
                .ok()
                .and_then(WsLogEntry::parse_line)
            {
                entries.push(entry);
            }
            self.line.clear();
        }
        self.line.extend_from_slice(rest);
        entries
    }
}

/// The entries of a whole log.
pub fn parse_ws_log(log: &[u8]) -> Vec<WsLogEntry> {
    WsLogParser::new().push(log)
}

/// Whether a response's headers are those of a WebSocket handshake: its
/// stored body is then the connection's log.
pub fn is_websocket_handshake(status: u16, headers: &[(String, String)]) -> bool {
    status == 101
        && headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("upgrade")
                && value.trim().eq_ignore_ascii_case(WEBSOCKET_UPGRADE)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at_ms: u64, event: WsEvent) -> WsLogEntry {
        WsLogEntry { at_ms, event }
    }

    #[test]
    fn entries_are_written_one_per_line_and_read_back() {
        let entries = [
            entry(0, WsEvent::Sent(WsData::Text("{\"a\":\"é\\n\"}".into()))),
            entry(12, WsEvent::Received(WsData::Binary(vec![0, 0xff, 0x10]))),
            entry(13, WsEvent::Received(WsData::Text(String::new()))),
            entry(40, WsEvent::Failed("a variable is missing".into())),
            entry(
                95,
                WsEvent::Closed {
                    by: WsSide::Server,
                    code: Some(1000),
                    reason: "bye".into(),
                },
            ),
            entry(
                96,
                WsEvent::Closed {
                    by: WsSide::Client,
                    code: None,
                    reason: String::new(),
                },
            ),
        ];
        let log: String = entries.iter().map(WsLogEntry::to_line).collect();
        assert_eq!(
            log.lines().nth(1),
            Some("{\"at\":12,\"type\":\"received\",\"binary\":\"00ff10\"}")
        );
        assert_eq!(
            log.lines().nth(4),
            Some(
                "{\"at\":95,\"type\":\"closed\",\"by\":\"server\",\"code\":1000,\"reason\":\"bye\"}"
            )
        );
        assert_eq!(parse_ws_log(log.as_bytes()), entries);
    }

    #[test]
    fn a_log_is_read_in_chunks_of_any_size_and_skips_what_is_not_an_entry() {
        let log = "{\"at\":1,\"type\":\"sent\",\"text\":\"ñ\"}\nnot json\n{\"at\":2,\"type\":\"what\"}\n{\"at\":3,\"type\":\"received\",\"binary\":\"0\"}\n{\"at\":4,\"type\":\"received\",\"text\":\"x\"}\n{\"at\":5";
        let whole = parse_ws_log(log.as_bytes());
        assert_eq!(whole.len(), 2);
        for size in 1..log.len() {
            let mut parser = WsLogParser::new();
            let pieces: Vec<WsLogEntry> = (log.as_bytes().chunks(size))
                .flat_map(|chunk| parser.push(chunk))
                .collect();
            assert_eq!(pieces, whole, "chunks of {size}");
        }
    }

    #[test]
    fn a_handshake_is_told_by_its_status_and_upgrade_header() {
        let headers = vec![("Upgrade".to_owned(), "WebSocket".to_owned())];
        assert!(is_websocket_handshake(101, &headers));
        assert!(!is_websocket_handshake(200, &headers));
        assert!(!is_websocket_handshake(101, &[]));
    }
}
