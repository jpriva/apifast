//! The git repository a collection lives in, read with gitoxide (`gix`,
//! ADR 0014): pure Rust, no `git` binary needed.
//!
//! A collection may be the repository, or a folder inside a larger one: only
//! the files under the collection's root are seen, and paths are relative to
//! it.

use std::collections::BTreeMap;
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
    GitError::Failed(error.to_string())
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
        let canonical = |path: &Path| dunce::canonicalize(path).map_err(failed);
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
            return Err(GitError::NoIdentity);
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

    fn branches(&self) -> Result<Vec<String>, GitError> {
        let repo = self.repo.to_thread_local();
        let references = repo.references().map_err(failed)?;
        let mut names = Vec::new();
        for reference in references.local_branches().map_err(failed)? {
            let reference = reference.map_err(failed)?;
            names.push(reference.name().shorten().to_str_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    fn switch_branch(&self, branch: &str) -> Result<(), GitError> {
        let repo = self.repo.to_thread_local();
        let full = format!("refs/heads/{branch}");
        let current = repo.head_name().map_err(failed)?;
        if current
            .as_ref()
            .is_some_and(|name| name.as_bstr() == full.as_str())
        {
            return Ok(());
        }
        let mut reference = (repo.find_reference(full.as_str()))
            .map_err(|_| GitError::NoBranch(branch.to_owned()))?;
        let target = reference
            .peel_to_commit()
            .map_err(failed)?
            .tree()
            .map_err(failed)?;
        let wanted = files_of(&target)?;
        let had = match Self::head_tree(&repo)? {
            Some(tree) => files_of(&tree)?,
            None => BTreeMap::new(),
        };
        let workdir = repo.workdir().map(Path::to_path_buf).unwrap_or_default();
        Self::refuse_losing_work(&repo, &workdir, (&had, &wanted))?;

        // The working tree: what the branch does not have goes, what it has
        // otherwise is written.
        for path in had.keys().filter(|path| !wanted.contains_key(*path)) {
            remove_file(&workdir, path)?;
        }
        for (path, (id, mode)) in &wanted {
            if had.get(path) != Some(&(*id, *mode)) {
                write_entry(&repo, &workdir.join(path), *id, *mode)?;
            }
        }
        // The index: the branch's tree, with the files' stats.
        let mut index = repo.index_from_tree(&target.id).map_err(failed)?;
        let (entries, paths) = index.entries_mut_and_pathbacking();
        for entry in entries {
            let path = workdir.join(entry.path_in(paths).to_str_lossy().as_ref());
            if let Some(stat) = gix::index::fs::Metadata::from_path_no_follow(&path)
                .ok()
                .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
            {
                entry.stat = stat;
            }
        }
        index
            .write(gix::index::write::Options::default())
            .map_err(failed)?;
        // And `HEAD`.
        let from =
            current.map_or_else(|| "a detached HEAD".to_owned(), |n| n.shorten().to_string());
        let edit = gix::refs::transaction::RefEdit {
            change: gix::refs::transaction::Change::Update {
                log: gix::refs::transaction::LogChange {
                    mode: gix::refs::transaction::RefLog::AndReference,
                    force_create_reflog: false,
                    message: format!("checkout: moving from {from} to {branch}").into(),
                },
                expected: gix::refs::transaction::PreviousValue::Any,
                new: gix::refs::Target::Symbolic(full.as_str().try_into().map_err(failed)?),
            },
            name: "HEAD".try_into().map_err(failed)?,
            deref: false,
        };
        repo.edit_reference(edit).map_err(failed)?;
        Ok(())
    }
}

/// A tree's files (not its folders), by path: their blob and mode.
type Files = BTreeMap<String, (gix::ObjectId, gix::object::tree::EntryMode)>;

fn files_of(tree: &gix::Tree<'_>) -> Result<Files, GitError> {
    let mut recorder = gix::traverse::tree::Recorder::default();
    tree.traverse()
        .breadthfirst(&mut recorder)
        .map_err(failed)?;
    Ok((recorder.records.into_iter())
        .filter(|entry| !entry.mode.is_tree())
        .map(|entry| {
            (
                entry.filepath.to_str_lossy().into_owned(),
                (entry.oid, entry.mode),
            )
        })
        .collect())
}

/// Removes `path` of the working tree, and the folders it leaves empty.
fn remove_file(workdir: &Path, path: &str) -> Result<(), GitError> {
    let full = workdir.join(path);
    match std::fs::remove_file(&full) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed(e)),
    }
    let mut dir = full.parent();
    while let Some(folder) = dir.filter(|d| *d != workdir) {
        // Not empty, or not there: the folders above stay too.
        if std::fs::remove_dir(folder).is_err() {
            break;
        }
        dir = folder.parent();
    }
    Ok(())
}

