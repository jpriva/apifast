//! The git repository a collection lives in, read with gitoxide (`gix`,
//! ADR 0014): pure Rust, no `git` binary needed.
//!
//! A collection may be the repository, or a folder inside a larger one: only
//! the files under the collection's root are seen, and paths are relative to
//! it.

use std::path::{Path, PathBuf};

use apifast_app::{ChangedFile, FileChange, GitError, GitRepository, GitStatus};
use gix::bstr::{BString, ByteSlice};
use gix::status::UntrackedFiles;

/// The repository of a collection, opened with gix.
#[derive(Debug, Clone)]
pub struct GixRepository {
    repo: gix::ThreadSafeRepository,
    /// The collection's root, relative to the repository's working tree,
    /// with `/` and no trailing one; empty when they are the same.
    prefix: String,
    /// The collection's root on disk.
    root: PathBuf,
}

fn failed(error: impl std::fmt::Display) -> GitError {
    GitError(error.to_string())
}

impl GixRepository {
    /// The repository the collection at `root` is in; `None` when it is in
    /// none, or in one without a working tree.
    ///
    /// # Errors
    ///
    /// If the paths cannot be resolved.
    pub fn discover(root: &Path) -> Result<Option<Self>, GitError> {
        // gix says only that it found none it could open: that is none.
        let Ok(repo) = gix::discover(root) else {
            return Ok(None);
        };
        let Some(workdir) = repo.workdir() else {
            return Ok(None);
        };
        let canonical = |path: &Path| path.canonicalize().map_err(failed);
        let (workdir, root) = (canonical(workdir)?, canonical(root)?);
        let Ok(relative) = root.strip_prefix(&workdir) else {
            return Ok(None);
        };
        let prefix = (relative.components())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        Ok(Some(Self {
            repo: repo.into_sync(),
            prefix,
            root,
        }))
    }

    /// `path` of the collection, as the repository names it.
    fn in_repo(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if self.prefix.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", self.prefix)
        }
    }

    /// `path` of the repository, as the collection names it; `None` when it
    /// is outside the collection.
    fn in_collection<'p>(&self, path: &'p str) -> Option<&'p str> {
        if self.prefix.is_empty() {
            return Some(path);
        }
        path.strip_prefix(&self.prefix)?.strip_prefix('/')
    }

    /// The tree of the last commit; `None` before the first one.
    fn head_tree(repo: &gix::Repository) -> Result<Option<gix::Tree<'_>>, GitError> {
        if repo.head().map_err(failed)?.is_unborn() {
            return Ok(None);
        }
        let commit = repo.head_commit().map_err(failed)?;
        commit.tree().map(Some).map_err(failed)
    }

    /// The blob at `path` (of the repository) in `tree`.
    fn blob(tree: &gix::Tree<'_>, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let Some(entry) = tree.lookup_entry_by_path(path).map_err(failed)? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let object = entry.object().map_err(failed)?;
        Ok(Some(object.data.clone()))
    }
}

impl GitRepository for GixRepository {
    fn status(&self) -> Result<GitStatus, GitError> {
        let repo = self.repo.to_thread_local();
        let branch = (repo.head_name().map_err(failed)?)
            .map(|name| name.shorten().to_str_lossy().into_owned());
        let patterns: Vec<BString> = if self.prefix.is_empty() {
            Vec::new()
        } else {
            vec![BString::from(format!("{}/", self.prefix))]
        };
        let items = (repo.status(gix::progress::Discard).map_err(failed)?)
            .untracked_files(UntrackedFiles::Files)
            .into_iter(patterns)
            .map_err(failed)?;
        let mut paths: Vec<String> = Vec::new();
        for item in items {
            let item = item.map_err(failed)?;
            if let gix::status::Item::IndexWorktree(change) = &item
                && change.summary().is_none()
            {
                continue;
            }
            let path = item.location().to_str_lossy().into_owned();
            if self.in_collection(&path).is_some() && !paths.contains(&path) {
                paths.push(path);
            }
        }
        // What changed is said against the last commit, staged or not.
        let tree = Self::head_tree(&repo)?;
        let workdir = repo.workdir().map(Path::to_path_buf).unwrap_or_default();
        let mut changes = Vec::new();
        for path in paths {
            let in_head = match &tree {
                Some(tree) => Self::blob(tree, &path)?,
                None => None,
            };
            let on_disk = std::fs::read(workdir.join(&path)).ok();
            let change = match (&in_head, &on_disk) {
                (None, Some(_)) => FileChange::Added,
                (Some(_), None) => FileChange::Deleted,
                (Some(old), Some(new)) if old != new => FileChange::Modified,
                _ => continue,
            };
            let Some(relative) = self.in_collection(&path) else {
                continue;
            };
            changes.push(ChangedFile {
                path: relative.to_owned(),
                change,
            });
        }
        changes.sort();
        Ok(GitStatus { branch, changes })
    }

    fn head_file(&self, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let repo = self.repo.to_thread_local();
        match Self::head_tree(&repo)? {
            Some(tree) => Self::blob(&tree, &self.in_repo(path)),
            None => Ok(None),
        }
    }
}

impl GixRepository {
    /// The collection's root on disk.
    pub fn root(&self) -> &Path {
        &self.root
    }
}
