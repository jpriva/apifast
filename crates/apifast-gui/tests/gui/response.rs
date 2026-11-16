//! The response panel shows JSON and XML pretty, anything raw, binary bodies
//! as a note, and the timeline of the send.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::{ResponseHead, TimelineEvent, TimelineKind};
use apifast_gui::body::BodyHandle;
use apifast_gui::core::{Received, ResponseState};
use apifast_gui::organisms::{BodyMode, PRETTY_LIMIT, ResponseTab, ResponseView, describe, millis};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    view: ResponseView,
    response: ResponseState,
}

fn received(content_type: &str, body: impl Into<Vec<u8>>) -> ResponseState {
    let body: Vec<u8> = body.into();
    ResponseState::Received(Received {
        head: ResponseHead {
            status: 200,
            version: "HTTP/1.1".into(),
            url: "http://localhost/x".into(),
            redirects: 1,
            headers: vec![("Content-Type".into(), content_type.into())],
            elapsed_ms: 20,
            timeline: vec![
                TimelineEvent {
                    at_us: 0,
                    kind: TimelineKind::Request {
                        method: "GET".into(),
                        url: "http://localhost/a".into(),
                    },
                },
                TimelineEvent {
                    at_us: 4_100,
                    kind: TimelineKind::Redirect {
                        status: 302,
                        location: "/x".into(),
                    },
                },
                TimelineEvent {
                    at_us: 11_250,
                    kind: TimelineKind::Headers { status: 200 },
                },
                TimelineEvent {
                    at_us: 20_000,
                    kind: TimelineKind::Body { bytes: 2048 },
                },
            ],
        },
        size: body.len() as u64,
        body: BodyHandle::memory(format!("r{}", body.len()), body),
        checks: apifast_gui::core::Checks::default(),
    })
}

fn harness(response: ResponseState) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 500.0))
        .build_ui_state(
            |ui, state: &mut State| state.view.show(ui, &state.response, &EN),
            State {
                view: ResponseView::default(),
                response,
            },
        )
}

fn has(harness: &Harness<'_, State>, text: &str) -> bool {
    harness.query_by_label(text).is_some()
}

