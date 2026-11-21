//! `apifast git`: the collection's repository (ADR 0014). Its branch and
//! what changed, field by field; a commit of some of those files; and its
//! branches.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use apifast_app::{FileChange, FileDiff, GitRepository, GitStatus, commit_changes, git_diff};
use apifast_domain::FieldChange;
use apifast_storage_fs::FsCollection;

use crate::locate;

#[derive(clap::Args)]
pub struct GitArgs {
    /// The collection folder, or a folder inside it.
    #[arg(long, global = true, default_value = ".")]
    collection: PathBuf,
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(clap::Subcommand)]
enum GitCommand {
    /// The branch, and the files of the collection that changed since the
    /// last commit: A added, M modified, D deleted.
    Status,
    /// What changed in each file since the last commit, field by field.
    Diff {
        /// The files, relative to the collection; all that changed when
        /// none is given.
        paths: Vec<String>,
    },
    /// Commit files of the collection as they are, a deleted one as
    /// deleted. What else is staged stays staged.
    Commit {
        /// The commit's message.
        #[arg(short, long)]
        message: String,
        /// Every file that changed.
        #[arg(long, conflicts_with = "paths")]
        all: bool,
        /// The files, relative to the collection.
        paths: Vec<String>,
    },
    /// The local branches; the one checked out with `*`.
    Branches,
    /// Check out a local branch. Refused while a tracked file has changes.
    Switch {
        /// The branch.
        branch: String,
    },
}

/// Opens the repository of the collection at `root`: the one place that
/// says which adapter of the git port the CLI uses.
#[cfg(feature = "git")]
fn open_git(root: &Path) -> Result<Option<Box<dyn GitRepository>>> {
    Ok(apifast_git::GixRepository::discover(root)?
        .map(|repo| Box::new(repo) as Box<dyn GitRepository>))
}

/// Without the `git` feature there is no adapter: say so.
#[cfg(not(feature = "git"))]
fn open_git(_: &Path) -> Result<Option<Box<dyn GitRepository>>> {
    bail!("this build of apifast has no git: build it with the `git` feature")
}

/// Runs the subcommand.
///
/// # Errors
///
/// If the collection is in no repository, or git fails.
pub fn run(args: &GitArgs) -> Result<()> {
    let root = locate::collection_root(&args.collection)?;
    let Some(git) = open_git(&root)? else {
        bail!("`{}` is not in a git repository", root.display());
    };
    match &args.command {
        GitCommand::Status => print!("{}", status_text(&git.status()?)),
        GitCommand::Diff { paths } => {
            let collection = FsCollection::open(&root)?;
            let status = git.status()?;
            let files = chosen(&status, paths)?;
            for file in &files {
                print!("{}", diff_text(&git_diff(&collection, git.as_ref(), file)?));
            }
        }
        GitCommand::Commit {
            message,
            all,
            paths,
        } => {
            let status = git.status()?;
            let files: Vec<String> = if *all {
                status.changes.iter().map(|c| c.path.clone()).collect()
            } else {
                chosen(&status, paths)?
                    .into_iter()
                    .map(|c| c.path)
                    .collect()
            };
            if files.is_empty() {
                bail!("nothing to commit: no file of the collection changed");
            }
            let id = commit_changes(git.as_ref(), &files, message)?;
            let on = status.branch.as_deref().unwrap_or("a detached HEAD");
            println!("Committed {} file(s) on {on}: {id}", files.len());
        }
        GitCommand::Branches => {
            let current = git.status()?.branch;
            for branch in git.branches()? {
                let mark = if current.as_deref() == Some(branch.as_str()) {
                    '*'
                } else {
                    ' '
                };
                println!("{mark} {branch}");
            }
        }
        GitCommand::Switch { branch } => {
            git.switch_branch(branch)?;
            println!("Switched to {branch}");
        }
    }
    Ok(())
}

/// The changed files `paths` names; all of them when it names none. A path
/// that did not change is an error, so a typo does not go unnoticed.
fn chosen(status: &GitStatus, paths: &[String]) -> Result<Vec<apifast_app::ChangedFile>> {
    if paths.is_empty() {
        return Ok(status.changes.clone());
    }
    paths
        .iter()
        .map(|path| {
            let path = path.trim_start_matches("./").replace('\\', "/");
            (status.changes.iter())
                .find(|c| c.path == path)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("`{path}` has no changes since the last commit"))
        })
        .collect()
}

fn letter(change: FileChange) -> char {
    match change {
        FileChange::Added => 'A',
        FileChange::Modified => 'M',
        FileChange::Deleted => 'D',
    }
}

fn status_text(status: &GitStatus) -> String {
    let mut out = match &status.branch {
        Some(branch) => format!("On {branch}\n"),
        None => "On a detached HEAD\n".to_owned(),
    };
    if status.changes.is_empty() {
        out.push_str("Nothing changed since the last commit\n");
    }
    for change in &status.changes {
        let _ = writeln!(out, "{} {}", letter(change.change), change.path);
    }
    out
}

fn diff_text(diff: &FileDiff) -> String {
    let what = match diff.change {
        FileChange::Added => "added",
        FileChange::Modified => "modified",
        FileChange::Deleted => "deleted",
    };
    let mut out = format!("{} ({what})\n", diff.path);
    for field in &diff.fields {
        field_text(&mut out, field);
    }
    out
}

/// A field: `name: before → after` when both fit on a line; otherwise its
/// lines, `-` before and `+` after.
fn field_text(out: &mut String, field: &FieldChange) {
    let one_line = |v: &Option<String>| v.as_deref().is_none_or(|v| !v.contains('\n'));
    if one_line(&field.before) && one_line(&field.after) {
        let side = |v: &Option<String>| v.clone().unwrap_or_else(|| "(none)".into());
        let _ = writeln!(
            out,
            "  {}: {} → {}",
            field.field,
            side(&field.before),
            side(&field.after)
        );
        return;
    }
    let _ = writeln!(out, "  {}:", field.field);
    for (sign, value) in [('-', &field.before), ('+', &field.after)] {
        for line in value.iter().flat_map(|v| v.lines()) {
            let _ = writeln!(out, "    {sign} {line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_on_one_line_or_by_lines() {
        let mut out = String::new();
        field_text(
            &mut out,
            &FieldChange {
                field: "method".into(),
                before: Some("GET".into()),
                after: Some("POST".into()),
            },
        );
        field_text(
            &mut out,
            &FieldChange {
                field: "header x-team".into(),
                before: None,
                after: Some("X-Team: core".into()),
            },
        );
        field_text(
            &mut out,
            &FieldChange {
                field: "body".into(),
                before: Some("json\n{}".into()),
                after: None,
            },
        );
        assert_eq!(
            out,
            "  method: GET → POST\n  header x-team: (none) → X-Team: core\n  body:\n    - json\n    - {}\n"
        );
    }
}
