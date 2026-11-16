//! Watches a collection's folder for changes made outside apifast: a text
//! editor, `git checkout`, a script.
//!
//! Events come in bursts (a checkout touches hundreds of files; an editor
//! writes, renames and changes permissions for one save), so they are
//! gathered until the folder has been quiet for [`QUIET`], or for at most
//! [`MAX_WAIT`], and handed over as one [`Changes`] of relative paths. The
//! application then reads those files again and reconciles them with what it
//! holds (ADR 0003).

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long the folder must be quiet before the changes are handed over.
pub const QUIET: Duration = Duration::from_millis(200);
/// The longest changes wait during a burst that does not stop.
pub const MAX_WAIT: Duration = Duration::from_secs(2);

/// Folders whose contents never belong to the collection.
const IGNORED_FOLDERS: &[&str] = &[".git", ".apifast", "node_modules"];

/// Changes seen in the folder since the last ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Paths that were created, changed or removed, relative to the folder
    /// and with `/` between segments.
    pub paths: BTreeSet<String>,
    /// Whether events may have been lost (the system's queue overflowed),
    /// so everything should be read again.
    pub rescan: bool,
}

/// Why the folder cannot be watched.
#[derive(Debug, thiserror::Error)]
#[error("cannot watch `{path}`: {message}")]
pub struct WatchError {
    /// The folder.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

/// Watches a folder while it lives. Dropping it stops watching.
pub struct FsWatcher {
    // Dropped first: that closes the channel, which ends the thread.
    watcher: Option<RecommendedWatcher>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for FsWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsWatcher").finish_non_exhaustive()
    }
}

impl FsWatcher {
    /// Watches `root` and everything under it. `on_change` is called from a
    /// thread of its own with each batch of changes.
    ///
    /// # Errors
    ///
    /// If the folder does not exist or the system refuses to watch it.
    pub fn start(
        root: &Path,
        on_change: impl Fn(Changes) + Send + 'static,
    ) -> Result<Self, WatchError> {
        Self::watch(root, RecursiveMode::Recursive, on_change)
    }

    /// Watches only the files directly in `root`, not its subfolders: for a
    /// workspace's `workspace.yml`, whose collections are watched on their
    /// own.
    ///
    /// # Errors
    ///
    /// If the folder does not exist or the system refuses to watch it.
    pub fn start_shallow(
        root: &Path,
        on_change: impl Fn(Changes) + Send + 'static,
    ) -> Result<Self, WatchError> {
        Self::watch(root, RecursiveMode::NonRecursive, on_change)
    }

    fn watch(
        root: &Path,
        mode: RecursiveMode,
        on_change: impl Fn(Changes) + Send + 'static,
    ) -> Result<Self, WatchError> {
        let error = |e: &dyn std::fmt::Display| WatchError {
            path: root.to_owned(),
            message: e.to_string(),
        };
        // Events carry absolute paths, which may be resolved (macOS reports
        // `/private/var` for `/var`): try both forms of the root.
        let roots = vec![root.to_owned(), root.canonicalize().map_err(|e| error(&e))?];
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender).map_err(|e| error(&e))?;
        watcher.watch(root, mode).map_err(|e| error(&e))?;
        let thread = std::thread::Builder::new()
            .name("apifast-watch".into())
            .spawn(move || gather(&receiver, &roots, &on_change))
            .map_err(|e| error(&e))?;
        Ok(Self {
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type Received = Result<notify::Event, notify::Error>;

/// Gathers events into batches until the channel closes.
fn gather(receiver: &mpsc::Receiver<Received>, roots: &[PathBuf], on_change: &dyn Fn(Changes)) {
    // The first event of a batch: wait as long as it takes.
    while let Ok(first) = receiver.recv() {
        let mut changes = Changes::default();
        add(&mut changes, first, roots);
        let started = Instant::now();
        loop {
            let left = MAX_WAIT.saturating_sub(started.elapsed());
            match receiver.recv_timeout(QUIET.min(left)) {
                Ok(event) => add(&mut changes, event, roots),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if started.elapsed() >= MAX_WAIT {
                break;
            }
        }
        if changes.rescan || !changes.paths.is_empty() {
            on_change(changes);
        }
    }
}

fn add(changes: &mut Changes, received: Received, roots: &[PathBuf]) {
    // An error from the system: what changed is unknown.
    let Ok(event) = received else {
        changes.rescan = true;
        return;
    };
    if event.need_rescan() {
        changes.rescan = true;
    }
    // Reading a file is not a change.
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    for path in &event.paths {
        if let Some(relative) = roots.iter().find_map(|root| relative(root, path)) {
            changes.paths.insert(relative);
        }
    }
}

/// `path` relative to `root`, with `/` between segments, unless it is
/// something the collection never holds: ignored folders, or temporary
/// files of editors and of apifast's own atomic writes.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let mut segments = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_str()?),
            _ => return None,
        }
    }
    let name = *segments.last()?;
    let ignored_folder = segments.iter().any(|s| IGNORED_FOLDERS.contains(s));
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let temporary = name.contains(".apifast-tmp-")
        || name.ends_with('~')
        || matches!(extension.as_deref(), Some("swp" | "swx" | "tmp"))
        || name.starts_with(".#")
        || name == "4913"; // Vim checks it can write a folder with this file.
    (!ignored_folder && !temporary).then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_relative_and_temporary_files_are_left_out() {
        let root = Path::new("/c");
        let rel = |p: &str| relative(root, Path::new(p));
        assert_eq!(rel("/c/users/get.yml").as_deref(), Some("users/get.yml"));
        assert_eq!(rel("/c/users").as_deref(), Some("users"));
        for ignored in [
            "/c/.git/index",
            "/c/.apifast/collection.yml",
            "/c/node_modules/x/a.yml",
            "/c/users/.get.yml.apifast-tmp-42",
            "/c/get.yml~",
            "/c/.get.yml.swp",
            "/c/.#get.yml",
            "/c/4913",
            "/other/get.yml",
            "/c",
        ] {
            assert_eq!(rel(ignored), None, "{ignored}");
        }
    }
}
