//! Atoms through the accessibility tree, as a screen reader would see them.
//! No GPU is needed: nothing is rendered to pixels.

use apifast_domain::Method;
use apifast_gui::{atoms, theme};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn method_badges_show_the_method_as_text() {
    let methods = [Method::Get, Method::Delete, Method::Custom("PURGE".into())];
    let harness = Harness::new_ui(|ui| {
        for method in &methods {
            ui.add(atoms::MethodBadge::new(method));
        }
    });
    // The color is never the only signal: the method is readable text.
    for label in ["GET", "DELETE", "PURGE"] {
        harness.get_by_label(label);
    }
}

#[test]
fn method_badges_have_a_fixed_width_so_names_line_up() {
    let short = Method::Get;
    let long = Method::Custom("PROPFIND-VERY-LONG-METHOD".into());
    let mut widths = Vec::new();
    let mut harness = Harness::new_ui(|ui| {
        widths.clear();
        for method in [&short, &long] {
            widths.push(ui.add(atoms::MethodBadge::new(method)).rect.width());
        }
    });
    harness.run();
    drop(harness);
    assert_eq!(widths.len(), 2);
    assert!(
        widths
            .iter()
            .all(|w| (*w - theme::METHOD_BADGE_WIDTH).abs() < 0.5),
        "{widths:?}"
    );
}

#[test]
fn status_badges_show_code_and_reason() {
    let harness = Harness::new_ui(|ui| {
        for status in [200, 404, 503, 599] {
            ui.add(atoms::StatusBadge::new(status));
        }
    });
    for label in ["200 OK", "404 Not Found", "503 Service Unavailable", "599"] {
        harness.get_by_label(label);
    }
}

#[test]
fn badges_take_their_colors_from_the_active_theme() {
    let mut seen = Vec::new();
    for dark in [false, true] {
        let mut harness = Harness::new_ui(|ui| {
            *ui.visuals_mut() = theme::visuals(
                if dark {
                    egui::Theme::Dark
                } else {
                    egui::Theme::Light
                },
                theme::Accent::Blue,
            );
            let expected = if dark { theme::DARK } else { theme::LIGHT };
            seen.push(*theme::palette(ui) == expected);
        });
        harness.run();
    }
    assert!(seen.iter().all(|ok| *ok), "{seen:?}");
}

#[test]
fn status_badges_never_wrap_and_move_whole_to_the_next_line() {
    // Each badge fits on a line alone, but not all on one line.
    let width = 220.0;
    let mut rects = Vec::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, 400.0))
        .build_ui(|ui| {
            rects.clear();
            ui.horizontal_wrapped(|ui| {
                for status in [204, 301, 503, 200] {
                    rects.push(ui.add(atoms::StatusBadge::new(status)).rect);
                }
            });
        });
    harness.run();
    drop(harness);
    // One line each, inside the row, and on more than one line overall.
    assert!(
        rects.iter().all(|r| r.height() < 30.0),
        "a badge wrapped: {rects:?}"
    );
    assert!(
        rects.iter().all(|r| r.right() <= width),
        "a badge overflows: {rects:?}"
    );
    assert!(
        rects.iter().any(|r| r.top() > rects[0].bottom()),
        "nothing moved down: {rects:?}"
    );
}
