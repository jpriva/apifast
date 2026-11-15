//! The body viewer reads bodies of any size by windows: it indexes them
//! over several frames, draws only the visible rows and numbers lines.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use apifast_gui::body::BodyHandle;
use apifast_gui::organisms::BodyViewer;
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    viewer: BodyViewer,
    body: BodyHandle,
}

fn harness(body: BodyHandle) -> Harness<'static, State> {
    // One step per `run`, so each frame's share of the indexing shows.
    Harness::builder()
        .with_size(egui::vec2(800.0, 400.0))
        .with_max_steps(1)
        .build_ui_state(
            |ui, state: &mut State| state.viewer.show(ui, &state.body, &EN),
            State {
                viewer: BodyViewer::default(),
                body,
            },
        )
}

/// `lines` lines, `line 1` to `line N`.
fn numbered(lines: usize) -> Vec<u8> {
    let mut text = String::new();
    for n in 1..=lines {
        writeln!(text, "line {n}").unwrap();
    }
    text.into_bytes()
}

/// Labels of the given text: rendered rows show up as labels.
fn has(harness: &Harness<'_, State>, text: &str) -> bool {
    harness.query_by_label(text).is_some()
}

#[test]
fn a_large_body_is_indexed_over_frames_and_viewable_meanwhile() {
    // About 45 MB: more than the budget of the frames run so far.
    let mut harness = harness(BodyHandle::memory("big", numbered(3_000_000)));
    harness.step();
    assert!(has(&harness, "line 1"));
    let state = harness.state();
    assert!(!state.viewer.indexed(&state.body));
    assert!(harness.query_by_label_contains(EN.indexing).is_some());

    harness.run_steps(6);
    let state = harness.state();
    assert!(state.viewer.indexed(&state.body));
    // The lines and the empty one after the last newline.
    assert_eq!(state.viewer.rows(), 3_000_001);
    assert!(harness.query_by_label_contains(EN.indexing).is_none());
    // Only what fits on screen is drawn.
    assert!(!has(&harness, "line 500"));
}

#[test]
fn scrolling_far_shows_those_rows_and_their_numbers() {
    let mut harness = harness(BodyHandle::memory("big", numbered(100_000)));
    harness.run_steps(3);
    harness.state_mut().viewer.scroll_to_row(76_543);
    harness.run_steps(2);
    assert!(has(&harness, "line 76544"), "the row after it is visible");
    // Right-aligned to the width of the largest number, `100001`.
    assert!(has(&harness, " 76544"), "with its line number");
    assert!(!has(&harness, "line 1"));
}

#[test]
fn a_minified_line_is_cut_into_rows_numbered_once() {
    let line = format!("[{}]", "1,".repeat(10_000));
    let mut harness = harness(BodyHandle::memory("min", line));
    harness.run_steps(2);
    // 20 002 bytes in rows of 2 KB: ten rows, numbered two wide.
    assert_eq!(harness.state().viewer.rows(), 10);
    assert!(has(&harness, " 1"), "the first row has the line number");
    assert!(!has(&harness, " 2"), "the rest continue line 1");
}

#[test]
fn a_new_body_starts_from_its_top() {
    let mut harness = harness(BodyHandle::memory("a", numbered(10)));
    harness.run_steps(2);
    harness.state_mut().body = BodyHandle::memory("b", "other\n");
    harness.run_steps(2);
    assert!(has(&harness, "other"));
    assert!(!has(&harness, "line 1"));
}

/// Time per frame while scrolling a 100 MB body, once indexed. Run with
/// `cargo test --release -p apifast-gui --test body_viewer -- --ignored
/// --nocapture`; the numbers go to `docs/10-rendimiento-y-memoria.md`.
#[test]
#[ignore = "measurement, run on demand in release"]
fn frame_time_with_a_100_mb_body() {
    let lines = 100 * 1024 * 1024 / 14;
    let started = Instant::now();
    let mut harness = harness(BodyHandle::memory("huge", numbered(lines)));
    harness.step();
    let mut frames = 1;
    while !{
        let s = harness.state();
        s.viewer.indexed(&s.body)
    } {
        harness.step();
        frames += 1;
    }
    println!(
        "indexed {} rows in {frames} frames, {:?}",
        harness.state().viewer.rows(),
        started.elapsed()
    );
    let mut times: Vec<Duration> = Vec::new();
    for i in 0..200 {
        harness.state_mut().viewer.scroll_to_row(i * 37_000);
        let start = Instant::now();
        harness.step();
        times.push(start.elapsed());
    }
    times.sort();
    println!(
        "scrolling: median {:?}, p95 {:?}, max {:?}",
        times[100], times[190], times[199]
    );
}
