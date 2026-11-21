//! The git repository a collection lives in (docs/09, F10): which branch
//! it is on and what changed since the last commit, read through a
//! [`GitRepository`]. Paths are relative to the collection's root, as the
//! rest of the application names files; files of the repository outside
//! the collection are not seen.
//!
//! [`git_diff`] says what changed in a file field by field: a request, a
//! folder, the collection or an environment, read as apifast reads them
//! both from the last commit and from disk.

use apifast_domain::{
    FieldChange, collection_changes, environment_changes, folder_changes, request_changes,
};

use crate::{AppError, CollectionFile, CollectionRepository};

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

/// What changed in a file since the last commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// The file, relative to the collection.
    pub path: String,
    /// How it changed.
    pub change: FileChange,
    /// Its fields that changed, in the file's order. Empty for a file added
    /// or deleted: all of it is.
    pub fields: Vec<FieldChange>,
}

/// What changed in `file` since the last commit of `git`: the file as it
/// was there and as `collections` reads it now, compared field by field. A
/// file apifast does not read is compared as text (`content`).
///
/// # Errors
///
/// If the repository or the file cannot be read, or one of its versions is
/// not valid.
pub fn git_diff(
    collections: &impl CollectionRepository,
    git: &dyn GitRepository,
    file: &ChangedFile,
) -> Result<FileDiff, AppError> {
    let fields = match file.change {
        FileChange::Added | FileChange::Deleted => Vec::new(),
        FileChange::Modified => {
            let before = match git.head_file(&file.path)? {
                Some(bytes) => Some(collections.parse_file(&file.path, &bytes)?),
                None => None,
            };
            let after = collections.file_now(&file.path)?;
            match (before, after) {
                (Some(before), Some(after)) => field_changes(&before, &after),
                _ => Vec::new(),
            }
        }
    };
    Ok(FileDiff {
        path: file.path.clone(),
        change: file.change,
        fields,
    })
}

fn field_changes(before: &CollectionFile, after: &CollectionFile) -> Vec<FieldChange> {
    use CollectionFile as F;
    match (before, after) {
        (F::Request(a), F::Request(b)) => request_changes(a, b),
        (F::Folder(a), F::Folder(b)) => folder_changes(a, b),
        (F::Collection(a), F::Collection(b)) => collection_changes(a, b),
        (F::Environment(a), F::Environment(b)) => environment_changes(a, b),
        (F::Other(a), F::Other(b)) if a != b => vec![FieldChange {
            field: "content".into(),
            before: Some(a.clone()),
            after: Some(b.clone()),
        }],
        (F::Other(_), F::Other(_)) => Vec::new(),
        // What the file is changed: say that.
        (a, b) => vec![FieldChange {
            field: "kind".into(),
            before: Some(kind(a).into()),
            after: Some(kind(b).into()),
        }],
    }
}

fn kind(file: &CollectionFile) -> &'static str {
    match file {
        CollectionFile::Request(_) => "request",
        CollectionFile::Folder(_) => "folder",
        CollectionFile::Collection(_) => "collection",
        CollectionFile::Environment(_) => "environment",
        CollectionFile::Other(_) => "file",
    }
}
