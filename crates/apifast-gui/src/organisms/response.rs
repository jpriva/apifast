//! The response panel: status, time and size, then the body or headers.

use apifast_domain::human_size;
use eframe::egui::{self, RichText};

use crate::atoms::StatusBadge;
use crate::core::{Received, ResponseState};
use crate::organisms::BodyViewer;
use crate::strings::Strings;
use crate::theme;

/// Which part of the response is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseTab {
    /// The body.
    #[default]
    Body,
    /// The headers.
    Headers,
}

/// The response panel and its open tab.
#[derive(Debug, Default)]
pub struct ResponseView {
    /// The open tab.
    pub tab: ResponseTab,
    /// The body, read by windows.
    pub viewer: BodyViewer,
}

impl ResponseView {
    /// Draws `state`.
    pub fn show(&mut self, ui: &mut egui::Ui, state: &ResponseState, strings: &Strings) {
        let palette = theme::palette(ui);
        match state {
            ResponseState::Idle => {
                ui.label(RichText::new(strings.response_idle).color(palette.text_muted));
            }
            ResponseState::Sending => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(strings.sending);
                });
            }
            ResponseState::Cancelled => {
                ui.label(RichText::new(strings.cancelled).color(palette.text_muted));
            }
            ResponseState::Failed(message) => {
                ui.label(RichText::new(message.as_str()).color(palette.danger));
            }
            ResponseState::Received(received) => self.received(ui, received, strings),
        }
    }

    fn received(&mut self, ui: &mut egui::Ui, received: &Received, strings: &Strings) {
        let muted = theme::palette(ui).text_muted;
        ui.horizontal(|ui| {
            ui.add(StatusBadge::new(received.head.status));
            ui.label(RichText::new(format!("{} ms", received.head.elapsed_ms)).color(muted));
            ui.label(RichText::new(human_size(received.size)).color(muted));
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, ResponseTab::Body, strings.body);
            let headers = format!("{} ({})", strings.headers, received.head.headers.len());
            ui.selectable_value(&mut self.tab, ResponseTab::Headers, headers);
        });
        ui.separator();
        match self.tab {
            ResponseTab::Body => self.viewer.show(ui, &received.body, strings),
            ResponseTab::Headers => {
                egui::ScrollArea::vertical()
                    .id_salt("response-headers")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("response-headers-grid")
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (name, value) in &received.head.headers {
                                    ui.label(RichText::new(name.as_str()).strong());
                                    ui.label(value.as_str());
                                    ui.end_row();
                                }
                            });
                    });
            }
        }
    }
}
