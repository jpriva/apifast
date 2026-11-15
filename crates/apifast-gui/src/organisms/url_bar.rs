//! The URL bar: method, URL and the send or cancel button.

use apifast_domain::Method;
use eframe::egui::{self, Align, Layout, RichText};

use crate::atoms::MethodBadge;
use crate::strings::Strings;

/// What the user did in the URL bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlBarEvent {
    /// Send the request.
    Send,
    /// Cancel the send in progress.
    Cancel,
}

/// The URL bar of the open request. Read-only until request editing
/// arrives.
#[derive(Debug, Clone, Copy)]
pub struct UrlBar<'a> {
    method: &'a Method,
    url: &'a str,
    sending: bool,
    strings: &'a Strings,
}

impl<'a> UrlBar<'a> {
    /// A bar for `method` and `url`. While `sending`, the button cancels.
    pub fn new(method: &'a Method, url: &'a str, sending: bool, strings: &'a Strings) -> Self {
        Self {
            method,
            url,
            sending,
            strings,
        }
    }

    /// Draws the bar.
    pub fn show(self, ui: &mut egui::Ui) -> Option<UrlBarEvent> {
        let mut event = None;
        ui.horizontal(|ui| {
            ui.add(MethodBadge::new(self.method));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, action) = if self.sending {
                    (self.strings.cancel, UrlBarEvent::Cancel)
                } else {
                    (self.strings.send, UrlBarEvent::Send)
                };
                if ui.button(text).clicked() {
                    event = Some(action);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(self.url).monospace())
                            .truncate()
                            .selectable(true),
                    );
                });
            });
        });
        event
    }
}
