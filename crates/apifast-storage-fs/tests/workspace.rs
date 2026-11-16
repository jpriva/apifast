//! `workspace.yml` as Bruno writes it, and the state apifast keeps of each
//! workspace on this machine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test: a failure fails the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use apifast_app::{RequestPath, Workspace, WorkspaceEntry, WorkspaceRepository};
use apifast_storage_fs::{
    FsKnownWorkspaces, FsWorkspace, FsWorkspaceState, KnownWorkspaces, WORKSPACE_FILE,
    WorkspaceStateData,
};

const TEAM: &str = "opencollection: 1.0.0
info:
  name: \"My team\"
  type: workspace

collections:
  - name: \"Shop API\"
    path: \"collections/shop\"
  - name: \"Legacy\"
    path: \"/srv/apis/legacy\"

specs:

docs: ''
";

fn workspace(text: &str) -> (tempfile::TempDir, FsWorkspace) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_FILE), text).unwrap();
    let workspace = FsWorkspace::open(dir.path()).unwrap();
    (dir, workspace)
}

fn bruno_workspaces(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            bruno_workspaces(&path, out);
        } else if path.file_name().is_some_and(|n| n == WORKSPACE_FILE) {
            out.push(path);
        }
    }
}

#[test]
fn every_bruno_workspace_reads_and_saves_back_byte_for_byte() {
    let mut files = Vec::new();
    bruno_workspaces(Path::new("tests/corpus/bruno"), &mut files);
    assert!(files.len() >= 5, "the corpus has Bruno's workspaces");
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        let (_dir, repo) = workspace(&text);
        let read = repo.workspace().unwrap();
        assert!(!read.name.is_empty(), "{}", file.display());
        repo.save_workspace(&read).unwrap();
        assert_eq!(
            fs::read_to_string(repo.folder().join(WORKSPACE_FILE)).unwrap(),
            text,
            "{}",
            file.display()
        );
        // Adding a collection and removing it again gives the file back.
        let mut more = read.clone();
        more.collections.push(WorkspaceEntry {
            name: "Extra".into(),
            path: "extra".into(),
        });
        repo.save_workspace(&more).unwrap();
        assert_eq!(repo.workspace().unwrap(), more);
        repo.save_workspace(&read).unwrap();
        assert_eq!(repo.workspace().unwrap(), read, "{}", file.display());
    }
}

#[test]
fn collections_are_read_added_and_removed_in_place() {
    let (_dir, repo) = workspace(TEAM);
    let mut team = repo.workspace().unwrap();
    assert_eq!(team.name, "My team");
    assert_eq!(
        team.collections,
        [
            WorkspaceEntry {
                name: "Shop API".into(),
                path: "collections/shop".into()
            },
            WorkspaceEntry {
                name: "Legacy".into(),
                path: "/srv/apis/legacy".into()
            },
        ]
    );
    team.collections.remove(1);
    team.collections.push(WorkspaceEntry {
        name: "Billing".into(),
        path: "billing".into(),
    });
    team.name = "Our team".into();
    repo.save_workspace(&team).unwrap();
    let after = fs::read_to_string(repo.folder().join(WORKSPACE_FILE)).unwrap();
    let expected = TEAM
        .replace("name: \"My team\"", "name: \"Our team\"")
        .replace(
            "  - name: \"Legacy\"\n    path: \"/srv/apis/legacy\"\n",
            // In the file's own quoting style.
            "  - name: \"Billing\"\n    path: \"billing\"\n",
        );
    assert_eq!(after, expected);
}

#[test]
fn paths_are_relative_inside_the_workspace_and_absolute_outside() {
    let (dir, repo) = workspace(TEAM);
    let inside = dir.path().join("collections/shop");
    fs::create_dir_all(&inside).unwrap();
    let entry = repo.entry_for(&inside, "Shop API");
    assert_eq!(entry.path, "collections/shop");
    assert_eq!(
        repo.resolve(&entry).canonicalize().unwrap(),
        inside.canonicalize().unwrap()
    );

    let elsewhere = tempfile::tempdir().unwrap();
    let entry = repo.entry_for(elsewhere.path(), "Elsewhere");
    assert!(Path::new(&entry.path).is_absolute(), "{}", entry.path);
    assert_eq!(
        repo.resolve(&entry).canonicalize().unwrap(),
        elsewhere.path().canonicalize().unwrap()
    );
}

#[test]
fn a_workspace_is_created_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("team");
    let repo = FsWorkspace::create(&folder, "Team").unwrap();
    assert_eq!(
        repo.workspace().unwrap(),
        Workspace {
            name: "Team".into(),
            collections: Vec::new()
        }
    );
    let text = fs::read_to_string(folder.join(WORKSPACE_FILE)).unwrap();
    assert!(text.contains("type: workspace"), "{text}");
    assert!(FsWorkspace::create(&folder, "Again").is_err());
    assert_eq!(
        FsWorkspace::open(&folder)
            .unwrap()
            .workspace()
            .unwrap()
            .name,
        "Team"
    );
    assert!(
        FsWorkspace::open(dir.path()).is_err(),
        "no workspace.yml there"
    );
}

#[test]
fn personal_state_and_the_known_list_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let state = FsWorkspaceState::new(dir.path().join("w/state.yml"));
    assert_eq!(state.load().unwrap(), WorkspaceStateData::default());
    let data = WorkspaceStateData {
        expanded: vec![PathBuf::from("/a/shop")],
        tabs: vec![
            (PathBuf::from("/a/shop"), RequestPath::new("users/get.yml")),
            (PathBuf::from("/b/pay"), RequestPath::new("charge.yml")),
        ],
        active: Some((PathBuf::from("/b/pay"), RequestPath::new("charge.yml"))),
    };
    state.save(&data).unwrap();
    assert_eq!(state.load().unwrap(), data);

    let known = FsKnownWorkspaces::new(dir.path().join("workspaces.yml"));
    assert_eq!(known.load().unwrap(), KnownWorkspaces::default());
    let list = KnownWorkspaces {
        known: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        active: Some(PathBuf::from("/b")),
    };
    known.save(&list).unwrap();
    assert_eq!(known.load().unwrap(), list);

    // Odd entries are skipped, not errors.
    fs::write(
        dir.path().join("w/state.yml"),
        "tabs:\n  - nope\n  - collection: /a\n    request: r.yml\nexpanded: 3\n",
    )
    .unwrap();
    let odd = state.load().unwrap();
    assert_eq!(odd.tabs, [(PathBuf::from("/a"), RequestPath::new("r.yml"))]);
    assert_eq!(odd.expanded, Vec::<PathBuf>::new());
}
