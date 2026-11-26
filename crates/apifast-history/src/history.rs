//! The history of sends as JSON Lines: one entry per line, only ever
//! appended to, except when [`FsHistoryStore::prune`] rewrites it.
//!
//! A line that cannot be read (a crash in the middle of a write, an entry
//! from a newer version) is skipped; the others are still read.
//!
//! Format 2 adds to each line its id, its name and what was sent. Lines of
//! format 1 are still read: a response's id is its body's, and a failed
//! send's is made from its line; the next rewrite stores them as format 2.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use apifast_app::{
    HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, RequestPath, ResponseHead,
    ResponseId, SentBody, SentPart, SentRequest, StoreError, TimelineEvent, TimelineKind,
    rename_in, within_limits,
};
use serde_json::{Map, Value, json};

/// Version of the line format, written in every line.
const FORMAT: u64 = 2;
/// The first format, still read.
const FORMAT_1: u64 = 1;

/// The history of one collection in a `.jsonl` file.
///
/// Clones share a lock: an entry appended while another clone rewrites the
/// file (to rename or remove an entry) waits, and is not lost.
#[derive(Debug, Clone)]
pub struct FsHistoryStore {
    file: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl FsHistoryStore {
    /// A history in `file`. It and its folder are created on the first
    /// write.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            lock: Arc::default(),
        }
    }

    /// Holds the file for one change; a poisoned lock still holds it.
    fn hold(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The history's file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The text of the file and its lines that are entries, with how many
    /// lines it has; none if there is no file.
    fn read(&self) -> Result<Option<(Vec<HistoryEntry>, usize)>, StoreError> {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(error("cannot read", &self.file, &e)),
        };
        let entries = (text.lines().enumerate())
            .filter_map(|(line, text)| decode(&serde_json::from_str(text).ok()?, line))
            .collect();
        Ok(Some((entries, text.lines().count())))
    }

    /// Writes `entries` as the whole history: aside, then renamed, so a
    /// crash leaves the old file whole.
    fn rewrite(&self, entries: &[HistoryEntry]) -> Result<(), StoreError> {
        let mut text = String::new();
        for entry in entries {
            text.push_str(&encode(entry).to_string());
            text.push('\n');
        }
        let partial = self.file.with_extension("jsonl.partial");
        fs::write(&partial, text)
            .and_then(|()| fs::rename(&partial, &self.file))
            .map_err(|e| error("cannot rewrite", &self.file, &e))
    }
}

impl HistoryStore for FsHistoryStore {
    fn append(&self, entry: &HistoryEntry) -> Result<(), StoreError> {
        let _held = self.hold();
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
        Ok(self.read()?.map(|(entries, _)| entries).unwrap_or_default())
    }

    fn prune(&self, limits: HistoryLimits, now_ms: i64) -> Result<usize, StoreError> {
        let _held = self.hold();
        let Some((entries, lines)) = self.read()? else {
            return Ok(0);
        };
        let kept = within_limits(entries, limits, now_ms);
        // Lines that could not be read go too.
        let removed = lines - kept.len();
        if removed > 0 {
            self.rewrite(&kept)?;
        }
        Ok(removed)
    }

    fn rename(&self, id: &str, name: Option<&str>) -> Result<bool, StoreError> {
        let _held = self.hold();
        let Some((mut entries, _)) = self.read()? else {
            return Ok(false);
        };
        let found = rename_in(&mut entries, id, name);
        if found {
            self.rewrite(&entries)?;
        }
        Ok(found)
    }

    fn remove(&self, id: &str) -> Result<Option<HistoryEntry>, StoreError> {
        let _held = self.hold();
        let Some((mut entries, _)) = self.read()? else {
            return Ok(None);
        };
        let Some(index) = entries.iter().position(|entry| entry.id == id) else {
            return Ok(None);
        };
        let removed = entries.remove(index);
        self.rewrite(&entries)?;
        Ok(Some(removed))
    }

    fn clear(&self) -> Result<Vec<HistoryEntry>, StoreError> {
        let _held = self.hold();
        let Some((entries, _)) = self.read()? else {
            return Ok(Vec::new());
        };
        match fs::remove_file(&self.file) {
            Ok(()) => Ok(entries),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(entries),
            Err(e) => Err(error("cannot remove", &self.file, &e)),
        }
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
        "id": entry.id,
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
        if let Some(name) = entry.name() {
            map.insert("name".into(), json!(name));
        }
        if let Some(sent) = &entry.sent {
            map.insert("sent".into(), encode_sent(sent));
        }
    }
    line
}

fn encode_sent(sent: &SentRequest) -> Value {
    let body = match &sent.body {
        SentBody::None => json!({"type": "none"}),
        SentBody::Text { text, size } => json!({"type": "text", "text": text, "size": size}),
        SentBody::Form(fields) => json!({"type": "form", "fields": fields}),
        SentBody::Multipart(parts) => json!({
            "type": "multipart",
            "parts": parts.iter().map(|part| json!({
                "name": part.name,
                "value": part.value,
                "files": part.files,
                "content_type": part.content_type,
            })).collect::<Vec<_>>(),
        }),
        SentBody::File(path) => json!({"type": "file", "path": path}),
    };
    json!({
        "method": sent.method,
        "url": sent.url,
        "headers": sent.headers,
        "body": body,
    })
}

