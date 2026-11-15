//! The response panel: status, time and size, then the body, the headers or
//! the timeline of the send.
//!
//! The body is shown "pretty" (formatted and colored) when it is JSON or XML
//! small enough to load whole, and otherwise "raw", by windows, whatever its
//! size.

use apifast_app::{TimelineEvent, TimelineKind};
use apifast_domain::human_size;
use eframe::egui::{self, RichText};

use crate::atoms::StatusBadge;
use crate::body::BodyHandle;
use crate::code::{self, Language};
use crate::core::{Received, ResponseState};
use crate::organisms::BodyViewer;
use crate::strings::Strings;
use crate::theme;

/// Largest body shown pretty, in bytes: it is read and formatted whole.
pub const PRETTY_LIMIT: u64 = 2 * 1024 * 1024;
/// Largest body the copy button copies, in bytes.
pub const COPY_LIMIT: u64 = 16 * 1024 * 1024;
/// Bytes looked at to tell the language and whether the body is binary.
const SNIFF_BYTES: usize = 8 * 1024;

/// Which part of the response is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseTab {
    /// The body.
    #[default]
    Body,
    /// The headers.
    Headers,
    /// What happened while sending, with times.
    Timeline,
}

/// How the body is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BodyMode {
    /// Formatted and colored, when it can be.
    #[default]
    Pretty,
    /// As received.
    Raw,
}

/// What the panel worked out about one body, kept while it is shown.
#[derive(Debug)]
struct Prepared {
    key: String,
    language: Option<Language>,
    binary: bool,
    /// The pretty text, made the first time it is shown.
    pretty: Option<Pretty>,
}

#[derive(Debug)]
struct Pretty {
    body: BodyHandle,
    /// Whether it could be formatted; if not, it is the body as received.
    formatted: bool,
}

/// The response panel and what it remembers.
#[derive(Debug, Default)]
pub struct ResponseView {
    /// The open tab.
    pub tab: ResponseTab,
    /// How the body is shown.
    pub mode: BodyMode,
    viewer: BodyViewer,
    prepared: Option<Prepared>,
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
            ui.selectable_value(&mut self.tab, ResponseTab::Timeline, strings.timeline);
        });
        ui.separator();
        match self.tab {
            ResponseTab::Body => self.body(ui, received, strings),
            ResponseTab::Headers => headers(ui, &received.head.headers),
            ResponseTab::Timeline => timeline(ui, received, strings),
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, received: &Received, strings: &Strings) {
        let body = &received.body;
        let muted = theme::palette(ui).text_muted;
        let (language, binary) = self.prepare(received);
        if binary {
            let text = format!("{} {}", strings.body_binary, human_size(body.len()));
            ui.label(RichText::new(text).color(muted));
            return;
        }
        let can_pretty = language.is_some() && body.len() <= PRETTY_LIMIT;
        ui.horizontal(|ui| {
            if can_pretty {
                ui.selectable_value(&mut self.mode, BodyMode::Pretty, strings.pretty);
                ui.selectable_value(&mut self.mode, BodyMode::Raw, strings.raw);
            }
            if body.len() <= COPY_LIMIT
                && ui
                    .button(strings.copy)
                    .on_hover_text(strings.copy_body)
                    .clicked()
                && let Ok(bytes) = body.read(0, usize::try_from(body.len()).unwrap_or(usize::MAX))
            {
                ui.ctx()
                    .copy_text(String::from_utf8_lossy(&bytes).into_owned());
            }
        });
        if language.is_some() && !can_pretty {
            ui.label(RichText::new(strings.pretty_off).color(muted));
        }
        let pretty = can_pretty && self.mode == BodyMode::Pretty;
        let Some(prepared) = self.prepared.as_mut() else {
            return;
        };
        match (pretty, language) {
            (true, Some(language)) => {
                let pretty = prepared
                    .pretty
                    .get_or_insert_with(|| make_pretty(body, language));
                if !pretty.formatted {
                    ui.label(RichText::new(strings.not_formatted).color(muted));
                }
                let shown = pretty.body.clone();
                self.viewer.show(ui, &shown, Some(language), strings);
            }
            _ => self.viewer.show(ui, body, None, strings),
        }
    }

    /// The language of the body shown and whether it is binary, worked
    /// out once per body.
    fn prepare(&mut self, received: &Received) -> (Option<Language>, bool) {
        let body = &received.body;
        let prepared = match self.prepared.take() {
            Some(prepared) if prepared.key == body.key() => prepared,
            _ => {
                let start = body.read(0, SNIFF_BYTES).unwrap_or_default();
                let content_type = received
                    .head
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.as_str());
                Prepared {
                    key: body.key().to_owned(),
                    language: code::detect(content_type, &start),
                    binary: code::looks_binary(&start),
                    pretty: None,
                }
            }
        };
        let found = (prepared.language, prepared.binary);
        self.prepared = Some(prepared);
        found
    }
}

/// The body formatted as `language`, or as received if it cannot be.
fn make_pretty(body: &BodyHandle, language: Language) -> Pretty {
    let bytes = body
        .read(0, usize::try_from(body.len()).unwrap_or(usize::MAX))
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let (text, formatted) = match code::format(&text, language) {
        Ok(pretty) => (pretty, true),
        Err(_) => (text.into_owned(), false),
    };
    Pretty {
        body: BodyHandle::memory(format!("{}#pretty", body.key()), text),
        formatted,
    }
}

fn headers(ui: &mut egui::Ui, headers: &[(String, String)]) {
    egui::ScrollArea::vertical()
        .id_salt("response-headers")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("response-headers-grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (name, value) in headers {
                        ui.label(RichText::new(name.as_str()).strong());
                        ui.label(value.as_str());
                        ui.end_row();
                    }
                });
        });
}

/// Microseconds as milliseconds with one decimal: `12.3 ms`.
pub fn millis(us: u64) -> String {
    format!("{}.{} ms", us / 1000, us % 1000 / 100)
}

/// What a timeline event says, ready to show.
pub fn describe(event: &TimelineEvent, strings: &Strings) -> String {
    match &event.kind {
        TimelineKind::Request { method, url } => format!("{method} {url}"),
        TimelineKind::Redirect { status, location } => {
            format!("{} {status}: {location}", strings.event_redirect)
        }
        TimelineKind::Headers { status } => {
            format!("{}: {}", strings.event_headers, StatusBadge::text(*status))
        }
        TimelineKind::Body { bytes } => {
            format!("{}: {}", strings.event_body, human_size(*bytes))
        }
    }
}

fn timeline(ui: &mut egui::Ui, received: &Received, strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    let events = &received.head.timeline;
    if events.is_empty() {
        ui.label(RichText::new(strings.timeline_empty).color(muted));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("response-timeline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("response-timeline-grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for event in events {
                        ui.label(RichText::new(millis(event.at_us)).monospace().color(muted));
                        ui.label(describe(event, strings));
                        ui.end_row();
                    }
                });
        });
}
