//! The response panel shows JSON and XML pretty, anything raw, binary bodies
//! as a note, and the timeline of the send.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::{ResponseHead, TimelineEvent, TimelineKind};
use apifast_gui::body::BodyHandle;
use apifast_gui::core::{Live, LiveStream, Received, ResponseState};
use apifast_gui::organisms::{
    BodyMode, PRETTY_LIMIT, ResponseTab, ResponseView, describe, millis, size_text,
};
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
    let text = |kind| describe(&TimelineEvent { at_us: 0, kind }, &EN);
    assert_eq!(
        text(TimelineKind::Dns {
            host: "api.test".into(),
            addresses: vec!["10.0.0.1".into(), "::1".into()],
            duration_us: 1_500,
            overridden: false,
        }),
        "DNS api.test → 10.0.0.1, ::1 (1.5 ms)"
    );
    assert_eq!(
        text(TimelineKind::Connected {
            address: Some("10.0.0.1:443".into()),
            duration_us: 800,
        }),
        "Connected to 10.0.0.1:443 (0.8 ms)"
    );
    assert_eq!(
        text(TimelineKind::Reused { address: None }),
        "Reused connection to ?"
    );
    assert_eq!(
        text(TimelineKind::Decompressed {
            encoding: "gzip".into(),
            wire_bytes: 2048,
        }),
        "Decompressed gzip: 2.0 KB"
    );
    let ResponseState::Received(mut compressed) = received("text/plain", vec![b'x'; 8192]) else {
        unreachable!()
    };
    assert_eq!(size_text(&compressed), "8.0 KB");
    compressed.head.timeline.push(TimelineEvent {
        at_us: 0,
        kind: TimelineKind::Decompressed {
            encoding: "br".into(),
            wire_bytes: 512,
        },
    });
    assert_eq!(size_text(&compressed), "8.0 KB (512 B br)");
    assert_eq!(
        text(TimelineKind::Tls {
            version: Some("TLSv1.3".into()),
            duration_us: 4_100,
        }),
        "TLS handshake, TLSv1.3 (4.1 ms)"
    );
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
fn the_tests_tab_shows_what_the_scripts_did() {
    use apifast_app::{
        ConsoleLevel, ConsoleLine, ScriptError, ScriptReport, ScriptRun, ScriptSource, TestOutcome,
    };
    use apifast_domain::ScriptKind;
    let ResponseState::Received(mut received) = received("application/json", "{}") else {
        unreachable!()
    };
    let run = |kind, tests: Vec<TestOutcome>, error| ScriptRun {
        console: (kind == ScriptKind::AfterResponse)
            .then(|| ConsoleLine {
                level: ConsoleLevel::Log,
                text: "user is Ana".into(),
            })
            .into_iter()
            .collect(),
        kind,
        source: ScriptSource::Request,
        tests,
        error,
    };
    received.checks.scripts = Box::new(ScriptReport {
        runs: vec![
            run(
                ScriptKind::AfterResponse,
                Vec::new(),
                Some(ScriptError::Thrown {
                    message: "Error: late".into(),
                    line: Some(2),
                }),
            ),
            run(
                ScriptKind::Tests,
                vec![
                    TestOutcome {
                        name: "is ok".into(),
                        error: None,
                    },
                    TestOutcome {
                        name: "is admin".into(),
                        error: Some("expected undefined to be true".into()),
                    },
                ],
                None,
            ),
        ],
        ..ScriptReport::default()
    });
    let mut harness = harness(ResponseState::Received(received));
    harness.get_by_label("Tests 1/2").click();
    harness.run();
    assert!(has(&harness, EN.script_tests));
    assert!(has(&harness, "expected undefined to be true"));
    assert!(has(
        &harness,
        "the request after-response script failed: Error: late at line 2"
    ));
    assert!(has(&harness, "user is Ana"));
    let top = |label: &str| harness.get_by_label(label).rect().min.y;
    assert!(top("is admin") < top("is ok"));
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

#[test]
fn the_cookies_tab_lists_what_the_response_set() {
    let ResponseState::Received(mut set) = received("text/plain", "x") else {
        unreachable!()
    };
    set.head.headers.extend([
        ("Set-Cookie".into(), "sid=abc; Path=/; HttpOnly".into()),
        ("set-cookie".into(), "theme=dark; Max-Age=60".into()),
    ]);
    let mut harness = harness(ResponseState::Received(set));
    harness.run();
    harness.get_by_label("Cookies (2)").click();
    harness.run();
    assert!(has(&harness, "sid") && has(&harness, "theme"));
    assert!(has(&harness, "HttpOnly") && has(&harness, EN.cookies_session));

    let mut empty = self::harness(self::received("text/plain", "x"));
    empty.run();
    empty.get_by_label(EN.cookies).click();
    empty.run();
    assert!(has(&empty, EN.response_no_cookies));
}

const STREAM: &str =
    "event: tick\nid: 1\ndata: {\"n\":1}\n\nevent: tick\nid: 2\ndata: {\"n\":2}\n\ndata: bye\n\n";

#[test]
fn an_event_stream_lists_its_events_and_shows_the_one_selected() {
    let mut harness = harness(received("text/event-stream", STREAM));
    harness.run();
    harness.get_by_label("Events (3)").click();
    harness.run();
    assert!(has_text(&harness, "bye"));
    assert!(has(&harness, EN.event_pick));
    harness.get_by_label_contains("{\"n\":2}").click();
    harness.run();
    // Its data, formatted.
    assert!(has_text(&harness, "{\n  \"n\": 2\n}"));

    harness.get_by_label(EN.events_filter).click();
    harness.run();
    harness.get_by_label(EN.events_filter).type_text("bye");
    harness.run();
    assert!(!has_text(&harness, "{\"n\":1}"));
    harness.get_by_label_contains("bye");
}

#[test]
fn a_stream_arriving_shows_its_events_live() {
    let live = Live {
        events: apifast_domain::parse_events(STREAM.as_bytes()),
        bytes: 99,
        ..Live::default()
    };
    let mut harness = harness(ResponseState::Streaming(LiveStream::from(live)));
    harness.run_steps(2);
    assert!(has(&harness, EN.streaming));
    harness.get_by_label_contains("bye");

    // Ended, it is a response whose events are still shown.
    harness.state_mut().response = received("text/event-stream", STREAM);
    harness.run_steps(2);
    assert_eq!(harness.state().view.tab, ResponseTab::Events);
    harness.get_by_label("Events (3)");
    harness.get_by_label_contains("bye");
}

#[test]
fn another_body_has_no_events_tab() {
    let mut harness = harness(received("text/plain", "data: x\n\n"));
    harness.run();
    assert!(!has(&harness, "Events (1)"));
}

const LOG: &str = concat!(
    "{\"at\":0,\"type\":\"sent\",\"text\":\"{\\\"op\\\":\\\"hi\\\"}\"}\n",
    "{\"at\":12,\"type\":\"received\",\"text\":\"welcome\"}\n",
    "{\"at\":30,\"type\":\"error\",\"message\":\"not sent: `missing` is not defined\"}\n",
    "{\"at\":95,\"type\":\"closed\",\"by\":\"server\",\"code\":4000,\"reason\":\"done\"}\n",
);

/// The connection whose log is [`LOG`], once closed.
fn connection() -> ResponseState {
    let ResponseState::Received(mut received) = received("text/plain", LOG) else {
        unreachable!()
    };
    received.head.status = 101;
    received.head.headers = vec![("Upgrade".into(), "websocket".into())];
    ResponseState::Received(received)
}

#[test]
fn a_websocket_connection_lists_its_messages_live_and_once_closed() {
    let live = Live {
        messages: apifast_domain::parse_ws_log(LOG.as_bytes()),
        socket: true,
        ..Live::default()
    };
    let mut harness = harness(ResponseState::Streaming(LiveStream::from(live)));
    harness.run_steps(2);
    assert!(has(&harness, EN.ws_connected));
    assert_eq!(harness.state().view.tab, ResponseTab::Messages);
    harness.get_by_label_contains("welcome");
    harness.get_by_label_contains("4000 done");

    // Closed, it is a response whose messages are still shown, with no
    // checks.
    harness.state_mut().response = connection();
    harness.run_steps(2);
    assert_eq!(harness.state().view.tab, ResponseTab::Messages);
    harness.get_by_label("Messages (4)");
    assert!(!has(&harness, EN.tests));
    harness.get_by_label_contains("{\"op\":\"hi\"}").click();
    harness.run();
    // The message, formatted.
    assert!(has_text(&harness, "{\n  \"op\": \"hi\"\n}"));

    harness.get_by_label(EN.ws_filter).click();
    harness.run();
    harness.get_by_label(EN.ws_filter).type_text("missing");
    harness.run();
    assert!(harness.query_by_label_contains("welcome").is_none());
    harness.get_by_label_contains("is not defined");
}

#[test]
fn a_grpc_call_lists_its_messages_and_how_it_ended() {
    let log = concat!(
        "{\"at\":0,\"type\":\"sent\",\"text\":\"{\\\"greeting\\\":\\\"hi\\\"}\"}\n",
        "{\"at\":4,\"type\":\"received\",\"text\":\"{\\\"reply\\\":\\\"hello\\\"}\"}\n",
        "{\"at\":5,\"type\":\"closed\",\"by\":\"server\",\"code\":0,\"reason\":\"OK\"}\n",
    );
    let mut harness = harness(received("application/grpc", log));
    harness.run_steps(2);
    assert_eq!(harness.state().view.tab, ResponseTab::Messages);
    harness.get_by_label("Messages (3)");
    harness.get_by_label_contains("{\"reply\":\"hello\"}");
    harness.get_by_label_contains("closed by the server  0 OK");
    assert!(!has(&harness, EN.tests));

    let live = Live {
        messages: apifast_domain::parse_ws_log(log.as_bytes()),
        socket: true,
        grpc: true,
        ..Live::default()
    };
    harness.state_mut().response = ResponseState::Streaming(LiveStream::from(live));
    harness.run_steps(2);
    assert!(has(&harness, EN.grpc_calling));
}