#[test]
fn json_is_shown_pretty_and_can_be_seen_raw() {
    let mut harness = harness(received("application/json", r#"{"a":1,"b":[true]}"#));
    harness.run();
    assert!(has(&harness, r#"  "a": 1,"#), "formatted, one key per line");
    assert!(!has(&harness, r#"{"a":1,"b":[true]}"#));

    harness.get_by_label(EN.raw).click();
    harness.run();
    assert_eq!(harness.state().view.mode, BodyMode::Raw);
    assert!(has(&harness, r#"{"a":1,"b":[true]}"#));
}

#[test]
fn a_body_that_cannot_be_formatted_is_shown_as_received() {
    let mut harness = harness(received("text/html", "<p>one<br>two</p>"));
    harness.run();
    assert!(has(&harness, EN.not_formatted));
    assert!(has(&harness, "<p>one<br>two</p>"));
}

#[test]
fn plain_text_has_no_pretty_view() {
    let harness = harness(received("text/plain", "hello"));
    assert!(!has(&harness, EN.pretty));
    assert!(has(&harness, "hello"));
}

#[test]
fn a_binary_body_is_not_drawn() {
    let harness = harness(received(
        "image/png",
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec(),
    ));
    assert!(has(&harness, &format!("{} 16 B", EN.body_binary)));
}

#[test]
fn a_large_json_body_is_shown_raw() {
    let big = format!(
        "[{}1]",
        "1,".repeat(usize::try_from(PRETTY_LIMIT).unwrap() / 2)
    );
    let mut harness = harness(received("application/json", big));
    harness.run();
    assert!(has(&harness, EN.pretty_off));
    assert!(!has(&harness, EN.pretty));
}

#[test]
fn the_copy_button_copies_the_body() {
    let mut harness = harness(received("text/plain", "copy me"));
    harness.get_by_label(EN.copy).click();
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "copy me"));
    assert!(copied);
}

#[test]
fn the_timeline_lists_each_event_with_its_time() {
    let mut harness = harness(received("text/plain", "x"));
    harness.get_by_label(EN.timeline).click();
    harness.run();
    assert_eq!(harness.state().view.tab, ResponseTab::Timeline);
    for label in [
        "0.0 ms",
        "GET http://localhost/a",
        "4.1 ms",
        "Redirect 302: /x",
        "11.2 ms",
        "Headers received: 200 OK",
        "20.0 ms",
        "Body received: 2.0 KB",
    ] {
        assert!(has(&harness, label), "{label}");
    }
}

#[test]
fn times_and_events_read_well() {
    assert_eq!(millis(1_234_567), "1234.5 ms");
    let event = TimelineEvent {
        at_us: 0,
        kind: TimelineKind::Body { bytes: 0 },
    };
    assert_eq!(describe(&event, &EN), "Body received: 0 B");
}

#[test]
fn the_pretty_limit_comes_from_the_settings() {
    let mut harness = harness(received("application/json", r#"{"a":1}"#));
    harness.state_mut().view.pretty_limit = Some(3);
    harness.run();
    assert!(has(&harness, EN.pretty_off));
    harness.state_mut().view.pretty_limit = None;
    harness.run();
    assert!(has(&harness, EN.pretty));
}

/// Masked secrets show as dots, which the interface's fonts must have.
#[test]
fn a_masked_secret_in_the_timeline_is_drawn() {
    let ResponseState::Received(mut received) = received("text/plain", "x") else {
        unreachable!()
    };
    let url = format!("http://localhost/users/{}", apifast_domain::MASK);
    received.head.timeline[0].kind = TimelineKind::Request {
        method: "GET".into(),
        url: url.clone(),
    };
    let mut harness = harness(ResponseState::Received(received));
    harness.get_by_label(EN.timeline).click();
    harness.run();
    assert!(has(&harness, &format!("GET {url}")));
}

#[test]
fn the_tests_tab_shows_failures_first_and_masks_secret_captures() {
    use apifast_app::{AssertionOutcome, CaptureOutcome, Verdict};
    use apifast_domain::{Assertion, CaptureScope, Operator, VariableValue};
    let ResponseState::Received(mut received) = received("application/json", "{}") else {
        unreachable!()
    };
    let outcome = |expression: &str, passed: bool| AssertionOutcome {
        assertion: Assertion::new(expression, Operator::parse("eq"), Some("200")),
        expected: Some("200".into()),
        verdict: Verdict {
            passed,
            actual: "404".into(),
            message: (!passed).then(|| "expected eq 200, got 404".into()),
        },
    };
    received.checks.assertions = vec![outcome("res.size", true), outcome("res.status", false)];
    received.checks.captures = vec![
        CaptureOutcome {
            variable: "token".into(),
            scope: CaptureScope::Runtime,
            value: Ok(VariableValue::text("s3cret")),
            secret: true,
        },
        CaptureOutcome {
            variable: "id".into(),
            scope: CaptureScope::Runtime,
            value: Err("res.body.id is undefined".into()),
            secret: false,
        },
    ];
    let mut harness = harness(ResponseState::Received(received));
    harness.get_by_label("Tests 1/2").click();
    harness.run();
    assert!(has(&harness, "res.status eq 200"));
    assert!(has(&harness, "expected eq 200, got 404"));
    assert!(has(&harness, apifast_app::HIDDEN));
    assert!(!has(&harness, "s3cret"));
    assert!(has(&harness, "res.body.id is undefined"));
    // Failures first.
    let top = |label: &str| harness.get_by_label(label).rect().min.y;
    assert!(top("res.status eq 200") < top("res.size eq 200"));
}

#[test]
fn the_tests_tab_says_when_nothing_ran() {
    let mut harness = harness(received("application/json", "{}"));
    harness.get_by_label(EN.tests).click();
    harness.run();
    assert!(has(&harness, EN.tests_none));
}

#[test]
fn the_filter_selects_with_jsonpath_or_xpath_and_reports_errors() {
    let mut harness = harness(received(
        "application/json",
        r#"{"items":[{"id":7,"name":"Ana"},{"id":9,"name":"Bo"}]}"#,
    ));
    harness.run();
    harness.state_mut().view.filter = "$.items[*].id".into();
    harness.run();
    assert!(has(&harness, "2 selected"));
    assert!(has_text(&harness, "7"));
    assert!(!has_text(&harness, "Ana"));

    harness.state_mut().view.filter = "$.items[?@.id > 8].name".into();
    harness.run();
    assert!(has(&harness, "1 selected"));
    assert!(has_text(&harness, "\"Bo\""));

    harness.state_mut().view.filter = "$.items[".into();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("invalid JSONPath")
            .is_some()
    );

    // Empty: the whole body again.
    harness.state_mut().view.filter.clear();
    harness.run();
    assert!(has_text(&harness, "\"Ana\""));

    let mut harness = harness_with_xml();
    harness.state_mut().view.filter = "//item/@id".into();
    harness.run();
    assert!(has(&harness, "2 selected"));
    assert!(has_text(&harness, "a1"));
}

fn harness_with_xml() -> Harness<'static, State> {
    let mut harness = harness(received(
        "application/xml",
        "<list><item id=\"a1\">x</item><item id=\"b2\">y</item></list>",
    ));
    harness.run();
    harness
}

/// Whether some row of the viewer contains `text`.
fn has_text(harness: &Harness<'_, State>, text: &str) -> bool {
    harness.query_by_label_contains(text).is_some()
}
