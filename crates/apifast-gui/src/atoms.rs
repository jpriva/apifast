//! Atoms: the smallest components, each an [`egui::Widget`] or a builder
//! with a `show` method.

mod code_editor;
mod suggestions;
mod template_field;

pub use code_editor::{CodeEditor, code_layout, syntax_layout};
pub use suggestions::{MAX_SUGGESTIONS, completions, refocus, suggestions};
pub use template_field::{
    HintsCache, Span, SpanKind, TemplateField, TemplateFieldTexts, VariableHint, VariableHints,
    complete_variable, spans, variable_prefix,
};

use apifast_domain::{Method, reason_phrase};
use eframe::egui::{self, Align, Layout, RichText, Sense, Widget};

use crate::theme;

/// The method of a request, colored and fixed-width so names line up.
#[derive(Debug, Clone, Copy)]
pub struct MethodBadge<'a> {
    method: &'a Method,
}

impl<'a> MethodBadge<'a> {
    /// A badge for `method`.
    pub fn new(method: &'a Method) -> Self {
        Self { method }
    }
}

impl Widget for MethodBadge<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = theme::method_color(theme::palette(ui), self.method);
        let text = RichText::new(self.method.as_str())
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
