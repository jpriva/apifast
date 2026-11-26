//! The contract of the git port (ADR 0014): what any [`GitRepository`]
//! must do, as scenarios an adapter's tests run with
//! [`git_repository_contract!`]. Each scenario makes a repository in a
//! temporary folder with the `git` binary, opens it with the adapter, and
//! checks what it says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "test support: a failure fails the test that runs it"
)]

use std::path::Path;
use std::process::Command;

pub use apifast_app::{ChangedFile, FileChange, GitRepository};

/// Opens the repository of the collection at a root, as an adapter does:
/// `None` when the root is in no repository.
pub type Open<'a> = &'a dyn Fn(&Path) -> Option<Box<dyn GitRepository>>;

/// Writes one `#[test]` per scenario of the contract, for the adapter that
/// `$open` (a `Fn(&Path) -> Option<Box<dyn GitRepository>>`) opens.
#[macro_export]
macro_rules! git_repository_contract {
    ($open:expr) => {
        mod git_repository_contract {
            fn open(root: &::std::path::Path) -> Option<Box<dyn $crate::GitRepository>> {
                let open = $open;
                open(root)
            }

            $crate::git_repository_contract!(@tests
                a_collection_outside_any_repository_has_none,
                changes_are_said_against_the_last_commit_staged_or_not,
                a_collection_in_a_folder_sees_only_its_files_by_its_own_paths,
                before_the_first_commit_everything_is_added_and_head_has_nothing,
                a_detached_head_has_no_branch,
                a_commit_takes_the_files_chosen_as_they_are_on_disk,
                a_commit_keeps_what_else_was_staged,
                the_first_commit_starts_the_branch,
                a_collection_in_a_folder_commits_by_its_own_paths,
                a_commit_keeps_other_changes_visible_whatever_the_clock,
                a_later_git_commit_keeps_what_was_committed,
                the_local_branches_are_listed,
                switching_branches_changes_the_files_the_index_and_head,
                a_switch_is_refused_while_a_tracked_file_has_changes,
                a_switch_is_refused_when_it_would_overwrite_an_untracked_file,
                an_unknown_branch_cannot_be_switched_to,
            );
        }
    };
    (@tests $($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                $crate::$name(&open);
            }
        )*
    };
}

/// Runs `git` with `args` in `dir`, as a fixed user and without signing.
pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Writes `text` at `path` under `dir`, making its folders.
pub fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A new repository on branch `main`, in a temporary folder, with an
/// identity to commit as.
pub fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Tester"]);
    git(dir.path(), &["config", "user.email", "tester@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    // Files as written, whatever the system's git says: Windows runners
    // turn `\n` into `\r\n` on checkout by default.
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    dir
}

/// What `git` prints for `args` in `dir`, trimmed.
pub fn git_output(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| (*p).to_owned()).collect()
}

fn changed(path: &str, change: FileChange) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        change,
    }
}

fn opened(open: Open<'_>, root: &Path) -> Box<dyn GitRepository> {
    open(root).expect("the adapter finds the repository")
}

/// A folder in no repository has none.
pub fn a_collection_outside_any_repository_has_none(open: Open<'_>) {
    let dir = tempfile::tempdir().unwrap();
    assert!(open(dir.path()).is_none());
}

/// Added, modified and deleted are said against `HEAD`, whatever is
/// staged; a change undone and an ignored file are not changes.
pub fn changes_are_said_against_the_last_commit_staged_or_not(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "a.yml", "a: 1\n");
    write(root, "b.yml", "b: 1\n");
    write(root, "same.yml", "s: 1\n");
    write(root, ".gitignore", "*.log\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);

    write(root, "a.yml", "a: 2\n");
    std::fs::remove_file(root.join("b.yml")).unwrap();
    write(root, "users/new.yml", "n: 1\n");
    write(root, "staged.yml", "st: 1\n");
    git(root, &["add", "staged.yml"]);
    // Changed, staged, then changed back: nothing against HEAD.
    write(root, "same.yml", "s: 2\n");
    git(root, &["add", "same.yml"]);
    write(root, "same.yml", "s: 1\n");
    write(root, "debug.log", "ignored");

    let repo = opened(open, root);
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(
        status.changes,
        [
            changed("a.yml", FileChange::Modified),
            changed("b.yml", FileChange::Deleted),
            changed("staged.yml", FileChange::Added),
            changed("users/new.yml", FileChange::Added),
        ]
    );
    assert_eq!(
        repo.head_file("a.yml").unwrap().as_deref(),
        Some(&b"a: 1\n"[..])
    );
    assert_eq!(
        repo.head_file("b.yml").unwrap().as_deref(),
        Some(&b"b: 1\n"[..])
    );
    assert_eq!(repo.head_file("users/new.yml").unwrap(), None);
}

