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

    fn commit(&self, paths: &[String], message: &str) -> Result<String, GitError> {
        let repo = self.repo.to_thread_local();
        if repo.committer().is_none() || repo.author().is_none() {
            return Err(GitError(
                "set your name and email first: git config user.name, git config user.email".into(),
            ));
        }
        let workdir = repo.workdir().map(Path::to_path_buf).unwrap_or_default();
        let head = Self::head_tree(&repo)?;
        let parents: Vec<gix::ObjectId> = match &head {
            Some(_) => vec![repo.head_id().map_err(failed)?.detach()],
            None => Vec::new(),
        };
        let base = head
            .as_ref()
            .map_or_else(|| repo.empty_tree().id, |tree| tree.id);
        let mut editor = repo.edit_tree(base).map_err(failed)?;
        // What each path is now: a blob and its mode, or gone.
        let mut written: Vec<(
            String,
            Option<(gix::ObjectId, gix::object::tree::EntryKind)>,
        )> = Vec::new();
        for path in paths {
            let in_repo = self.in_repo(path);
            let on_disk = workdir.join(&in_repo);
            match std::fs::read(&on_disk) {
                Ok(bytes) => {
                    let id = repo.write_blob(bytes).map_err(failed)?.detach();
                    let kind = entry_kind(&on_disk);
                    editor.upsert(in_repo.as_str(), kind, id).map_err(failed)?;
                    written.push((in_repo, Some((id, kind))));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    editor.remove(in_repo.as_str()).map_err(failed)?;
                    written.push((in_repo, None));
                }
                Err(e) => return Err(failed(e)),
            }
        }
        let tree = editor.write().map_err(failed)?.detach();
        let id = repo
            .commit("HEAD", message, tree, parents)
            .map_err(failed)?;
        Self::update_index(&repo, &workdir, &written)?;
        Ok(id.shorten_or_id().to_string())
    }
}

/// The mode a file goes into a tree with: executable or not.
fn entry_kind(path: &Path) -> gix::object::tree::EntryKind {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
            return gix::object::tree::EntryKind::BlobExecutable;
        }
    }
    let _ = path;
    gix::object::tree::EntryKind::Blob
}

impl GixRepository {
    /// The index after a commit of `written`: those paths as committed,
    /// everything else as it was staged.
    fn update_index(
        repo: &gix::Repository,
        workdir: &Path,
        written: &[(
            String,
            Option<(gix::ObjectId, gix::object::tree::EntryKind)>,
        )],
    ) -> Result<(), GitError> {
        let mut index = match repo.open_index() {
            Ok(index) => index,
            // No index yet: one with what was just committed.
            Err(_) => gix::index::File::from_state(
                gix::index::State::new(repo.object_hash()),
                repo.index_path(),
            ),
        };
        let committed: Vec<&str> = written.iter().map(|(p, _)| p.as_str()).collect();
        index.remove_entries(|_, path, _| committed.contains(&path.to_str_lossy().as_ref()));
        for (path, blob) in written {
            let Some((id, kind)) = blob else { continue };
            let full = workdir.join(path);
            let stat = gix::index::fs::Metadata::from_path_no_follow(&full)
                .ok()
                .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
                .unwrap_or_default();
            let mode = match kind {
                gix::object::tree::EntryKind::BlobExecutable => {
                    gix::index::entry::Mode::FILE_EXECUTABLE
                }
                _ => gix::index::entry::Mode::FILE,
            };
            index.dangerously_push_entry(
                stat,
                *id,
                gix::index::entry::Flags::empty(),
                mode,
                path.as_str().into(),
            );
        }
        index.sort_entries();
        index
            .write(gix::index::write::Options::default())
            .map_err(failed)
    }
}

impl GixRepository {
    /// The collection's root on disk.
    pub fn root(&self) -> &Path {
        &self.root
    }
}
