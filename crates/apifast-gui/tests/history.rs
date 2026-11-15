//! The history panel lists sends newest first, filters them and reports the
//! one picked.

use apifast_app::{HistoryEntry, RequestPath, ResponseHead, ResponseId};
use apifast_gui::organisms::{HistoryView, ago, matches};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const NOW: i64 = 10_000_000;

fn entries() -> Vec<HistoryEntry> {
    let head = ResponseHead {
        status: 201,
        version: "HTTP/1.1".into(),
        url: "http://h/users".into(),
        redirects: 0,
        headers: Vec::new(),
        elapsed_ms: 35,
        timeline: Vec::new(),
    };
    vec![
        HistoryEntry::failed(
            NOW - 3 * 3_600_000,
            RequestPath::new("health.yml"),
            ("GET".into(), "{{host}}/health".into()),
            "cannot connect".into(),
        ),
        HistoryEntry::received(
            NOW - 90_000,
            RequestPath::new("users/create.yml"),
            ("POST".into(), "{{host}}/users".into()),
            head,
            (2, ResponseId("r".into())),
            false,
        ),
    ]
}

struct State {
    view: HistoryView,
    entries: Vec<HistoryEntry>,
    picked: Vec<usize>,
}

fn harness() -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, state: &mut State| {
                if let Some(index) = state.view.show(ui, &state.entries, NOW, &EN) {
                    state.picked.push(index);
                }
            },
            State {
                view: HistoryView::default(),
                entries: entries(),
                picked: Vec::new(),
            },
        )
}

#[test]
fn entries_show_newest_first_with_their_outcome() {
    let harness = harness();
    let newest = harness.get_by_label("users/create.yml").rect().min.y;
    let oldest = harness.get_by_label("health.yml").rect().min.y;
    assert!(newest < oldest, "the newest is on top");
    harness.get_by_label("201 Created");
    harness.get_by_label(EN.failed);
    harness.get_by_label("1 min ago, 35 ms");
    harness.get_by_label("3 h ago");
}

#[test]
fn picking_an_entry_reports_its_index() {
    let mut harness = harness();
    harness.get_by_label("health.yml").click();
    harness.run();
    assert_eq!(harness.state().picked, [0]);
}

#[test]
fn the_filter_matches_the_request_method_or_url() {
    let mut harness = harness();
    harness.state_mut().view.filter = "post".into();
    harness.run();
    assert!(harness.query_by_label("health.yml").is_none());
    harness.get_by_label("users/create.yml");

    let [failed, sent] = entries().try_into().ok().unwrap_or_else(|| unreachable!());
    assert!(matches(&failed, "HEALTH"));
    assert!(matches(&sent, "{{host}}/us"));
    assert!(!matches(&sent, "delete"));
    assert!(matches(&sent, "  "));
}

#[test]
fn times_read_as_how_long_ago() {
    assert_eq!(ago(NOW, NOW - 5_000, &EN), "5 s ago");
    assert_eq!(ago(NOW, NOW - 61_000, &EN), "1 min ago");
    assert_eq!(ago(NOW, NOW - 7_200_000, &EN), "2 h ago");
    assert_eq!(ago(NOW, NOW - 3 * 86_400_000, &EN), "3 d ago");
    // A clock that went back is "now", not a negative time.
    assert_eq!(ago(NOW, NOW + 5_000, &EN), "0 s ago");
}

#[test]
fn an_empty_history_says_so() {
    let mut harness = harness();
    harness.state_mut().entries.clear();
    harness.run();
    harness.get_by_label(EN.history_empty);
}
