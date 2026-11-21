//! The git repository a collection lives in (docs/09, F10): which branch
//! it is on and what changed since the last commit, read through a
//! [`GitRepository`]. Paths are relative to the collection's root, as the
//! rest of the application names files; files of the repository outside
//! the collection are not seen.

/// How a file differs from the last commit (`HEAD`), whatever is staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileChange {
    /// Not in the last commit: new, or never committed.
    Added,
    /// In the last commit, with other content now.
    Modified,
    /// In the last commit, gone now.
    Deleted,
}

/// A file of the collection that differs from the last commit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangedFile {
    /// Its path, relative to the collection's root, with `/`.
    pub path: String,
    /// How.
    pub change: FileChange,
}

/// Where a collection's repository stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// The branch checked out; `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// The files that differ from the last commit, by path.
    pub changes: Vec<ChangedFile>,
}

/// Why git could not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("git: {0}")]
pub struct GitError(pub String);

/// The repository of a collection.
pub trait GitRepository: Send + Sync {
    /// The branch, and the files of the collection that changed.
    ///
    /// # Errors
    ///
    /// If the repository cannot be read.
    fn status(&self) -> Result<GitStatus, GitError>;

    /// The content of `path` (relative to the collection) in the last
    /// commit; `None` when it is not there, or there is no commit yet.
    ///
    /// # Errors
    ///
    /// If the repository cannot be read.
    fn head_file(&self, path: &str) -> Result<Option<Vec<u8>>, GitError>;
}
