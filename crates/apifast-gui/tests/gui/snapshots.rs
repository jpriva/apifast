//! Screenshot tests: parts of the interface drawn with `wgpu` and compared
//! with the images in `tests/snapshots`. They catch what the accessibility
//! tree does not show, such as a misaligned badge, a cut button or a glyph
//! missing from the fonts (which makes epaint panic only when drawing).
//!
//! The images are drawn on Linux with lavapipe, Mesa's Vulkan on the CPU,
//! which `egui_kittest` prefers when it is installed; other drivers and
//! systems draw some pixels differently, so the tests run only on Linux.
//! After a change meant to alter the look, `UPDATE_SNAPSHOTS=1 cargo test`
//! rewrites the images; check them before committing.

#![cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "the tests run only on Linux")
)]

use std::sync::{Mutex, MutexGuard, PoisonError};

use apifast_domain::{Body, Header, HttpRequest, Method, Param, ParamKind};
use apifast_gui::atoms::{VariableHint, VariableHints};
use apifast_gui::core::{CollectionEntry, TabContext};
use apifast_gui::organisms::{RequestPanel, RequestTab, TreeView};
use apifast_gui::strings::EN;
use apifast_gui::theme;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, SnapshotResults};

/// Lets one screenshot test draw at a time, for as long as it holds it.
///
/// Lavapipe crashed (SIGSEGV) in CI on Ubuntu when several tests drew with
/// wgpu at once; one device at a time costs little, the tests take under
/// two seconds together.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static GPU: Mutex<()> = Mutex::new(());
    // A test that failed while holding it leaves it poisoned, not unusable.
    GPU.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Builds a harness that draws `content` in `theme`, on a panel's
/// background, with apifast's style installed before the first frame.
fn harness(
    size: egui::Vec2,
    theme: egui::Theme,
    mut content: impl FnMut(&mut egui::Ui) + 'static,
) -> Harness<'static> {
    let mut first = true;
    let mut harness = Harness::builder()
        .with_size(size)
        .wgpu()
        .build_ui(move |ui| {
            if first {
                theme::install(ui.ctx());
                ui.ctx().set_theme(theme);
                first = false;
            }
            egui::Frame::central_panel(ui.style()).show(ui, |ui| content(ui));
        });
    // The first frame installs the style; the next ones draw with it.
    harness.run();
    harness
}