/// A collection in a folder of a larger repository sees its own files
/// only, by paths relative to it.
pub fn a_collection_in_a_folder_sees_only_its_files_by_its_own_paths(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "api/a.yml", "a: 1\n");
    write(root, "other/x.txt", "x");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    write(root, "api/a.yml", "a: 2\n");
    write(root, "other/x.txt", "y");
    write(root, "top.txt", "t");

    let repo = opened(open, &root.join("api"));
    assert_eq!(
        repo.status().unwrap().changes,
        [changed("a.yml", FileChange::Modified)]
    );
    assert_eq!(
        repo.head_file("a.yml").unwrap().as_deref(),
        Some(&b"a: 1\n"[..])
    );
}

/// Before the first commit every file is added, and `HEAD` has none.
pub fn before_the_first_commit_everything_is_added_and_head_has_nothing(open: Open<'_>) {
    let dir = repo();
    write(dir.path(), "a.yml", "a: 1\n");
    let repo = opened(open, dir.path());
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.changes, [changed("a.yml", FileChange::Added)]);
    assert_eq!(repo.head_file("a.yml").unwrap(), None);
}

/// A detached `HEAD` is on no branch.
pub fn a_detached_head_has_no_branch(open: Open<'_>) {
    let dir = repo();
    write(dir.path(), "a.yml", "a: 1\n");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "first"]);
    git(dir.path(), &["checkout", "-q", "--detach"]);
    let repo = opened(open, dir.path());
    assert_eq!(repo.status().unwrap().branch, None);
}

/// A commit takes the files chosen as they are on disk, a deleted one as
/// deleted; the others stay changed, and git agrees.
pub fn a_commit_takes_the_files_chosen_as_they_are_on_disk(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "a.yml", "a: 1\n");
    write(root, "b.yml", "b: 1\n");
    write(root, "c.yml", "c: 1\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    write(root, "a.yml", "a: 2\n");
    write(root, "b.yml", "b: 2\n");
    std::fs::remove_file(root.join("c.yml")).unwrap();
    write(root, "users/new.yml", "n: 1\n");

    let repo = opened(open, root);
    let id = repo
        .commit(
            &paths(&["a.yml", "c.yml", "users/new.yml"]),
            "Change a, drop c",
        )
        .unwrap();
    assert!(id.len() >= 4, "an abbreviated id: {id}");
    assert_eq!(
        repo.status().unwrap().changes,
        [changed("b.yml", FileChange::Modified)]
    );
    assert_eq!(
        repo.head_file("a.yml").unwrap().as_deref(),
        Some(&b"a: 2\n"[..])
    );
    assert_eq!(repo.head_file("c.yml").unwrap(), None);
    assert_eq!(
        git_output(root, &["log", "-1", "--format=%s|%an|%ae"]),
        "Change a, drop c|Tester|tester@example.com"
    );
    assert_eq!(git_output(root, &["rev-list", "--count", "HEAD"]), "2");
    assert!(
        git_output(root, &["rev-parse", "--short", "HEAD"]).starts_with(&id[..7.min(id.len())])
    );
    // git sees the same: only b changed, nothing staged.
    assert_eq!(git_output(root, &["status", "--porcelain"]), "M b.yml");
    // And what was written is sound.
    git(root, &["fsck", "--no-progress", "--strict"]);
}

