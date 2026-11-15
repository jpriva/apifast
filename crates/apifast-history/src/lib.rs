//! Response history and bodies on disk.
//!
//! [`FsHistoryStore`] keeps the history of sends of a collection as JSON
//! Lines, in `history/<collection-id>.jsonl`.
//!
//! [`FsResponseStore`] keeps one file per response body in a folder of the
//! user's data directory (outside the collection, see ADR 0008), such as
//! `responses/<collection-id>/`. Bodies are written in chunks through a fixed
//! buffer and read back in windows, so memory never grows with their size.

pub mod dirs;
mod history;

pub use history::FsHistoryStore;

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use apifast_app::{BodyWriter, ResponseId, ResponseStore, StoreError, StoredBody};

/// Buffer between the network and the disk.
const WRITE_BUFFER: usize = 64 * 1024;
const BODY_EXTENSION: &str = "body";
const PARTIAL_EXTENSION: &str = "partial";

/// Numbers responses created by this process, so ids never repeat.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Response bodies stored as files in one folder.
#[derive(Debug, Clone)]
pub struct FsResponseStore {
    dir: PathBuf,
}

/// How many stored responses [`FsResponseStore::prune`] keeps. The newest are
/// kept; any limit left as `None` does not apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Maximum number of responses.
    pub max_count: Option<usize>,
    /// Maximum total size in bytes.
    pub max_total_bytes: Option<u64>,
    /// Responses older than this are removed.
    pub max_age: Option<Duration>,
}

/// What [`FsResponseStore::prune`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Stored responses removed.
    pub responses: usize,
    /// Leftover partial files removed.
    pub partials: usize,
    /// Bytes freed.
    pub bytes: u64,
}

impl FsResponseStore {
    /// A store in `dir`. The folder is created on the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The store's folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn body_path(&self, id: &ResponseId) -> Result<PathBuf, StoreError> {
        if !is_valid_id(&id.0) {
            return Err(StoreError(format!("`{}` is not a response id", id.0)));
        }
        Ok(self.dir.join(format!("{}.{BODY_EXTENSION}", id.0)))
    }

    /// Size of a stored body, in bytes.
    ///
    /// # Errors
    ///
    /// If the id is invalid or there is no such response.
    pub fn size(&self, id: &ResponseId) -> Result<u64, StoreError> {
        let path = self.body_path(id)?;
        Ok(fs::metadata(&path).map_err(|e| missing(id, &e))?.len())
    }

    /// Removes a stored response. Removing one that does not exist is fine.
    ///
    /// # Errors
    ///
    /// If the id is invalid or the file cannot be removed.
    pub fn remove(&self, id: &ResponseId) -> Result<(), StoreError> {
        match fs::remove_file(self.body_path(id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError(format!(
                "cannot remove response `{}`: {e}",
                id.0
            ))),
        }
    }

    /// Removes the oldest responses until `limits` hold, and partial files
    /// left by sends that crashed. Partial files of sends still running are
    /// kept: only those untouched for an hour are removed.
    ///
    /// # Errors
    ///
    /// If the folder cannot be listed. Files that cannot be removed are skipped.
    pub fn prune(&self, limits: Limits, now: SystemTime) -> Result<Pruned, StoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Pruned::default()),
            Err(e) => {
                return Err(StoreError(format!(
                    "cannot list `{}`: {e}",
                    self.dir.display()
                )));
            }
        };

        let mut pruned = Pruned::default();
        // (modified, size, path) of every stored body.
        let mut bodies = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
            match path.extension().and_then(|e| e.to_str()) {
                Some(BODY_EXTENSION) => bodies.push((modified, metadata.len(), path)),
                Some(PARTIAL_EXTENSION) => {
                    let stale = now
                        .duration_since(modified)
                        .is_ok_and(|age| age > Duration::from_secs(3600));
                    if stale && fs::remove_file(&path).is_ok() {
                        pruned.partials += 1;
                        pruned.bytes += metadata.len();
                    }
                }
                _ => {}
            }
        }

        // Newest first; ids sort by creation time, which breaks mtime ties.
        bodies.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)));
        let mut kept_count = 0;
        let mut kept_bytes = 0u64;
        for (modified, size, path) in bodies {
            let too_old = limits
                .max_age
                .is_some_and(|max| now.duration_since(modified).is_ok_and(|age| age > max));
            let too_many = limits.max_count.is_some_and(|max| kept_count >= max);
            let too_big = limits
                .max_total_bytes
                .is_some_and(|max| kept_bytes + size > max);
            if (too_old || too_many || too_big) && fs::remove_file(&path).is_ok() {
                pruned.responses += 1;
                pruned.bytes += size;
            } else {
                kept_count += 1;
                kept_bytes += size;
            }
        }
        Ok(pruned)
    }
}

