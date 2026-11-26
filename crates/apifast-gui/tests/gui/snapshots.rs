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

use apifast_domain::{Body, Header, HttpRequest, Method, Param, ParamKind};
use apifast_gui::atoms::{VariableHint, VariableHints};
use apifast_gui::core::{CollectionEntry, TabContext};
use apifast_gui::organisms::{RequestPanel, RequestTab, TreeView};
use apifast_gui::strings::EN;
use apifast_gui::theme;
use eframe::egui;
use egui_kittest::{Harness, SnapshotResults};

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

fn request_panel(tab: RequestTab, name: &str) {
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
        harness.snapshot(format!("{name}_{suffix}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_headers_table() {
    request_panel(RequestTab::Headers, "headers");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_params_table() {
    request_panel(RequestTab::Params, "params");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_json_body() {
    request_panel(RequestTab::Body, "body");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "drawn only on Linux")]
fn the_collection_tree() {
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
