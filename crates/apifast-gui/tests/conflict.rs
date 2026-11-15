//! A tab whose file changed on disk while it had a draft shows a warning
//! with the two ways out.

use std::sync::Arc;

use apifast_app::{CollectionTree, RequestPath};
use apifast_domain::{Collection, HttpRequest, Method};
use apifast_gui::core::{CollectionView, Command, ResponseState, Snapshot, Tab};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn snapshot(conflict: bool) -> Snapshot {
    let path = RequestPath::new("health.yml");
    let saved = HttpRequest::new("Health", Method::Get, "{{host}}/theirs");
    let draft = HttpRequest::new("Health", Method::Get, "{{host}}/mine");
    Snapshot {
        collection: Some(CollectionView {
            root: "/c".into(),
            tree: CollectionTree {
                collection: Collection::new("C"),
                items: Vec::new(),
            },
        }),
        tabs: vec![Tab {
            path: path.clone(),
            saved,
            draft: Some(draft),
            response: ResponseState::Idle,
            revision: 1,
            context: Arc::default(),
            conflict,
        }],
        active: Some(path),
        ..Snapshot::default()
    }
}

struct State {
    screen: MainScreen,
    snapshot: Snapshot,
    commands: Vec<Command>,
}

fn harness(conflict: bool) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let commands = state.screen.show(ui, &state.snapshot, &EN);
                state.commands.extend(commands);
            },
            State {
                screen: MainScreen::default(),
                snapshot: snapshot(conflict),
                commands: Vec::new(),
            },
        )
}

fn resolutions(harness: &Harness<'_, State>) -> Vec<bool> {
    (harness.state().commands.iter())
        .filter_map(|c| match c {
            Command::ResolveConflict { keep_draft, .. } => Some(*keep_draft),
            _ => None,
        })
        .collect()
}

#[test]
fn the_warning_offers_keeping_the_draft_or_loading_the_file() {
    let mut harness = harness(true);
    harness.get_by_label(EN.changed_on_disk);
    harness.get_by_label(EN.keep_mine).click();
    harness.run();
    harness.get_by_label(EN.load_from_disk).click();
    harness.run();
    assert_eq!(resolutions(&harness), [true, false]);
}

#[test]
fn without_a_conflict_there_is_no_warning() {
    let harness = harness(false);
    assert!(harness.query_by_label(EN.changed_on_disk).is_none());
}