/// What else was staged stays staged after a commit of other files.
pub fn a_commit_keeps_what_else_was_staged(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "a.yml", "a: 1\n");
    write(root, "b.yml", "b: 1\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    write(root, "a.yml", "a: 2\n");
    write(root, "b.yml", "b: 2\n");
    git(root, &["add", "b.yml"]);

    opened(open, root)
        .commit(&paths(&["a.yml"]), "Change a")
        .unwrap();
    assert_eq!(
        git_output(root, &["diff", "--cached", "--name-only"]),
        "b.yml"
    );
    assert_eq!(git_output(root, &["status", "--porcelain"]), "M  b.yml");
}

/// The first commit of a repository starts its branch.
pub fn the_first_commit_starts_the_branch(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "a.yml", "a: 1\n");
    write(root, "users/b.yml", "b: 1\n");
    let repo = opened(open, root);
    repo.commit(&paths(&["a.yml", "users/b.yml"]), "First")
        .unwrap();
    assert_eq!(repo.status().unwrap().changes, []);
    assert_eq!(git_output(root, &["log", "--format=%s", "main"]), "First");
    assert_eq!(git_output(root, &["status", "--porcelain"]), "");
}

/// A collection in a folder commits its files by its own paths.
pub fn a_collection_in_a_folder_commits_by_its_own_paths(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "api/a.yml", "a: 1\n");
    write(root, "top.txt", "t");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    write(root, "api/a.yml", "a: 2\n");
    write(root, "top.txt", "u");

    opened(open, &root.join("api"))
        .commit(&paths(&["a.yml"]), "Change a")
        .unwrap();
    assert_eq!(git_output(root, &["status", "--porcelain"]), "M top.txt");
    assert_eq!(git_output(root, &["show", "HEAD:api/a.yml"]), "a: 2");
}

/// A commit keeps the other files' changes visible to git, even one made
/// in the same second as the index was last written, with the same size.
/// Git only finds those by reading the file ("racy git"): rewriting the
/// index must keep them marked to be read, as git does.
pub fn a_commit_keeps_other_changes_visible_whatever_the_clock(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    // Dated at a set time, so the test does not depend on the clock.
    let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let set_mtime = |path: &str| {
        let times = std::fs::FileTimes::new().set_modified(past);
        let file = std::fs::File::options().write(true).open(root.join(path));
        file.and_then(|file| file.set_times(times)).unwrap();
    };
    // ctime is left out, so that only the mtime set below counts.
    git(root, &["config", "core.trustctime", "false"]);
    write(root, "a.yml", "a: 1\n");
    write(root, "top.txt", "t");
    set_mtime("top.txt");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    // Changed in the same second as the index, with the same size and
    // time: only the index's timestamp tells git to read it.
    write(root, "top.txt", "u");
    set_mtime("top.txt");
    set_mtime(".git/index");
    // Without writing the index, which would mark the file itself.
    let status = ["--no-optional-locks", "status", "--porcelain"];
    assert_eq!(git_output(root, &status), "M top.txt");

    write(root, "a.yml", "a: 2\n");
    opened(open, root)
        .commit(&paths(&["a.yml"]), "Change a")
        .unwrap();
    assert_eq!(git_output(root, &status), "M top.txt");
}

/// A commit leaves no stale tree cache in the index: a later `git commit`
/// of another file keeps what was committed, and does not take a folder
/// as it was before.
pub fn a_later_git_commit_keeps_what_was_committed(open: Open<'_>) {
    let dir = repo();
    let root = dir.path();
    write(root, "api/a.yml", "a: 1\n");
    write(root, "top.txt", "t");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    // git keeps the trees it built in the index.
    git(root, &["write-tree"]);

    write(root, "api/a.yml", "a: 2\n");
    opened(open, root)
        .commit(&paths(&["api/a.yml"]), "Change a")
        .unwrap();
    write(root, "top.txt", "u");
    git(root, &["add", "top.txt"]);
    git(root, &["commit", "-q", "-m", "Change top"]);
    assert_eq!(git_output(root, &["show", "HEAD:api/a.yml"]), "a: 2");
    assert_eq!(git_output(root, &["status", "--porcelain"]), "");
}