impl ResponseStore for FsResponseStore {
    type Writer = FsBodyWriter;

    fn create(&self) -> Result<FsBodyWriter, StoreError> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| StoreError(format!("cannot create `{}`: {e}", self.dir.display())))?;
        let id = new_id();
        let partial = self.dir.join(format!("{id}.{PARTIAL_EXTENSION}"));
        let file = File::create_new(&partial)
            .map_err(|e| StoreError(format!("cannot create `{}`: {e}", partial.display())))?;
        Ok(FsBodyWriter {
            file: Some(BufWriter::with_capacity(WRITE_BUFFER, file)),
            partial,
            done: self.dir.join(format!("{id}.{BODY_EXTENSION}")),
            id: ResponseId(id),
            size: 0,
        })
    }

    fn read(&self, id: &ResponseId, offset: u64, len: usize) -> Result<Vec<u8>, StoreError> {
        let path = self.body_path(id)?;
        let mut file = File::open(&path).map_err(|e| missing(id, &e))?;
        let size = file.metadata().map_err(|e| missing(id, &e))?.len();
        if offset >= size || len == 0 {
            return Ok(Vec::new());
        }
        let available = usize::try_from(size - offset).unwrap_or(usize::MAX);
        let mut window = vec![0; len.min(available)];
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_exact(&mut window))
            .map_err(|e| StoreError(format!("cannot read response `{}`: {e}", id.0)))?;
        Ok(window)
    }
}

/// Writes one response body. Until [`BodyWriter::finish`] it is a partial
/// file that readers never see; dropping the writer without finishing (a
/// failed or cancelled send) removes it.
#[derive(Debug)]
pub struct FsBodyWriter {
    file: Option<BufWriter<File>>,
    partial: PathBuf,
    done: PathBuf,
    id: ResponseId,
    size: u64,
}

impl BodyWriter for FsBodyWriter {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        let Some(file) = self.file.as_mut() else {
            return Err(StoreError("the response body was already finished".into()));
        };
        file.write_all(chunk)
            .map_err(|e| StoreError(format!("cannot write response `{}`: {e}", self.id.0)))?;
        self.size += chunk.len() as u64;
        Ok(())
    }

    fn finish(mut self) -> Result<StoredBody, StoreError> {
        let file = self
            .file
            .take()
            .ok_or_else(|| StoreError("the response body was already finished".into()))?;
        let fail = |e: io::Error| StoreError(format!("cannot save response `{}`: {e}", self.id.0));
        file.into_inner().map_err(|e| fail(e.into_error()))?;
        fs::rename(&self.partial, &self.done).map_err(fail)?;
        Ok(StoredBody {
            id: self.id.clone(),
            size: self.size,
        })
    }
}

impl Drop for FsBodyWriter {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// `<unix millis>-<process id>-<sequence>`: sorts by creation time and is
/// unique across processes writing to the same folder.
fn new_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{millis:013}-{}-{sequence:06}", std::process::id())
}

/// Ids are made of digits and dashes only, so they can never name a path
/// outside the store.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

fn missing(id: &ResponseId, error: &io::Error) -> StoreError {
    StoreError(format!("no response `{}`: {error}", id.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_sorted_by_creation() {
        let ids: Vec<String> = (0..1000).map(|_| new_id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, ids);
        assert!(ids.iter().all(|id| is_valid_id(id)));
    }

    #[test]
    fn ids_cannot_name_other_paths() {
        for bad in [
            "",
            "../x",
            "a/b",
            "x.body",
            "..",
            "/etc/passwd",
            "1\0",
            &"1".repeat(65),
        ] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
        assert!(is_valid_id("1700000000000-42-000001"));
    }
}
