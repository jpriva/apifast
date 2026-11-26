//! Layouts: where the panels go, without knowing what they hold.

use eframe::egui;

use crate::theme;

/// Default width of the sidebar, in points.
const SIDEBAR_WIDTH: f32 = 260.0;
/// Narrowest the sidebar can be resized to, in points.
const SIDEBAR_MIN_WIDTH: f32 = 180.0;

/// The main window: a top bar across the window, a status bar across its
/// foot (when `bottom` is given), a resizable sidebar on the left that
/// slides in and out as its `bool` says (and sets it when dragged closed or
/// open), and the content in the rest.
/// `corner` rounds the outer corners of the panels that reach them, for a
/// window whose frame apifast draws (0 for square). Returns what each part
/// returns.
pub fn workbench<T, B: Default, S: Default, C>(
    ui: &mut egui::Ui,
    corner: u8,
    (top, bottom): (
        impl FnOnce(&mut egui::Ui) -> T,
        Option<impl FnOnce(&mut egui::Ui) -> B>,
    ),
    (expanded, sidebar): (&mut bool, impl FnOnce(&mut egui::Ui) -> S),
    content: impl FnOnce(&mut egui::Ui) -> C,
) -> (T, B, S, C) {
    let round = |nw: bool, ne: bool, sw: bool, se: bool| egui::CornerRadius {
        nw: if nw { corner } else { 0 },
        ne: if ne { corner } else { 0 },
        sw: if sw { corner } else { 0 },
        se: if se { corner } else { 0 },
    };
    let (has_bottom, has_sidebar) = (bottom.is_some(), *expanded);
    let top = egui::Panel::top("top-bar")
        .frame(
            egui::Frame::side_top_panel(ui.style())
                .inner_margin(theme::space::M)
                .corner_radius(round(true, true, false, false)),
        )
        .show(ui, top)
        .inner;
    // The status bar, only when it has something to say.
    let bottom = bottom.map_or_else(B::default, |bottom| {
        egui::Panel::bottom("status-bar")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(theme::space::S)
                    .corner_radius(round(false, false, true, true)),
            )
            .show(ui, bottom)
            .inner
    });
    // Hidden, it gives what it gives when there is nothing to report.
    let sidebar = egui::Panel::left("sidebar")
        .frame(egui::Frame::side_top_panel(ui.style()).corner_radius(round(
            false,
            false,
            !has_bottom,
            false,
        )))
        .resizable(true)
        .default_size(SIDEBAR_WIDTH)
        .min_size(SIDEBAR_MIN_WIDTH)
        .show_collapsible(ui, expanded, sidebar)
        .map_or_else(S::default, |shown| shown.inner);
    let content = egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).corner_radius(round(
            false,
            false,
            !has_bottom && !has_sidebar,
            !has_bottom,
        )))
        .show(ui, content)
        .inner;
    (top, bottom, sidebar, content)
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
