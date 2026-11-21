//! The git repository of each loaded collection (docs/09, F10, ADR 0014):
//! its branch and what changed, refreshed when the collection's files
//! change; the field-by-field diff of a file; commits and branch switches.

use std::path::Path;
use std::sync::Arc;

use apifast_app::{ChangedFile, FileDiff, GitRepository, commit_changes, git_diff};

use super::State;

/// What the interface sees of a collection's repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitView {
    /// The branch checked out; `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// The files that changed since the last commit, by path.
    pub changes: Vec<ChangedFile>,
    /// The local branches, sorted.
    pub branches: Vec<String>,
    /// The diff the user asked for last, of one of [`Self::changes`].
    pub diff: Option<FileDiff>,
    /// What the last commit or switch said: `Committed 2 files: 1a2b3c4`.
    pub notice: Option<String>,
    /// Why the last thing asked failed.
    pub error: Option<String>,
}

/// What the interface asks of the focused collection's repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitCommand {
    /// Reads its status and branches again.
    Refresh,
    /// Says what changed in the file at this path, field by field.
    Diff(String),
    /// Commits these files with this message.
    Commit {
        /// The files, relative to the collection.
        paths: Vec<String>,
        /// The message.
        message: String,
    },
    /// Checks out this branch.
    Switch(String),
}

/// Opens the repository of the collection at `root`: the one place that
/// says which adapter of the git port the interface uses. `None` when the
/// collection is in no repository, or it cannot be read.
#[cfg(feature = "git")]
pub(super) fn open_git(root: &Path) -> Option<Arc<dyn GitRepository>> {
    let repo = apifast_git::GixRepository::discover(root).ok()??;
    Some(Arc::new(repo))
}

/// Without the `git` feature no repository is read: the interface shows
/// nothing of git.
#[cfg(not(feature = "git"))]
pub(super) fn open_git(_: &Path) -> Option<Arc<dyn GitRepository>> {
    None
}

/// The status and branches of `git`, keeping what `view` showed besides.
fn read(git: &dyn GitRepository, view: Option<&GitView>) -> GitView {
    let mut next = view.cloned().unwrap_or_default();
    match git.status() {
        Ok(status) => {
            next.branch = status.branch;
            next.changes = status.changes;
        }
        Err(e) => next.error = Some(e.to_string()),
    }
    match git.branches() {
        Ok(branches) => next.branches = branches,
        Err(e) => next.error = Some(e.to_string()),
    }
    // A diff of a file that no longer changed is gone.
    if let Some(diff) = &next.diff
        && !next.changes.iter().any(|c| c.path == diff.path)
    {
        next.diff = None;
    }
    next
}

impl State {
    /// Reads again the repository of the collection at `root`, if it has one.
    pub(super) fn refresh_git(&mut self, root: &Path) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let Some(git) = open.git.clone() else {
            return;
        };
        open.git_view = Some(read(&*git, open.git_view.as_ref()));
    }

    /// Does `command` in the focused collection's repository.
    pub(super) fn git_command(&mut self, command: GitCommand) {
        let Some(root) = self.focus.clone() else {
            return;
        };
        // Unsaved changes would turn into conflicts after a switch.
        let drafts = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root && tab.draft.is_some())
            .count();
        let Some(open) = self.open.get_mut(&root) else {
            return;
        };
        let Some(git) = open.git.clone() else {
            return;
        };
        let mut view = open.git_view.clone().unwrap_or_default();
        view.error = None;
        match command {
            GitCommand::Refresh => view.notice = None,
            GitCommand::Diff(path) => {
                let file = view.changes.iter().find(|c| c.path == path).cloned();
                if let Some(file) = file {
                    match git_diff(&*open.repo, &*git, &file) {
                        Ok(diff) => view.diff = Some(diff),
                        Err(e) => view.error = Some(e.to_string()),
                    }
                }
            }
            GitCommand::Commit { paths, message } => {
                match commit_changes(&*git, &paths, &message) {
                    Ok(id) => {
                        view.notice = Some(format!("Committed {} file(s): {id}", paths.len()));
                    }
                    Err(e) => view.error = Some(e.to_string()),
                }
            }
            GitCommand::Switch(branch) if drafts > 0 => {
                view.error = Some(format!(
                    "save or discard the unsaved changes of {drafts} tab(s) before switching to {branch}"
                ));
            }
            GitCommand::Switch(branch) => match git.switch_branch(&branch) {
                Ok(()) => view.notice = Some(format!("Switched to {branch}")),
                Err(e) => view.error = Some(e.to_string()),
            },
        }
        let error = view.error.take();
        let mut view = read(&*git, Some(&view));
        if error.is_some() {
            view.error = error;
        }
        open.git_view = Some(view);
    }
}
