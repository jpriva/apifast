//! Atoms: the smallest components, each an [`egui::Widget`] or a builder
//! with a `show` method.

mod code_editor;
mod suggestions;
mod template_field;

pub use code_editor::{CodeEditor, Completer, EditorOptions, code_layout, syntax_layout};
pub use suggestions::{MAX_SUGGESTIONS, completions, refocus, suggestions};
pub use template_field::{
    HintsCache, Span, SpanKind, TemplateField, TemplateFieldTexts, VariableHint, VariableHints,
    complete_variable, spans, variable_prefix,
};

use apifast_domain::{Method, RequestKind, reason_phrase};
use eframe::egui::{self, Align, Layout, RichText, Sense, Widget};

use crate::theme;

/// The method of a request, colored and fixed-width so names line up.
#[derive(Debug, Clone, Copy)]
pub struct MethodBadge<'a> {
    method: &'a Method,
    kind: RequestKind,
}

impl<'a> MethodBadge<'a> {
    /// A badge for `method`.
    pub fn new(method: &'a Method) -> Self {
        Self {
            method,
            kind: RequestKind::Http,
        }
    }

    /// For a request of `kind`: a WebSocket one shows `WS`, not its method.
    #[must_use]
    pub fn kind(mut self, kind: RequestKind) -> Self {
        self.kind = kind;
        self
    }
}

impl Widget for MethodBadge<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let palette = theme::palette(ui);
        let (label, color) = match self.kind {
            RequestKind::WebSocket => ("WS", palette.accent),
            RequestKind::Grpc => ("gRPC", palette.info),
            _ => (
                self.method.as_str(),
                theme::method_color(palette, self.method),
            ),
        };
        let text = RichText::new(label)
            .monospace()
            .strong()
            .size(theme::text::SMALL)
            .color(color);
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        // The response covers the whole fixed width, not just the text, so
        // every badge has the same size whatever its method.
        ui.allocate_ui_with_layout(
            egui::vec2(theme::METHOD_BADGE_WIDTH, height),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(theme::METHOD_BADGE_WIDTH);
                // Custom methods can be long: keep the badge one line wide.
                ui.add(egui::Label::new(text).truncate().selectable(false));
            },
        )
        .response
    }
}

/// An HTTP status, as `404 Not Found`, on a tinted background.
#[derive(Debug, Clone, Copy)]
pub struct StatusBadge {
    status: u16,
}

impl StatusBadge {
    /// A badge for `status`.
    pub fn new(status: u16) -> Self {
        Self { status }
    }

    /// The text the badge shows.
    pub fn text(status: u16) -> String {
        match reason_phrase(status) {
            Some(reason) => format!("{status} {reason}"),
            None => status.to_string(),
        }
    }
}

impl Widget for StatusBadge {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = theme::status_color(theme::palette(ui), self.status);
        let text = Self::text(self.status);
        // The color goes on the text itself: the theme's override text color
        // would win over a color given only when painting.
        // Lay the text out first and allocate its exact size, so a wrapping
        // row can move the whole badge to the next line. A badge never wraps.
        let galley = egui::WidgetText::from(RichText::new(&text).strong().color(color))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
        let padding = egui::vec2(6.0, 2.0);
        let (rect, response) =
            ui.allocate_exact_size(galley.size() + 2.0 * padding, Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            painter.rect_filled(
                rect,
                theme::radius::SMALL,
                color.gamma_multiply(theme::BADGE_TINT),
            );
            painter.galley(rect.min + padding, galley, color);
        }
        response
    }
}

/// A small square of an environment's color; empty space without one, so
/// names line up.
#[derive(Debug, Clone, Copy)]
pub struct ColorSwatch<'a> {
    color: Option<&'a str>,
}

impl<'a> ColorSwatch<'a> {
    /// A swatch of `color`, written as `#rrggbb`.
    pub fn new(color: Option<&'a str>) -> Self {
        Self { color }
    }
}

