//! Layouts: where the panels go, without knowing what they hold.

use eframe::egui;

use crate::theme;

/// Default width of the sidebar, in points.
const SIDEBAR_WIDTH: f32 = 260.0;
/// Narrowest the sidebar can be resized to, in points.
const SIDEBAR_MIN_WIDTH: f32 = 180.0;

/// The main window: a top bar across the window, a resizable sidebar on the
/// left (when `sidebar` is given; hidden otherwise) and the content in the
/// rest. Returns what each part returns.
pub fn workbench<T, S: Default, C>(
    ui: &mut egui::Ui,
    top: impl FnOnce(&mut egui::Ui) -> T,
    sidebar: Option<impl FnOnce(&mut egui::Ui) -> S>,
    content: impl FnOnce(&mut egui::Ui) -> C,
) -> (T, S, C) {
    let top = egui::Panel::top("top-bar")
        .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(theme::space::M))
        .show(ui, top)
        .inner;
    // Hidden, it gives what it gives when there is nothing to report.
    let sidebar = sidebar.map_or_else(S::default, |sidebar| {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(SIDEBAR_WIDTH)
            .min_size(SIDEBAR_MIN_WIDTH)
            .show(ui, sidebar)
            .inner
    });
    let content = egui::CentralPanel::default().show(ui, content).inner;
    (top, sidebar, content)
}

/// A single centered column, for screens with little content.
pub fn centered<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::CentralPanel::default()
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 4.0);
                content(ui)
            })
            .inner
        })
        .inner
}
