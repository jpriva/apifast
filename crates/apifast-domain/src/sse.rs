//! Server-Sent Events (docs/07, F7): a `text/event-stream` body read into
//! its events as it arrives, as the WHATWG HTML standard reads it.
//!
//! Lines end with CRLF, LF or CR, which may fall in different chunks; a line
//! starting with `:` is a comment; `field: value` loses one space after the
//! colon; a blank line ends the event. An event with no `data` is not
//! dispatched, and neither is the unfinished one at the end of the stream.

/// The name of an event that names none.
pub const DEFAULT_EVENT: &str = "message";

/// One event of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Its `event` field, or [`DEFAULT_EVENT`].
    pub event: String,
    /// Its `data` lines, joined by line feeds.
    pub data: String,
    /// The last event id when it was dispatched: set by an `id` field of
    /// this event or an earlier one.
    pub id: Option<String>,
    /// The reconnection time it asked for, in milliseconds.
    pub retry: Option<u64>,
}

/// Whether a `Content-Type` value is that of an event stream.
pub fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// Reads a stream given in chunks of any size.
#[derive(Debug, Default)]
pub struct SseParser {
    /// The unfinished line.
    line: Vec<u8>,
    /// The last chunk ended in CR: a LF starting the next one ends nothing.
    after_cr: bool,
    /// Whether the byte order mark at the start was looked for.
    started: bool,
    event: String,
    data: String,
    /// Whether a `data` field was seen in this event, even an empty one.
    has_data: bool,
    last_id: Option<String>,
    retry: Option<u64>,
}

impl SseParser {
    /// A parser at the start of a stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads `chunk`; returns the events it finished, in order.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut chunk = chunk;
        if !self.started {
            // The byte order mark may itself be split; wait for three bytes.
            let mut head = std::mem::take(&mut self.line);
            head.extend_from_slice(chunk);
            if head.len() < 3 && b"\xEF\xBB\xBF".starts_with(&head) {
                self.line = head;
                return Vec::new();
            }
            self.started = true;
            let rest = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&head).to_vec();
            return self.push_lines(&rest);
        }
        if self.after_cr {
            self.after_cr = false;
            if let Some(rest) = chunk.strip_prefix(b"\n") {
                chunk = rest;
            }
        }
        self.push_lines(chunk)
    }

    fn push_lines(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let mut rest = chunk;
        while let Some(end) = rest.iter().position(|b| matches!(b, b'\r' | b'\n')) {
            self.line.extend_from_slice(&rest[..end]);
            let line = std::mem::take(&mut self.line);
            events.extend(self.line_ended(&line));
            let crlf = rest[end] == b'\r';
            rest = &rest[end + 1..];
            if crlf {
                match rest.first() {
                    Some(b'\n') => rest = &rest[1..],
                    None => self.after_cr = true,
                    Some(_) => {}
                }
            }
        }
        self.line.extend_from_slice(rest);
        events
    }

    /// Takes one whole line; returns the event a blank line dispatches.
    fn line_ended(&mut self, line: &[u8]) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(b":") {
            return None;
        }
        let line = String::from_utf8_lossy(line);
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_ref(), ""),
        };
        match field {
            "event" => value.clone_into(&mut self.event),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            // An id with a NUL is ignored, as the standard says.
            "id" if !value.contains('\0') => self.last_id = Some(value.to_owned()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                self.retry = value.parse().ok();
            }
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = std::mem::take(&mut self.event);
        let data = std::mem::take(&mut self.data);
        let retry = self.retry.take();
        if !std::mem::take(&mut self.has_data) {
            return None;
        }
        Some(SseEvent {
            event: if event.is_empty() {
                DEFAULT_EVENT.to_owned()
            } else {
                event
            },
            data,
            id: self.last_id.clone().filter(|id| !id.is_empty()),
            retry,
        })
    }
}

/// The events of a whole stream.
pub fn parse_events(stream: &[u8]) -> Vec<SseEvent> {
    SseParser::new().push(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, data: &str, id: Option<&str>) -> SseEvent {
        SseEvent {
            event: name.to_owned(),
            data: data.to_owned(),
            id: id.map(str::to_owned),
            retry: None,
        }
    }

    #[test]
    fn events_have_their_fields_and_comments_are_skipped() {
        let stream = b": hello\n\nevent: tick\nid: 1\ndata: {\"n\": 1}\n\n\
            data: one\ndata:two\ndata\n\nretry: 3000\ndata: x\n\nevent: lonely\n\ndata: cut";
        let events = parse_events(stream);
        assert_eq!(
            events,
            vec![
                event("tick", "{\"n\": 1}", Some("1")),
                event("message", "one\ntwo\n", Some("1")),
                SseEvent {
                    retry: Some(3000),
                    ..event("message", "x", Some("1"))
                },
            ]
        );
    }

    #[test]
    fn any_line_ending_and_any_chunking_give_the_same_events() {
        let stream = "\u{FEFF}id: a\r\ndata: é1\r\n\r\nevent: e\rdata: 2\r\rdata: 3\n\n";
        let whole = parse_events(stream.as_bytes());
        assert_eq!(
            whole,
            vec![
                event("message", "é1", Some("a")),
                event("e", "2", Some("a")),
                event("message", "3", Some("a")),
            ]
        );
        for size in 1..stream.len() {
            let mut parser = SseParser::new();
            let events: Vec<SseEvent> = stream
                .as_bytes()
                .chunks(size)
                .flat_map(|chunk| parser.push(chunk))
                .collect();
            assert_eq!(events, whole, "chunks of {size}");
        }
    }

    #[test]
    fn an_empty_id_clears_it() {
        let events = parse_events(b"id: 7\ndata: a\n\nid\ndata: b\n\n");
        assert_eq!(events[0].id.as_deref(), Some("7"));
        assert_eq!(events[1].id, None);
    }

    #[test]
    fn the_content_type_tells_a_stream() {
        assert!(is_event_stream("text/event-stream"));
        assert!(is_event_stream("Text/Event-Stream; charset=utf-8"));
        assert!(!is_event_stream("text/plain"));
        assert!(!is_event_stream(""));
    }
}
