//! The history of sends as JSON Lines: one entry per line, only ever
//! appended to, except when [`FsHistoryStore::prune`] rewrites it.
//!
//! A line that cannot be read (a crash in the middle of a write, an entry
//! from a newer version) is skipped; the others are still read.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use apifast_app::{
    HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, RequestPath, ResponseHead,
    ResponseId, StoreError, TimelineEvent, TimelineKind, within_limits,
};
use serde_json::{Map, Value, json};

/// Version of the line format, written in every line.
const FORMAT: u64 = 1;

/// The history of one collection in a `.jsonl` file.
#[derive(Debug, Clone)]
pub struct FsHistoryStore {
    file: PathBuf,
}

impl FsHistoryStore {
    /// A history in `file`. It and its folder are created on the first
    /// write.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The history's file.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl HistoryStore for FsHistoryStore {
    fn append(&self, entry: &HistoryEntry) -> Result<(), StoreError> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir).map_err(|e| error("cannot create", dir, &e))?;
        }
        // After a line cut by a crash, start a new one: otherwise this entry
        // would be glued to it and lost with it.
        let mut line = if ends_open(&self.file) {
            "\n".to_owned()
        } else {
            String::new()
        };
        line.push_str(&encode(entry).to_string());
        line.push('\n');
        // One write of a whole line, in append mode: a reader never sees
        // half of it, unless the process dies in the middle.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|e| error("cannot write", &self.file, &e))
    }

    fn entries(&self) -> Result<Vec<HistoryEntry>, StoreError> {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(error("cannot read", &self.file, &e)),
        };
        Ok(text
            .lines()
            .filter_map(|line| decode(&serde_json::from_str(line).ok()?))
            .collect())
    }

    fn prune(&self, limits: HistoryLimits, now_ms: i64) -> Result<usize, StoreError> {
        let entries = self.entries()?;
        let lines = match fs::read_to_string(&self.file) {
            Ok(text) => text.lines().count(),
            Err(_) => return Ok(0),
        };
        let kept = within_limits(entries, limits, now_ms);
        // Lines that could not be read go too.
        let removed = lines - kept.len();
        if removed == 0 {
            return Ok(0);
        }
        let mut text = String::new();
        for entry in &kept {
            text.push_str(&encode(entry).to_string());
            text.push('\n');
        }
        // Written aside and renamed, so a crash leaves the old file whole.
        let partial = self.file.with_extension("jsonl.partial");
        fs::write(&partial, text)
            .and_then(|()| fs::rename(&partial, &self.file))
            .map_err(|e| error("cannot rewrite", &self.file, &e))?;
        Ok(removed)
    }
}

/// Whether `file` exists and its last line has no newline.
fn ends_open(file: &Path) -> bool {
    let Ok(mut file) = File::open(file) else {
        return false;
    };
    let mut last = [0];
    file.seek(SeekFrom::End(-1))
        .and_then(|_| file.read_exact(&mut last))
        .is_ok_and(|()| last[0] != b'\n')
}

fn error(what: &str, path: &Path, e: &io::Error) -> StoreError {
    StoreError(format!("{what} `{}`: {e}", path.display()))
}

fn encode(entry: &HistoryEntry) -> Value {
    let mut line = json!({
        "v": FORMAT,
        "at": entry.at_ms,
        "request": entry.request.as_str(),
        "method": entry.method,
        "url": entry.url,
    });
    let outcome = match &entry.outcome {
        HistoryOutcome::Received { head, size, body } => (
            "response",
            json!({
                "status": head.status,
                "version": head.version,
                "url": head.url,
                "redirects": head.redirects,
                "headers": head.headers,
                "elapsed_ms": head.elapsed_ms,
                "timeline": head.timeline.iter().map(encode_event).collect::<Vec<_>>(),
                "size": size,
                "body": body.0,
            }),
        ),
        HistoryOutcome::Failed(message) => ("error", json!(message)),
    };
    if let Value::Object(map) = &mut line {
        map.insert(outcome.0.into(), outcome.1);
    }
    line
}

fn encode_event(event: &TimelineEvent) -> Value {
    let mut value = match &event.kind {
        TimelineKind::Request { method, url } => {
            json!({"type": "request", "method": method, "url": url})
        }
        TimelineKind::Redirect { status, location } => {
            json!({"type": "redirect", "status": status, "location": location})
        }
        TimelineKind::Headers { status } => json!({"type": "headers", "status": status}),
        TimelineKind::Body { bytes } => json!({"type": "body", "bytes": bytes}),
    };
    if let Value::Object(map) = &mut value {
        map.insert("at_us".into(), json!(event.at_us));
    }
    value
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)?.as_str().map(str::to_owned)
}

fn number(map: &Map<String, Value>, key: &str) -> Option<u64> {
    map.get(key)?.as_u64()
}

fn status(map: &Map<String, Value>) -> Option<u16> {
    u16::try_from(number(map, "status")?).ok()
}

fn decode(line: &Value) -> Option<HistoryEntry> {
    let map = line.as_object()?;
    if number(map, "v")? != FORMAT {
        return None;
    }
    let outcome = match (map.get("response"), map.get("error")) {
        (Some(Value::Object(response)), _) => HistoryOutcome::Received {
            head: ResponseHead {
                status: status(response)?,
                version: text(response, "version")?,
                url: text(response, "url")?,
                redirects: u32::try_from(number(response, "redirects")?).ok()?,
                headers: response
                    .get("headers")?
                    .as_array()?
                    .iter()
                    .map(|pair| {
                        let pair = pair.as_array()?;
                        Some((
                            pair.first()?.as_str()?.to_owned(),
                            pair.get(1)?.as_str()?.to_owned(),
                        ))
                    })
                    .collect::<Option<_>>()?,
                elapsed_ms: number(response, "elapsed_ms")?,
                timeline: response
                    .get("timeline")?
                    .as_array()?
                    .iter()
                    .map(decode_event)
                    .collect::<Option<_>>()?,
            },
            size: number(response, "size")?,
            body: ResponseId(text(response, "body")?),
        },
        (_, Some(Value::String(message))) => HistoryOutcome::Failed(message.clone()),
        _ => return None,
    };
    Some(HistoryEntry {
        at_ms: map.get("at")?.as_i64()?,
        request: RequestPath::new(text(map, "request")?),
        method: text(map, "method")?,
        url: text(map, "url")?,
        outcome,
    })
}

fn decode_event(event: &Value) -> Option<TimelineEvent> {
    let map = event.as_object()?;
    let kind = match map.get("type")?.as_str()? {
        "request" => TimelineKind::Request {
            method: text(map, "method")?,
            url: text(map, "url")?,
        },
        "redirect" => TimelineKind::Redirect {
            status: status(map)?,
            location: text(map, "location")?,
        },
        "headers" => TimelineKind::Headers {
            status: status(map)?,
        },
        "body" => TimelineKind::Body {
            bytes: number(map, "bytes")?,
        },
        _ => return None,
    };
    Some(TimelineEvent {
        at_us: number(map, "at_us")?,
        kind,
    })
}
