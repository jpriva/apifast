//! The window's own frame, for when apifast draws its title bar instead of
//! the system: the top bar moves the window when dragged and maximizes it
//! on a double click, the buttons minimize, maximize and close it (always
//! the three, whatever the desktop shows), and the window's edges resize
//! it. All of it asks the system through viewport commands, which winit
//! carries out on Wayland and X11.

use eframe::egui::{self, CursorIcon, PointerButton, ResizeDirection, Sense, ViewportCommand};

use crate::strings::Strings;
use crate::theme;

/// Whether apifast can draw its own title bar here: on Linux. Windows and
/// macOS keep theirs until it is tried there.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// How far from the window's edge the pointer resizes it, in points, when
/// the window fills itself to the edge.
const EDGE: f32 = 6.0;
/// The transparent room around a window whose frame apifast draws: where
/// its shadow falls, and where the pointer resizes it, in points.
pub const FRAME_MARGIN: f32 = 12.0;
/// How round the corners of that window are, in points.
pub const FRAME_CORNER: u8 = 12;
/// How far from a corner the pointer resizes both ways, in points.
const CORNER: f32 = 14.0;

/// Draws the window's frame, for a transparent window without system
/// decorations: a shadow in the margin around it, the window rounded at
/// `corner` points, and a thin border over its content, which `add` puts
/// inside. The shadow is lighter while the window does not have the focus.
pub fn window_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui, u8) -> R) -> R {
    let rect = ui.max_rect().shrink(FRAME_MARGIN);
    let corner = egui::CornerRadius::same(FRAME_CORNER);
    let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
    let dark = ui.visuals().dark_mode;
    let alpha = match (dark, focused) {
        (true, true) => 140,
        (true, false) => 80,
        (false, true) => 70,
        (false, false) => 35,
    };
    let shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(alpha),
    };
    let painter = ui.painter().clone();
    painter.add(shadow.as_shape(rect, corner));
    painter.rect_filled(rect, corner, ui.visuals().panel_fill);
    let inner = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            add(ui, FRAME_CORNER)
        })
        .inner;
    let border = theme::palette(ui).border;
    painter.rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    inner
}

/// Whether the window is maximized, as the system last said.
pub fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Makes the space behind the widgets of `ui` (the title bar) move the
/// window when dragged, and maximize or restore it on a double click. Call
/// it before adding the bar's widgets, so they stay on top.
pub fn drag_area(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let response = ui.interact(
        rect,
        ui.scope_id().with("title-bar-drag"),
        Sense::click_and_drag(),
    );
    if response.double_clicked_by(PointerButton::Primary) {
        let maximized = maximized(ui.ctx());
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// What a window button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Minimize,
    Maximize,
    Close,
}

/// Minimize, maximize (or restore) and close, drawn left to right; in a
/// right-to-left layout, close comes first.
pub fn window_controls(ui: &mut egui::Ui, strings: &Strings) {
    let maximized = maximized(ui.ctx());
    let maximize = if maximized {
        strings.window_restore
    } else {
        strings.window_maximize
    };
    let mut controls = [
        (Control::Minimize, strings.window_minimize),
        (Control::Maximize, maximize),
        (Control::Close, strings.window_close),
    ];
    if ui.layout().prefer_right_to_left() {
        controls.reverse();
    }
    ui.spacing_mut().item_spacing.x = theme::space::XS;
    for (control, name) in controls {
        if control_button(ui, control, name, maximized).clicked() {
            let command = match control {
                Control::Minimize => ViewportCommand::Minimized(true),
                Control::Maximize => ViewportCommand::Maximized(!maximized),
                // Comes back as a request to close, which may be asked about.
                Control::Close => ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// One window button, its symbol drawn (the fonts have none of them).
fn control_button(
    ui: &mut egui::Ui,
    control: Control,
    name: &str,
    maximized: bool,
) -> egui::Response {
    let height = ui.spacing().interact_size.y.max(24.0);
    let size = egui::vec2(height * 1.25, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if ui.is_rect_visible(rect) {
        let palette = theme::palette(ui);
        let hot = response.hovered() || response.has_focus();
        let mut color = palette.text;
        if hot {
            let fill = if control == Control::Close {
                color = egui::Color32::WHITE;
                palette.danger
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, theme::radius::MEDIUM, fill);
        }
        let painter = ui.painter();
        let stroke = egui::Stroke::new(1.2, color);
        let c = rect.center();
        let r = height * 0.18;
        match control {
            Control::Minimize => {
                painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, r)], stroke);
            }
            Control::Maximize if maximized => {
                // Restore: two squares, one behind the other.
                let small = egui::Vec2::splat(r * 1.5);
                let front = egui::Rect::from_min_size(c - egui::vec2(r, r * 0.5), small);
                let back = front.translate(egui::vec2(r * 0.5, -r * 0.5));
                painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Middle);
                painter.rect_filled(front, 1.0, ui.visuals().panel_fill);
                painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Middle);
            }
            Control::Maximize => {
                let square = egui::Rect::from_center_size(c, egui::Vec2::splat(r * 2.0));
                painter.rect_stroke(square, 1.0, stroke, egui::StrokeKind::Middle);
            }
            Control::Close => {
                painter.line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
                painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], stroke);
            }
        }
    }
    response.on_hover_text(name)
}