impl Widget for ColorSwatch<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        if let Some(color) = self.color.and_then(theme::parse_color) {
            ui.painter().rect_filled(rect, theme::radius::SMALL, color);
        }
        response
    }
}

/// A symbol drawn with lines, not written: the fonts have few of them, and
/// a missing glyph stops the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A plus: create.
    Plus,
    /// A folder: open one.
    Folder,
    /// An arrow into a tray: import.
    Import,
    /// A page with a plus: a new request.
    NewRequest,
    /// A folder with a plus: a new folder.
    NewFolder,
    /// A bin: remove.
    Trash,
}

impl Icon {
    /// Draws the icon in `rect` with `stroke`.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
        let r = rect.shrink(rect.width() * 0.2);
        let (c, w, h) = (r.center(), r.width(), r.height());
        let at = |x: f32, y: f32| egui::pos2(r.left() + w * x, r.top() + h * y);
        let plus = |center: egui::Pos2, arm: f32| {
            painter.line_segment(
                [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
                stroke,
            );
            painter.line_segment(
                [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
                stroke,
            );
        };
        let folder = || {
            let points = vec![
                at(0.0, 0.15),
                at(0.38, 0.15),
                at(0.5, 0.3),
                at(1.0, 0.3),
                at(1.0, 0.9),
                at(0.0, 0.9),
                at(0.0, 0.15),
            ];
            painter.add(egui::Shape::line(points, stroke));
        };
        match self {
            Self::Plus => plus(c, w * 0.45),
            Self::Folder => folder(),
            Self::Import => {
                // A tray, and an arrow coming down into it.
                let tray = vec![at(0.0, 0.6), at(0.0, 0.95), at(1.0, 0.95), at(1.0, 0.6)];
                painter.add(egui::Shape::line(tray, stroke));
                painter.line_segment([at(0.5, 0.0), at(0.5, 0.72)], stroke);
                let head = vec![at(0.25, 0.48), at(0.5, 0.72), at(0.75, 0.48)];
                painter.add(egui::Shape::line(head, stroke));
            }
            Self::NewRequest => {
                let page = vec![
                    at(0.1, 0.0),
                    at(0.6, 0.0),
                    at(0.85, 0.25),
                    at(0.85, 1.0),
                    at(0.1, 1.0),
                    at(0.1, 0.0),
                ];
                painter.add(egui::Shape::line(page, stroke));
                plus(at(0.47, 0.6), w * 0.17);
            }
            Self::NewFolder => {
                folder();
                plus(at(0.5, 0.6), w * 0.15);
            }
            Self::Trash => {
                // The lid and its handle, and the bin, narrower at the bottom.
                painter.line_segment([at(0.0, 0.18), at(1.0, 0.18)], stroke);
                let handle = vec![
                    at(0.35, 0.18),
                    at(0.38, 0.02),
                    at(0.62, 0.02),
                    at(0.65, 0.18),
                ];
                painter.add(egui::Shape::line(handle, stroke));
                let bin = vec![at(0.12, 0.18), at(0.2, 1.0), at(0.8, 1.0), at(0.88, 0.18)];
                painter.add(egui::Shape::line(bin, stroke));
                painter.line_segment([at(0.4, 0.38), at(0.42, 0.8)], stroke);
                painter.line_segment([at(0.6, 0.38), at(0.58, 0.8)], stroke);
            }
        }
    }
}

/// A square button with an [`Icon`], named for screen readers and in its
/// hover, framed only while hovered or focused.
#[derive(Debug, Clone, Copy)]
pub struct IconButton<'a> {
    icon: Icon,
    name: &'a str,
    side: Option<f32>,
    quiet: bool,
}

impl<'a> IconButton<'a> {
    /// A button showing `icon`, named `name`.
    pub fn new(icon: Icon, name: &'a str) -> Self {
        Self {
            icon,
            name,
            side: None,
            quiet: false,
        }
    }

