//! The collection's git repository in the interface: the status, refreshed
//! when files change; the diff of a file; a commit; branches.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use apifast_app::{ChangedFile, FileChange, FileDiff, RequestPath};
use apifast_domain::FieldChange;
use apifast_git_contract::{git, git_output, repo, write};
use apifast_gui::core::{Command, Core, GitCommand, GitView, Snapshot, TabKey};
use apifast_vault::SystemKeyring;

fn start() -> (Core, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let core = Core::start_with(data.path().to_path_buf(), SystemKeyring::off(), || {});
    (core, data)
}

fn wait(core: &Core, what: &str, done: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let start = Instant::now();
    loop {
        let snapshot = core.snapshot();
        if done(&snapshot) {
            return snapshot;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}: {:#?}",
            snapshot.git
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

const LIST: &str =
    "info:\n  name: List\n  type: http\nhttp:\n  method: GET\n  url: \"{{host}}/users\"\n";

#[test]
fn the_status_follows_the_files_and_a_commit_and_a_switch_change_it() {
    let dir = repo();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\n",
    );
    write(root, "list.yml", LIST);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    git(root, &["branch", "feature"]);
    let (core, _data) = start();
    core.send(Command::OpenCollection(root.to_path_buf()));
    let snapshot = wait(&core, "the repository", |s| s.git.is_some());
    let view = snapshot.git.as_ref().unwrap();
    assert_eq!(view.branch.as_deref(), Some("main"));
    assert_eq!(view.branches, ["feature", "main"]);
    assert_eq!(view.changes, []);

    // A change on disk shows, by the watcher.
    write(root, "list.yml", &LIST.replace("GET", "POST"));
    wait(&core, "the change", |s| {
        s.git.as_ref().is_some_and(|g| g.changes.len() == 1)
    });
    core.send(Command::Git(GitCommand::Diff("list.yml".into())));
    let snapshot = wait(&core, "the diff", |s| {
        s.git.as_ref().is_some_and(|g| g.diff.is_some())
    });
    assert_eq!(
        snapshot.git.as_ref().unwrap().diff.as_ref().unwrap().fields,
        [FieldChange {
            field: "method".into(),
            before: Some("GET".into()),
            after: Some("POST".into()),
        }]
    );

    // A tab with unsaved changes stops a switch.
    let root = snapshot.collection.as_ref().unwrap().root.clone();
    let tab = TabKey::new(root.clone(), RequestPath::new("list.yml"));
    core.send(Command::OpenRequest(tab.clone()));
    let snapshot = wait(&core, "the tab", |s| s.active_tab().is_some());
    let mut request = snapshot.active_tab().unwrap().request().clone();
    request.url = "{{host}}/draft".into();
    core.send(Command::Edit {
        tab: tab.clone(),
        request: Box::new(request),
    });
    wait(&core, "the draft", |s| {
        s.active_tab().is_some_and(apifast_gui::core::Tab::is_dirty)
    });
    core.send(Command::Git(GitCommand::Switch("feature".into())));
    let snapshot = wait(&core, "the refusal", |s| {
        s.git.as_ref().is_some_and(|g| g.error.is_some())
    });
    assert!(
        snapshot
            .git
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("unsaved")
    );
    core.send(Command::CloseTab(tab));

    core.send(Command::Git(GitCommand::Commit {
        paths: vec!["list.yml".into()],
        message: "Post users".into(),
    }));
    let snapshot = wait(&core, "the commit", |s| {
        s.git
            .as_ref()
            .is_some_and(|g| g.notice.is_some() && g.changes.is_empty())
    });
    let view = snapshot.git.as_ref().unwrap();
    assert!(
        view.notice
            .as_ref()
            .unwrap()
            .starts_with("Committed 1 file(s): ")
    );
    assert_eq!(
        view.diff, None,
        "a diff of a file that no longer changed goes"
    );
    assert_eq!(
        git_output(&root, &["log", "-1", "--format=%s"]),
        "Post users"
    );

    core.send(Command::Git(GitCommand::Switch("feature".into())));
    wait(&core, "the switch", |s| {
        s.git
            .as_ref()
            .is_some_and(|g| g.branch.as_deref() == Some("feature"))
    });
    assert_eq!(
        std::fs::read_to_string(root.join("list.yml")).unwrap(),
        LIST
    );
}

#[test]
fn a_collection_in_no_repository_has_no_git() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shop\n",
    );
    let (core, _data) = start();
    core.send(Command::OpenCollection(dir.path().to_path_buf()));
    let snapshot = wait(&core, "the collection", |s| s.collection.is_some());
    assert_eq!(snapshot.git, None);
}

mod window {
    use super::*;
    use apifast_gui::organisms::GitWindow;
    use apifast_gui::strings::EN;
    use eframe::egui;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        window: GitWindow,
        view: GitView,
        commands: Vec<GitCommand>,
    }

    fn changed(path: &str, change: FileChange) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            change,
        }
    }

    fn harness() -> Harness<'static, State> {
        let mut window = GitWindow::default();
        let first = window.open();
        let view = GitView {
            branch: Some("main".into()),
            branches: vec!["feature".into(), "main".into()],
            changes: vec![
                changed("list.yml", FileChange::Modified),
                changed("new.yml", FileChange::Added),
            ],
            diff: Some(FileDiff {
                path: "list.yml".into(),
                change: FileChange::Modified,
                fields: vec![FieldChange {
                    field: "method".into(),
                    before: Some("GET".into()),
                    after: Some("POST".into()),
                }],
            }),
            ..GitView::default()
        };
        Harness::builder()
            .with_size(egui::vec2(900.0, 700.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    let ctx = ui.ctx().clone();
                    let commands = state.window.show(&ctx, Some(&state.view), &EN);
                    state.commands.extend(commands);
                },
                State {
                    window,
                    view,
                    commands: vec![first],
                },
            )
    }

    #[test]
    fn a_file_shows_its_diff_and_the_files_checked_are_committed() {
        let mut harness = harness();
        harness.run();
        assert!(
            harness
                .query_by_label(&format!("{}: main", EN.git_branch))
                .is_some()
        );
        harness.get_by_label("list.yml").click();
        harness.run();
        assert!(harness.query_by_label("method").is_some());
        assert!(harness.query_by_label("- GET").is_some());
        assert!(harness.query_by_label("+ POST").is_some());

        // `new.yml` is left out.
        harness
            .get_by_label(&format!("{}: new.yml", EN.git_include))
            .click();
        harness.run();
        assert!(
            harness
                .query_by_label(&format!("{} (1)", EN.git_commit))
                .is_some()
        );
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .focus();
        harness.run();
        harness
            .input_mut()
            .events
            .push(egui::Event::Text("Post users".into()));
        harness.run();
        harness
            .get_by_label(&format!("{} (1)", EN.git_commit))
            .click();
        harness.run();
        assert_eq!(
            harness.state().commands,
            [
                GitCommand::Refresh,
                GitCommand::Diff("list.yml".into()),
                GitCommand::Commit {
                    paths: vec!["list.yml".into()],
                    message: "Post users".into(),
                },
            ]
        );
    }

    #[test]
    fn another_branch_is_picked_and_switched_to() {
        let mut harness = harness();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, EN.git_branch)
            .click();
        harness.run();
        harness.get_by_label("feature").click();
        harness.run();
        harness.get_by_label(EN.git_switch).click();
        harness.run();
        assert_eq!(
            harness.state().commands.last(),
            Some(&GitCommand::Switch("feature".into()))
        );
    }
}