/// The bands along the window's edges and corners, each with the way it
/// resizes, for a window as big as `window`: `edge` deep, and the corners
/// `edge` more on each side.
pub fn edges(window: egui::Rect, edge: f32) -> [(ResizeDirection, egui::Rect); 8] {
    use ResizeDirection::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
    let corner = edge + CORNER - EDGE;
    let (min, max) = (window.min, window.max);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    };
    [
        (
            NorthWest,
            rect(min.x, min.y, min.x + corner, min.y + corner),
        ),
        (
            NorthEast,
            rect(max.x - corner, min.y, max.x, min.y + corner),
        ),
        (
            SouthWest,
            rect(min.x, max.y - corner, min.x + corner, max.y),
        ),
        (
            SouthEast,
            rect(max.x - corner, max.y - corner, max.x, max.y),
        ),
        (
            North,
            rect(min.x + corner, min.y, max.x - corner, min.y + edge),
        ),
        (
            South,
            rect(min.x + corner, max.y - edge, max.x - corner, max.y),
        ),
        (
            West,
            rect(min.x, min.y + corner, min.x + edge, max.y - corner),
        ),
        (
            East,
            rect(max.x - edge, min.y + corner, max.x, max.y - corner),
        ),
    ]
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// Lets the window's edges and corners resize it, over everything else,
/// unless it is maximized. With its frame drawn (`framed`), the margin
/// around it resizes it, and a little inside.
pub fn resize_edges(ctx: &egui::Context, framed: bool) {
    if maximized(ctx) {
        return;
    }
    let edge = if framed { FRAME_MARGIN + 3.0 } else { EDGE };
    let window = ctx.content_rect();
    egui::Area::new(egui::Id::unique("window-resize-edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            for (direction, rect) in edges(window, edge) {
                let id = ui.scope_id().with(("edge", format!("{direction:?}")));
                let response = ui.interact(rect, id, Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor(direction));
                }
                if response.drag_started_by(PointerButton::Primary) {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_go_around_the_window_without_overlapping() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let edges = edges(window, EDGE);
        for (i, (a, ra)) in edges.iter().enumerate() {
            assert!(window.contains_rect(*ra), "{a:?} is inside");
            for (b, rb) in &edges[i + 1..] {
                assert!(!ra.intersects(rb.shrink(0.01)), "{a:?} and {b:?}");
            }
        }
        let at = |p: egui::Pos2| edges.iter().find(|(_, r)| r.contains(p)).map(|(d, _)| *d);
        assert_eq!(at(egui::pos2(1.0, 1.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(at(egui::pos2(400.0, 599.0)), Some(ResizeDirection::South));
        assert_eq!(at(egui::pos2(799.0, 300.0)), Some(ResizeDirection::East));
        assert_eq!(at(egui::pos2(400.0, 300.0)), None);
    }
}