fn themes() -> [(egui::Theme, &'static str); 2] {
    [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")]
}

fn request() -> HttpRequest {
    let mut request = HttpRequest::new("Users", Method::Post, "{{host}}/users");
    request.headers = vec![
        Header::new("Content-Type", "application/json"),
        Header::new("Authorization", "Bearer {{token}}"),
        Header::new("X-Trace", "{{missing}}"),
    ];
    request.params = vec![
        Param {
            name: "page".into(),
            value: "1".into(),
            kind: ParamKind::Query,
            enabled: true,
        },
        Param {
            name: "limit".into(),
            value: "20".into(),
            kind: ParamKind::Query,
            enabled: false,
        },
    ];
    request.body = Body::Json("{\n  \"name\": \"{{user}}\",\n  \"age\": 3\n}".into());
    request
}

fn request_panel(tab: RequestTab, name: &str, before: fn(&mut Harness<'static>)) {
    // One result for both themes: a harness dropped with its own fails.
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let mut request = request();
        let mut panel = RequestPanel {
            tab,
            ..RequestPanel::default()
        };
        let context = TabContext::default();
        let mut hints = VariableHints::new();
        hints.insert(
            "token".into(),
            VariableHint {
                value: "secret".into(),
                origin: "environment".into(),
            },
        );
        let mut harness = harness(egui::vec2(720.0, 260.0), theme, move |ui| {
            panel.show(ui, &mut request, &context, &hints, &EN);
        });
        before(&mut harness);
        harness.snapshot(format!("{name}_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_headers_table() {
    let _gpu = one_at_a_time();
    request_panel(RequestTab::Headers, "headers", |_| {});
}

/// A hovered row shows its remove button; the field with the focus is
/// outlined.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn a_hovered_row_and_a_focused_cell() {
    let _gpu = one_at_a_time();
    request_panel(RequestTab::Headers, "headers_hover", |harness| {
        harness.get_all_by_label(EN.value).nth(2).unwrap().focus();
        harness.run();
        let row = harness.get_all_by_label(EN.name).nth(1).unwrap().rect();
        harness.hover_at(row.center());
        harness.run();
    });
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_params_table() {
    let _gpu = one_at_a_time();
    request_panel(RequestTab::Params, "params", |_| {});
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_json_body() {
    let _gpu = one_at_a_time();
    request_panel(RequestTab::Body, "body", |_| {});
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_collection_tree() {
    let _gpu = one_at_a_time();
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let collections = [CollectionEntry::loaded("/c", crate::organisms::tree())];
        let mut view = TreeView::default();
        let mut harness = harness(egui::vec2(280.0, 220.0), theme, move |ui| {
            view.show(ui, &collections, 1, (None, None), &EN);
        });
        harness.snapshot(format!("tree_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// A window inside apifast's: its own title bar, with the title and a
/// drawn ×, over a line.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn a_window_with_its_own_title_bar() {
    let _gpu = one_at_a_time();
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let mut open = true;
        let mut harness = harness(egui::vec2(420.0, 200.0), theme, move |ui| {
            let window = egui::Window::new("Collection settings · API")
                .id(egui::Id::unique("snapshot-window"))
                .fixed_pos(egui::pos2(20.0, 20.0))
                .default_width(360.0);
            let title = ("Collection settings · API", EN.close_window);
            apifast_gui::molecules::window(ui.ctx(), window, title, &mut open, |ui| {
                ui.label("Proxy");
                ui.checkbox(&mut true, "Verify certificates");
            });
            // The page behind the window, painted to the edges.
            ui.allocate_space(ui.available_size());
        });
        harness.snapshot(format!("window_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// A response with its earlier responses listed, and the request as it was
/// sent.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_request_sent_and_the_earlier_responses() {
    use apifast_app::{HistoryEntry, RequestPath, ResponseHead, ResponseId, SentBody, SentRequest};
    use apifast_gui::body::BodyHandle;
    use apifast_gui::core::{Checks, Received, ResponseState};
    use apifast_gui::organisms::{HistoryOf, ResponseTab, ResponseView};
    use std::sync::Arc;

    let _gpu = one_at_a_time();
    let head = |status: u16, elapsed_ms: u64| ResponseHead {
        status,
        version: "HTTP/1.1".into(),
        url: "http://localhost:8080/users".into(),
        redirects: 0,
        headers: vec![("content-type".into(), "application/json".into())],
        elapsed_ms,
        timeline: Vec::new(),
    };
    let sent = SentRequest {
        method: "POST".into(),
        url: "http://localhost:8080/users".into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("authorization".into(), "[hidden]".into()),
        ],
        body: SentBody::Text {
            text: "{\n  \"name\": \"Ana\"\n}".into(),
            size: 19,
        },
    };
    let path = RequestPath::new("users/create.yml");
    // Fixed times, far in the past: "how long ago" grows by days only.
    let entry = |at_ms: i64, status: u16, id: &str, name: Option<&str>| {
        let mut entry = HistoryEntry::received(
            at_ms,
            path.clone(),
            ("POST".into(), "{{host}}/users".into()),
            head(status, 40),
            (19, ResponseId(id.into())),
            &[],
        );
        entry.name = name.map(str::to_owned);
        entry
    };
    let history = Arc::new(vec![
        entry(0, 500, "a", None),
        entry(1_000, 201, "b", Some("Before the fix")),
        entry(2_000, 201, "c", None),
    ]);
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let response = ResponseState::Received(Received {
            head: head(201, 42),
            size: 19,
            body: BodyHandle::memory("r", b"{\"id\":7}".to_vec()),
            checks: Checks::default(),
            sent: Some(Arc::new(sent.clone())),
        });
        let mut view = ResponseView::default();
        view.tab = ResponseTab::Request;
        view.history = Some(HistoryOf {
            entries: history.clone(),
            path: path.clone(),
            now_ms: 3 * 3_600_000,
        });
        let mut harness = harness(egui::vec2(720.0, 360.0), theme, move |ui| {
            view.show(ui, &response, &EN);
        });
        let label = format!("{} (3)", EN.response_history);
        harness.get_by_label(&label).click();
        harness.run();
        harness.snapshot(format!("response_history_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// The main screen with a request open: its panes side by side, as in
/// Bruno, and one under the other after Ctrl+J.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_editor_panes_side_by_side_and_stacked() {
    use apifast_gui::core::{ResponseState, Snapshot, Tab};
    use apifast_gui::screens::MainScreen;
    use std::sync::Arc;

    let _gpu = one_at_a_time();
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let saved = request();
        let tab = Tab {
            root: "/c".into(),
            path: apifast_app::RequestPath::new("users/create.yml"),
            draft: None,
            saved,
            response: ResponseState::Idle,
            revision: 1,
            context: Arc::default(),
            conflict: false,
        };
        let snapshot = Snapshot {
            active: Some(tab.key()),
            tabs: vec![tab],
            ..Snapshot::default()
        };
        let mut screen = MainScreen::default();
        screen.settings.own_title_bar = false;
        let mut first = true;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 520.0))
            .wgpu()
            .build_ui_state(
                move |ui, screen: &mut MainScreen| {
                    if first {
                        theme::install(ui.ctx());
                        ui.ctx().set_theme(theme);
                        first = false;
                    }
                    let _ = screen.show(ui, &snapshot, &EN);
                },
                screen,
            );
        harness.run();
        harness.snapshot(format!("main_side_by_side_{suffix}"));
        harness.state_mut().layout.toggle();
        harness.run();
        harness.snapshot(format!("main_stacked_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// The Layouts window, with two layouts saved.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_layouts_window() {
    use apifast_gui::organisms::LayoutsWindow;

    let _gpu = one_at_a_time();
    let mut results = SnapshotResults::new();
    for (theme, suffix) in themes() {
        let mut window = LayoutsWindow::default();
        window.open = true;
        let names = vec!["Bruno".to_owned(), "Two monitors".to_owned()];
        let mut harness = harness(egui::vec2(520.0, 300.0), theme, move |ui| {
            let _ = window.show(ui.ctx(), &names, &EN);
            ui.allocate_space(ui.available_size());
        });
        harness.snapshot(format!("layouts_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}
