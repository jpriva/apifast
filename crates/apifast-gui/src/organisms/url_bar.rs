//! The URL bar: method, URL and the send or cancel button.

use apifast_domain::Method;
use eframe::egui::{self, Align, Layout, RichText};

use crate::atoms::{TemplateField, TemplateFieldTexts, VariableHints};
use crate::strings::Strings;
use crate::theme;

/// Methods offered in the selector. Others (custom methods in a file) are
/// shown when the request uses them.
const METHODS: [Method; 7] = [
    Method::Get,
    Method::Post,
    Method::Put,
    Method::Patch,
    Method::Delete,
    Method::Head,
    Method::Options,
];

/// What the user did in the URL bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlBarEvent {
    /// The method or the URL changed.
    Edited,
    /// Send the request.
    Send,
    /// Cancel the send in progress.
    Cancel,
}

/// The URL bar of the open request. It edits the caller's copy of the
/// method and URL, which the screen keeps as the tab's draft.
#[derive(Debug)]
pub struct UrlBar<'a> {
    method: &'a mut Method,
    url: &'a mut String,
    hints: &'a VariableHints,
    sending: bool,
    strings: &'a Strings,
}

impl<'a> UrlBar<'a> {
    /// A bar over `method` and `url`, highlighting the variables in
    /// `hints`. While `sending`, the button cancels.
    pub fn new(
        method: &'a mut Method,
        url: &'a mut String,
        hints: &'a VariableHints,
        sending: bool,
        strings: &'a Strings,
    ) -> Self {
        Self {
            method,
            url,
            hints,
            sending,
            strings,
        }
    }

    /// Draws the bar.
    pub fn show(self, ui: &mut egui::Ui) -> Option<UrlBarEvent> {
        let Self {
            method,
            url,
            hints,
            sending,
            strings,
        } = self;
        let mut event = None;
        ui.horizontal(|ui| {
            if method_selector(ui, method, strings) {
                event = Some(UrlBarEvent::Edited);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, action) = if sending {
                    (strings.cancel, UrlBarEvent::Cancel)
                } else {
                    (strings.send, UrlBarEvent::Send)
                };
                if ui.button(text).clicked() {
                    event = Some(action);
                }
                let texts = TemplateFieldTexts {
                    placeholder: strings.url_placeholder,
                    missing: strings.variable_missing,
                    generated: strings.variable_generated,
                };
                let field = TemplateField::new(url, hints, texts)
                    .desired_width(ui.available_width())
                    .show(ui);
                if field.changed() {
                    event = Some(UrlBarEvent::Edited);
                }
            });
        });
        event
    }
}

/// The method, colored as its badge. Returns whether the user changed it.
fn method_selector(ui: &mut egui::Ui, method: &mut Method, strings: &Strings) -> bool {
    let palette = theme::palette(ui);
    let colored = |m: &Method| {
        RichText::new(m.as_str())
            .monospace()
            .strong()
            .color(theme::method_color(palette, m))
    };
    let mut chosen = None;
    let response = egui::ComboBox::from_id_salt("method")
        .selected_text(colored(method))
        .width(theme::METHOD_BADGE_WIDTH + theme::space::L)
        .show_ui(ui, |ui| {
            let custom = (!METHODS.contains(method)).then(|| method.clone());
            for option in METHODS.iter().chain(custom.as_ref()) {
                if ui
                    .selectable_label(option == method, colored(option))
                    .clicked()
                {
                    chosen = Some(option.clone());
                }
            }
        })
        .response;
    response.on_hover_text(strings.method);
    match chosen {
        Some(new) if new != *method => {
            *method = new;
            true
        }
        _ => false,
    }
}
