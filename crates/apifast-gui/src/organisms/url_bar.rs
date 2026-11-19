//! The URL bar: method, URL and the send or cancel button.

use apifast_domain::{Method, RequestKind};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlBarEvent {
    /// The method or the URL changed.
    Edited,
    /// Send the request.
    Send,
    /// Cancel the send in progress.
    Cancel,
    /// A cURL command was pasted into the URL: the request should become
    /// it. The URL was left as it was.
    PastedCurl(String),
}

/// The URL bar of the open request. It edits the caller's copy of the
/// method and URL, which the screen keeps as the tab's draft.
#[derive(Debug)]
pub struct UrlBar<'a> {
    method: &'a mut Method,
    url: &'a mut String,
    hints: &'a VariableHints,
    sending: bool,
    streaming: bool,
    kind: RequestKind,
    strings: &'a Strings,
    focus: bool,
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
            streaming: false,
            kind: RequestKind::Http,
            strings,
            focus: false,
        }
    }

    /// Puts the cursor in the URL field.
    #[must_use]
    pub fn focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    /// While sending, an event stream is arriving: the button stops it,
    /// keeping what arrived.
    #[must_use]
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// What the request is: a WebSocket connection has no method to pick.
    #[must_use]
    pub fn kind(mut self, kind: RequestKind) -> Self {
        self.kind = kind;
        self
    }

    /// Draws the bar.
    pub fn show(self, ui: &mut egui::Ui) -> Option<UrlBarEvent> {
        let Self {
            method,
            url,
            hints,
            sending,
            streaming,
            kind,
            strings,
            focus,
        } = self;
        let mut event = None;
        ui.horizontal(|ui| {
            if kind == RequestKind::WebSocket {
                ui.add(crate::atoms::MethodBadge::new(method).kind(kind))
                    .on_hover_text(strings.ws_badge);
            } else if method_selector(ui, method, strings) {
                event = Some(UrlBarEvent::Edited);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, action) = if sending {
                    let text = if streaming {
                        strings.stop
                    } else {
                        strings.cancel
                    };
                    (text, UrlBarEvent::Cancel)
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
                let id = ui.make_persistent_id("url-field");
                // Taken before the field sees it: a one-line field would
                // drop the command's line breaks.
                if let Some(command) = take_pasted_curl(ui, id) {
                    event = Some(UrlBarEvent::PastedCurl(command));
                }
                let field = TemplateField::new(url, hints, texts)
                    .id(id)
                    .accessible_name(strings.url)
                    .desired_width(ui.available_width())
                    .show(ui);
                if focus {
                    field.request_focus();
                }
                if field.changed() && event.is_none() {
                    event = Some(UrlBarEvent::Edited);
                }
            });
        });
        event
    }
}

/// A `curl …` pasted into the field `id`, if it has the focus, taken out
/// of the input so the field does not get it.
fn take_pasted_curl(ui: &egui::Ui, id: egui::Id) -> Option<String> {
    if !ui.memory(|m| m.has_focus(id)) {
        return None;
    }
    ui.input_mut(|input| {
        let at = input.events.iter().position(
            |e| matches!(e, egui::Event::Paste(text) if text.trim_start().starts_with("curl ")),
        )?;
        match input.events.remove(at) {
            egui::Event::Paste(text) => Some(text),
            _ => None,
        }
    })
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
