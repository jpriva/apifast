//! The runner window: the form becomes a run event, and a run shows its
//! progress, totals and steps, each step opening its request.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::Arc;

use apifast_app::{
    RequestPath, ResponseHead, ResponseId, RunReport, SendOutcome, StepResult, StoredBody,
};
use apifast_gui::core::{RunSettings, RunView};
use apifast_gui::organisms::{RunnerEvent, RunnerWindow};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    window: RunnerWindow,
    run: Option<RunView>,
    events: Vec<RunnerEvent>,
}

fn harness(run: Option<RunView>) -> Harness<'static, State> {
    let mut window = RunnerWindow::default();
    window.open_for("/shop".into(), Some("users".into()), "Users".into());
    Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let ctx = ui.ctx().clone();
                if let Some(event) = state.window.show(&ctx, state.run.as_ref(), &EN) {
                    state.events.push(event);
                }
            },
            State {
                window,
                run,
                events: Vec::new(),
            },
        )
}

fn step(path: &str, status: u16) -> StepResult {
    StepResult {
        iteration: 1,
        path: RequestPath::new(path),
        name: path.trim_end_matches(".yml").to_owned(),
        outcome: Ok(SendOutcome {
            scripts: apifast_app::ScriptReport::default(),
            head: ResponseHead {
                status,
                version: "HTTP/1.1".into(),
                url: String::new(),
                redirects: 0,
                headers: Vec::new(),
                elapsed_ms: 3,
                timeline: Vec::new(),
            },
            body: StoredBody {
                id: ResponseId("r".into()),
                size: 0,
            },
            problems: Vec::new(),
            contains_secret: false,
            secrets: Vec::new(),
            captures: Vec::new(),
            assertions: Vec::new(),
            dependencies: Vec::new(),
        }),
        duration_ms: 3,
    }
}

fn view(running: bool) -> RunView {
    let steps = vec![step("users/list.yml", 200)];
    RunView {
        root: "/shop".into(),
        collection: "Shop".into(),
        settings: RunSettings::default(),
        environment: None,
        started_ms: 0,
        running,
        total: 2,
        iterations: 1,
        current: running.then(|| (1, 1, RequestPath::new("users/get.yml"))),
        steps: Arc::new(steps.clone()),
        report: (!running).then(|| {
            Arc::new(RunReport {
                steps,
                skipped: Vec::new(),
                duration_ms: 3,
                bailed: false,
                cancelled: false,
                stopped: false,
                runtime: Vec::new(),
                script_changes: Vec::new(),
            })
        }),
        error: None,
    }
}

#[test]
fn the_form_starts_a_run_of_the_folder_with_its_tags() {
    let mut harness = harness(None);
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::TextInput, EN.run_tags)
        .focus();
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("smoke, auth".into()));
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, EN.run_bail)
        .click();
    harness.run();
    harness.get_by_label(EN.run_start).click();
    harness.run();
    let RunnerEvent::Run { root, settings } = harness.state().events.last().unwrap() else {
        panic!("{:?}", harness.state().events);
    };
    assert_eq!(root.to_str(), Some("/shop"));
    assert_eq!(settings.folder.as_deref(), Some("users"));
    assert_eq!(settings.tags, ["smoke", "auth"]);
    assert!(settings.bail && settings.recursive);
}

#[test]
fn a_run_in_progress_can_be_stopped_and_its_steps_opened() {
    let mut harness = harness(Some(view(true)));
    harness.run();
    assert!(
        harness.query_by_label(EN.run_start).is_none(),
        "no form while running"
    );
    assert!(harness.query_by_label("1 / 2").is_some());
    assert!(
        harness.query_by_label("users/get.yml").is_some(),
        "the current request"
    );
    harness.get_by_label(EN.run_stop).click();
    harness.run();
    harness.get_by_label("users/list").click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [RunnerEvent::Stop, RunnerEvent::OpenStep(0)]
    );
}

#[test]
fn a_finished_run_shows_its_totals() {
    let mut harness = harness(Some(view(false)));
    harness.run();
    assert!(
        harness
            .query_by_label_contains("1 passed · 0 failed · 0 errors · 0 skipped")
            .is_some()
    );
    harness.get_by_label(EN.run_clear).click();
    harness.run();
    assert_eq!(harness.state().events, [RunnerEvent::Close]);
}