/// Writes the blob `id` at `path` as `mode` says: a file (executable or
/// not) or a link. A submodule is left alone.
fn write_entry(
    repo: &gix::Repository,
    path: &Path,
    id: gix::ObjectId,
    mode: gix::object::tree::EntryMode,
) -> Result<(), GitError> {
    if mode.is_commit() {
        return Ok(());
    }
    let data = repo.find_object(id).map_err(failed)?.data.clone();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed(e)),
    }
    #[cfg(unix)]
    if mode.is_link() {
        let target = String::from_utf8_lossy(&data).into_owned();
        return std::os::unix::fs::symlink(target, path).map_err(failed);
    }
    std::fs::write(path, &data).map_err(failed)?;
    #[cfg(unix)]
    if mode.is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).map_err(failed)?.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        std::fs::set_permissions(path, permissions).map_err(failed)?;
    }
    Ok(())
}

impl GixRepository {
    /// Refuses a switch that would lose work: a tracked file of the
    /// repository with changes (staged or not), or an untracked one where
    /// the branch has a file.
    fn refuse_losing_work(
        repo: &gix::Repository,
        workdir: &Path,
        (had, wanted): (&Files, &Files),
    ) -> Result<(), GitError> {
        let index = repo.index_or_empty().map_err(failed)?;
        let items = (repo.status(gix::progress::Discard).map_err(failed)?)
            .untracked_files(UntrackedFiles::Files)
            .into_iter(Vec::<BString>::new())
            .map_err(failed)?;
        let mut changed = Vec::new();
        let mut overwritten = Vec::new();
        for item in items {
            let item = item.map_err(failed)?;
            if let gix::status::Item::IndexWorktree(change) = &item
                && change.summary().is_none()
            {
                continue;
            }
            let path = item.location().to_str_lossy().into_owned();
            let tracked =
                had.contains_key(&path) || index.entry_by_path(path.as_str().into()).is_some();
            if tracked {
                // Changed back to what was committed is no change.
                let on_disk = std::fs::read(workdir.join(&path)).ok();
                let committed = had
                    .get(&path)
                    .and_then(|(id, _)| repo.find_object(*id).ok().map(|o| o.data.clone()));
                let staged = index.entry_by_path(path.as_str().into()).map(|e| e.id);
                let same = on_disk.is_some()
                    && on_disk == committed
                    && staged == had.get(&path).map(|(id, _)| *id);
                if !same && !changed.contains(&path) {
                    changed.push(path);
                }
            } else if wanted.contains_key(&path) && !overwritten.contains(&path) {
                overwritten.push(path);
            }
        }
        if !changed.is_empty() {
            return Err(GitError::Uncommitted(changed));
        }
        if !overwritten.is_empty() {
            return Err(GitError::Untracked(overwritten));
        }
        Ok(())
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
        // Git finds some changes only by reading the file: those made in the
        // same second as the index was written, with the same size ("racy
        // git"). Their entries are dated at or after the index read; the
        // new one is dated now, past them, so git would trust their stat
        // and miss the change. As git does when it writes an index, they
        // are marked to be read (size 0, a "smudged" entry).
        let since = index.timestamp().unix_seconds();
        for entry in index.entries_mut() {
            if i64::from(entry.stat.mtime.secs) >= since {
                entry.stat.size = 0;
            }
        }
        // The tree cache would still say the old trees are current, and a
        // later `git commit` could take them as they were (gitoxide #2421).
        index.remove_tree();
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