/// A repository with `main` (a, b, run.sh) and `feature` (a changed, b
/// deleted, c added, run.sh executable), checked out on `main`.
fn two_branches() -> tempfile::TempDir {
    let dir = repo();
    let root = dir.path();
    write(root, "a.yml", "a: main\n");
    write(root, "b.yml", "b: 1\n");
    write(root, "tools/run.sh", "echo main\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "main"]);
    git(root, &["switch", "-q", "-c", "feature"]);
    write(root, "a.yml", "a: feature\n");
    std::fs::remove_file(root.join("b.yml")).unwrap();
    write(root, "users/c.yml", "c: 1\n");
    write(root, "tools/run.sh", "echo feature\n");
    git(root, &["add", "."]);
    // After `add`, which would take the mode from disk again; on disk too,
    // so the working tree matches.
    git(root, &["update-index", "--chmod=+x", "tools/run.sh"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("tools/run.sh");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    git(root, &["commit", "-q", "-m", "feature"]);
    git(root, &["switch", "-q", "main"]);
    dir
}

fn read(root: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(root.join(path)).ok()
}

/// The local branches are listed by name, sorted.
pub fn the_local_branches_are_listed(open: Open<'_>) {
    let dir = two_branches();
    git(dir.path(), &["branch", "a-fix"]);
    let repo = opened(open, dir.path());
    assert_eq!(repo.branches().unwrap(), ["a-fix", "feature", "main"]);
}

/// A switch makes the files, the index and `HEAD` the branch's, and git
/// agrees; untracked files it does not touch stay.
pub fn switching_branches_changes_the_files_the_index_and_head(open: Open<'_>) {
    let dir = two_branches();
    let root = dir.path();
    write(root, "notes.txt", "mine");
    let repo = opened(open, root);

    repo.switch_branch("feature").unwrap();
    assert_eq!(read(root, "a.yml").as_deref(), Some("a: feature\n"));
    assert_eq!(read(root, "b.yml"), None);
    assert_eq!(read(root, "users/c.yml").as_deref(), Some("c: 1\n"));
    assert_eq!(read(root, "notes.txt").as_deref(), Some("mine"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(root.join("tools/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "run.sh is executable on feature");
    }
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("feature"));
    assert_eq!(status.changes, [changed("notes.txt", FileChange::Added)]);
    assert_eq!(
        git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feature"
    );
    assert_eq!(git_output(root, &["status", "--porcelain"]), "?? notes.txt");

    repo.switch_branch("main").unwrap();
    assert_eq!(read(root, "a.yml").as_deref(), Some("a: main\n"));
    assert_eq!(read(root, "b.yml").as_deref(), Some("b: 1\n"));
    assert_eq!(read(root, "users/c.yml"), None);
    assert!(!root.join("users").exists(), "a folder left empty goes too");
    assert_eq!(git_output(root, &["status", "--porcelain"]), "?? notes.txt");
}

/// While a tracked file of the repository has changes, even outside the
/// collection, a switch is refused and nothing changes.
pub fn a_switch_is_refused_while_a_tracked_file_has_changes(open: Open<'_>) {
    let dir = two_branches();
    let root = dir.path();
    write(root, "tools/run.sh", "echo edited\n");
    let repo = opened(open, root);
    assert!(repo.switch_branch("feature").is_err());
    assert_eq!(read(root, "a.yml").as_deref(), Some("a: main\n"));
    assert_eq!(read(root, "tools/run.sh").as_deref(), Some("echo edited\n"));
    assert_eq!(
        git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
}

/// An untracked file the branch has would be overwritten: refused.
pub fn a_switch_is_refused_when_it_would_overwrite_an_untracked_file(open: Open<'_>) {
    let dir = two_branches();
    let root = dir.path();
    write(root, "users/c.yml", "mine\n");
    let repo = opened(open, root);
    assert!(repo.switch_branch("feature").is_err());
    assert_eq!(read(root, "users/c.yml").as_deref(), Some("mine\n"));
    assert_eq!(
        git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
}

/// There is no switching to a branch that is not there.
pub fn an_unknown_branch_cannot_be_switched_to(open: Open<'_>) {
    let dir = two_branches();
    let repo = opened(open, dir.path());
    assert!(repo.switch_branch("nope").is_err());
    assert_eq!(
        git_output(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
}