    /// `side` points square, instead of as tall as a control.
    #[must_use]
    pub fn side(mut self, side: f32) -> Self {
        self.side = Some(side);
        self
    }

    /// Draws the icon only while the button is hovered or focused, when
    /// `quiet`: for the buttons of a row, shown as the row is hovered.
    /// The button is there all the same, for the keyboard and screen
    /// readers.
    #[must_use]
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let side = self.side.unwrap_or(ui.spacing().interact_size.y);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), self.name)
        });
        let hot = response.hovered() || response.has_focus();
        if ui.is_rect_visible(rect) && (hot || !self.quiet) {
            let palette = theme::palette(ui);
            if hot {
                let fill = ui.visuals().widgets.hovered.weak_bg_fill;
                ui.painter().rect_filled(rect, theme::radius::MEDIUM, fill);
            }
            let color = if !ui.is_enabled() {
                palette.text_muted.gamma_multiply(0.6)
            } else if hot {
                palette.text
            } else {
                palette.text_muted
            };
            self.icon
                .paint(ui.painter(), rect, egui::Stroke::new(1.4, color));
        }
        response.on_hover_text(self.name)
    }
}

/// The button of a screen's main action (Send): filled with the accent.
pub fn primary_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    let palette = theme::palette(ui);
    egui::Button::new(
        egui::RichText::new(text)
            .color(theme::on_accent(palette))
            .strong(),
    )
    .fill(palette.accent)
    .min_size(egui::vec2(72.0, 0.0))
}

/// The button of the application's menu: three lines, drawn (the fonts
/// have no glyph for them), named for screen readers and in its hover.
#[derive(Debug, Clone, Copy)]
pub struct MenuButton<'a> {
    name: &'a str,
}

impl<'a> MenuButton<'a> {
    /// A button named `name`.
    pub fn new(name: &'a str) -> Self {
        Self { name }
    }
}

impl Widget for MenuButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let side = ui.spacing().interact_size.y.max(22.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, self.name));
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            ui.painter()
                .rect_filled(rect, theme::radius::MEDIUM, visuals.weak_bg_fill);
            let stroke = egui::Stroke::new(1.5, theme::palette(ui).text);
            let (c, half) = (rect.center(), side * 0.22);
            for dy in [-0.6, 0.0, 0.6] {
                let y = c.y + dy * half;
                ui.painter().line_segment(
                    [egui::pos2(c.x - half, y), egui::pos2(c.x + half, y)],
                    stroke,
                );
            }
        }
        response.on_hover_text(self.name)
    }
}

/// A small button with an ×, to close or remove something. Drawn, not
/// written: the glyph looks different, or is missing, in some fonts. It has
/// no background until hovered, so rows of them stay quiet.
#[derive(Debug, Clone, Copy)]
pub struct CloseButton<'a> {
    name: &'a str,
}

impl<'a> CloseButton<'a> {
    /// A button named `name` for screen readers and in its hover.
    pub fn new(name: &'a str) -> Self {
        Self { name }
    }
}

impl Widget for CloseButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let side = ui.spacing().interact_size.y.min(18.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, self.name));
        if ui.is_rect_visible(rect) {
            let palette = theme::palette(ui);
            let hovered = response.hovered() || response.has_focus();
            if hovered {
                ui.painter().rect_filled(
                    rect,
                    theme::radius::SMALL,
                    ui.visuals().widgets.hovered.weak_bg_fill,
                );
            }
            let color = if hovered {
                palette.text
            } else {
                palette.text_muted
            };
            let arm = side * 0.2;
            let c = rect.center();
            let stroke = egui::Stroke::new(1.4, color);
            ui.painter().line_segment(
                [c + egui::vec2(-arm, -arm), c + egui::vec2(arm, arm)],
                stroke,
            );
            ui.painter().line_segment(
                [c + egui::vec2(-arm, arm), c + egui::vec2(arm, -arm)],
                stroke,
            );
        }
        response.on_hover_text(self.name)
    }
}