/// Name and value pairs, as `[["name", "value"], …]`.
fn pairs(value: &Value) -> Option<Vec<(String, String)>> {
    value
        .as_array()?
        .iter()
        .map(|pair| {
            let pair = pair.as_array()?;
            Some((
                pair.first()?.as_str()?.to_owned(),
                pair.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn decode_sent(value: &Value) -> Option<SentRequest> {
    let map = value.as_object()?;
    let body = map.get("body")?.as_object()?;
    let body = match body.get("type")?.as_str()? {
        "none" => SentBody::None,
        "text" => SentBody::Text {
            text: text(body, "text")?,
            size: number(body, "size")?,
        },
        "form" => SentBody::Form(pairs(body.get("fields")?)?),
        "multipart" => SentBody::Multipart(
            (body.get("parts")?.as_array()?.iter())
                .map(|part| {
                    let part = part.as_object()?;
                    Some(SentPart {
                        name: text(part, "name")?,
                        value: text(part, "value")?,
                        files: part.get("files")?.as_bool()?,
                        content_type: text(part, "content_type"),
                    })
                })
                .collect::<Option<_>>()?,
        ),
        "file" => SentBody::File(text(body, "path")?),
        _ => return None,
    };
    Some(SentRequest {
        method: text(map, "method")?,
        url: text(map, "url")?,
        headers: pairs(map.get("headers")?)?,
        body,
    })
}

fn encode_event(event: &TimelineEvent) -> Value {
    let mut value = match &event.kind {
        TimelineKind::Request { method, url } => {
            json!({"type": "request", "method": method, "url": url})
        }
        TimelineKind::Redirect { status, location } => {
            json!({"type": "redirect", "status": status, "location": location})
        }
        TimelineKind::Dns {
            host,
            addresses,
            duration_us,
            overridden,
        } => {
            let mut dns = json!({"type": "dns", "host": host, "addresses": addresses, "duration_us": duration_us});
            if *overridden {
                dns["overridden"] = json!(true);
            }
            dns
        }
        TimelineKind::Connected {
            address,
            duration_us,
        } => json!({"type": "connected", "address": address, "duration_us": duration_us}),
        TimelineKind::Reused { address } => json!({"type": "reused", "address": address}),
        TimelineKind::Tls {
            version,
            duration_us,
        } => json!({"type": "tls", "version": version, "duration_us": duration_us}),
        TimelineKind::Headers { status } => json!({"type": "headers", "status": status}),
        TimelineKind::Body { bytes } => json!({"type": "body", "bytes": bytes}),
        TimelineKind::Decompressed {
            encoding,
            wire_bytes,
        } => json!({"type": "decompressed", "encoding": encoding, "wire_bytes": wire_bytes}),
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

/// The entry on line number `line` of the file, if it is one.
fn decode(value: &Value, line: usize) -> Option<HistoryEntry> {
    let map = value.as_object()?;
    let format = number(map, "v")?;
    if format != FORMAT && format != FORMAT_1 {
        return None;
    }
    let outcome = match (map.get("response"), map.get("error")) {
        (Some(Value::Object(response)), _) => HistoryOutcome::Received {
            head: ResponseHead {
                status: status(response)?,
                version: text(response, "version")?,
                url: text(response, "url")?,
                redirects: u32::try_from(number(response, "redirects")?).ok()?,
                headers: pairs(response.get("headers")?)?,
                elapsed_ms: number(response, "elapsed_ms")?,
                timeline: response
                    .get("timeline")?
                    .as_array()?
                    .iter()
                    // An event of a newer version is skipped, not the entry.
                    .filter_map(decode_event)
                    .collect(),
            },
            size: number(response, "size")?,
            body: ResponseId(text(response, "body")?),
        },
        (_, Some(Value::String(message))) => HistoryOutcome::Failed(message.clone()),
        _ => return None,
    };
    let at_ms = map.get("at")?.as_i64()?;
    // Format 1 had no ids: a response is named by its body, which is
    // unique, and a failure by its time and line.
    let id = text(map, "id").unwrap_or_else(|| match &outcome {
        HistoryOutcome::Received { body, .. } => body.0.clone(),
        HistoryOutcome::Failed(_) => format!("failed-{at_ms}-line{line}"),
    });
    Some(HistoryEntry {
        id,
        name: text(map, "name").filter(|name| !name.is_empty()),
        at_ms,
        request: RequestPath::new(text(map, "request")?),
        method: text(map, "method")?,
        url: text(map, "url")?,
        outcome,
        // Something unreadable in what was sent does not lose the entry.
        sent: map.get("sent").and_then(decode_sent),
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
        "dns" => TimelineKind::Dns {
            host: text(map, "host")?,
            addresses: (map.get("addresses")?.as_array()?.iter())
                .filter_map(|a| a.as_str().map(str::to_owned))
                .collect(),
            duration_us: number(map, "duration_us")?,
            overridden: map.get("overridden").and_then(Value::as_bool) == Some(true),
        },
        "connected" => TimelineKind::Connected {
            address: text(map, "address"),
            duration_us: number(map, "duration_us")?,
        },
        "reused" => TimelineKind::Reused {
            address: text(map, "address"),
        },
        "tls" => TimelineKind::Tls {
            version: text(map, "version"),
            duration_us: number(map, "duration_us")?,
        },
        "headers" => TimelineKind::Headers {
            status: status(map)?,
        },
        "body" => TimelineKind::Body {
            bytes: number(map, "bytes")?,
        },
        "decompressed" => TimelineKind::Decompressed {
            encoding: text(map, "encoding")?,
            wire_bytes: number(map, "wire_bytes")?,
        },
        _ => return None,
    };
    Some(TimelineEvent {
        at_us: number(map, "at_us")?,
        kind,
    })
}
