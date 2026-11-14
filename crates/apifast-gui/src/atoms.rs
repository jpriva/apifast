//! Atoms: the smallest components, each an [`egui::Widget`].

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
