//! The response panel: status, time and size, then the body, the headers or
//! the timeline of the send, and the events of an event stream, live while
//! it arrives.
//!
//! The body is shown "pretty" (formatted and colored) when it is JSON or XML
//! small enough to load whole, and otherwise "raw", by windows, whatever its
//! size.

use apifast_app::{TimelineEvent, TimelineKind};
use apifast_domain::{SseEvent, human_size, is_event_stream, parse_events};
use eframe::egui::{self, RichText};

use crate::atoms::StatusBadge;
use crate::body::BodyHandle;
use crate::code::{self, Language};
use crate::core::{LiveStream, Received, ResponseState};
use crate::organisms::{BodyViewer, EventList, response_cookies};
use crate::strings::Strings;
use crate::theme;

/// Largest body shown pretty, in bytes: it is read and formatted whole.
pub const PRETTY_LIMIT: u64 = 2 * 1024 * 1024;
/// Largest body the copy button copies, in bytes.
pub const COPY_LIMIT: u64 = 16 * 1024 * 1024;
/// Largest part of an event stream read into its events, in bytes.
pub const EVENTS_LIMIT: u64 = 16 * 1024 * 1024;
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
    /// The cookies the response set.
    Cookies,
    /// What happened while sending, with times.
    Timeline,
    /// The assertions and captures that ran.
    Tests,
    /// The events of an event stream.
    Events,
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
    /// The largest body shown pretty, in bytes; [`PRETTY_LIMIT`] if `None`.
    pub pretty_limit: Option<u64>,
    /// The JSONPath or XPath that filters the body; empty shows it all.
    pub filter: String,
    /// "Save as example" was pressed; taken by the screen, which adds the
    /// example to the request.
    pub example_requested: bool,
    /// What saving an example said: done, or why not.
    pub example_note: Option<String>,
    viewer: BodyViewer,
    prepared: Option<Prepared>,
    /// What the filter selected, for the body and expression it ran on.
    filtered: Option<Filtered>,
    events: EventList,
    /// The events read from a stored stream: its key, them, and whether the
    /// stream was longer than [`EVENTS_LIMIT`].
    parsed: Option<(String, Vec<SseEvent>, bool)>,
}

/// The result of the filter over one body.
#[derive(Debug)]
struct Filtered {
    /// The body's key and the expression.
    key: (String, String),
    /// The selected values, shown formatted, or why there are none.
    result: Result<(BodyHandle, Option<Language>, usize), String>,
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
            ResponseState::Streaming(stream) => self.streaming(ui, stream, strings),
            ResponseState::Cancelled => {
                ui.label(RichText::new(strings.cancelled).color(palette.text_muted));
            }
            ResponseState::Failed(message) => {
                ui.label(RichText::new(message.as_str()).color(palette.danger));
            }
            ResponseState::Received(received) => self.received(ui, received, strings),
        }
    }

    /// An event stream as it arrives: its status and its events so far.
    fn streaming(&mut self, ui: &mut egui::Ui, stream: &LiveStream, strings: &Strings) {
        let muted = theme::palette(ui).text_muted;
        self.tab = ResponseTab::Events;
        stream.read(|live| {
            ui.horizontal(|ui| {
                if let Some(head) = &live.head {
                    ui.add(StatusBadge::new(head.status));
                }
                ui.spinner();
                ui.label(RichText::new(strings.streaming).color(muted));
                ui.label(RichText::new(human_size(live.bytes)).color(muted));
            });
            ui.separator();
            self.events
                .show(ui, (&live.events, live.dropped + 1), true, strings);
        });
    }

    fn received(&mut self, ui: &mut egui::Ui, received: &Received, strings: &Strings) {
        let muted = theme::palette(ui).text_muted;
        let stream = (received.head.headers.iter()).any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type") && is_event_stream(value)
        });
        if !stream && self.tab == ResponseTab::Events {
            self.tab = ResponseTab::Body;
        }
        ui.horizontal(|ui| {
            ui.add(StatusBadge::new(received.head.status));
            ui.label(RichText::new(format!("{} ms", received.head.elapsed_ms)).color(muted));
            ui.label(RichText::new(size_text(received)).color(muted));
        });
        ui.horizontal(|ui| {
            if stream {
                let count = parsed_events(&mut self.parsed, received).1.len();
                let label = format!("{} ({count})", strings.events);
                ui.selectable_value(&mut self.tab, ResponseTab::Events, label);
            }
            ui.selectable_value(&mut self.tab, ResponseTab::Body, strings.body);
            let headers = format!("{} ({})", strings.headers, received.head.headers.len());
            ui.selectable_value(&mut self.tab, ResponseTab::Headers, headers);
            let cookies = match response_cookies::count(&received.head.headers) {
                0 => strings.cookies.to_owned(),
                n => format!("{} ({n})", strings.cookies),
            };
            ui.selectable_value(&mut self.tab, ResponseTab::Cookies, cookies);
            ui.selectable_value(&mut self.tab, ResponseTab::Timeline, strings.timeline);
            ui.selectable_value(
                &mut self.tab,
                ResponseTab::Tests,
                tests_label(ui, received, strings),
            );
        });
        ui.separator();
        match self.tab {
            ResponseTab::Body => self.body(ui, received, strings),
            ResponseTab::Headers => headers(ui, &received.head.headers),
            ResponseTab::Cookies => {
                let head = &received.head;
                let now = apifast_app::Clock::unix_millis(&crate::system::SystemClock);
                let cookies = response_cookies::set_by(&head.headers, &head.url, now);
                response_cookies::show(ui, &cookies, strings);
            }
            ResponseTab::Timeline => timeline(ui, received, strings),
            ResponseTab::Tests => tests(ui, received, strings),
            ResponseTab::Events => {
                let (partial, events) = parsed_events(&mut self.parsed, received);
                if partial {
                    ui.label(RichText::new(strings.events_partial).color(muted));
                }
                self.events.show(ui, (events, 1), false, strings);
            }
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
        let limit = self.pretty_limit.unwrap_or(PRETTY_LIMIT);
        let can_pretty = language.is_some() && body.len() <= limit;
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
            if ui
                .button(strings.save_as_example)
                .on_hover_text(strings.save_as_example_hint)
                .clicked()
            {
                self.example_requested = true;
            }
        });
        if let Some(note) = &self.example_note {
            ui.label(RichText::new(note.as_str()).color(muted).small());
        }
        if language.is_some() && !can_pretty {
            ui.label(RichText::new(strings.pretty_off).color(muted));
        }
        let can_filter = can_pretty && matches!(language, Some(Language::Json | Language::Xml));
        if can_filter && self.filtered_view(ui, body, language, strings) {
            return;
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
    /// The filter field and, when it has an expression, what it selects
    /// instead of the body. Returns whether it drew the result.
    fn filtered_view(
        &mut self,
        ui: &mut egui::Ui,
        body: &BodyHandle,
        language: Option<Language>,
        strings: &Strings,
    ) -> bool {
        let palette = theme::palette(ui);
        let hint = if language == Some(Language::Xml) {
            strings.filter_xpath
        } else {
            strings.filter_jsonpath
        };
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(hint)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        )
        .accessible_name(strings.filter_body);
        let expression = self.filter.trim().to_owned();
        if expression.is_empty() {
            return false;
        }
        let key = (body.key().to_owned(), expression);
        if self.filtered.as_ref().is_none_or(|f| f.key != key) {
            let result = filter(body, &key.1);
            self.filtered = Some(Filtered { key, result });
        }
        let Some(filtered) = &self.filtered else {
            return false;
        };
        match &filtered.result {
            Err(message) => {
                ui.label(RichText::new(message.as_str()).color(palette.danger));
            }
            Ok((shown, shown_language, count)) => {
                let text = strings
                    .filter_results
                    .replace("{count}", &count.to_string());
                ui.label(RichText::new(text).color(palette.text_muted));
                let shown = shown.clone();
                let shown_language = *shown_language;
                self.viewer.show(ui, &shown, shown_language, strings);
            }
        }
        true
    }

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
/// Runs `expression` over the whole body: the values a JSONPath selects
/// as formatted JSON (one value, or an array of several), or the texts an
/// XPath selects, one per line.
fn filter(
    body: &BodyHandle,
    expression: &str,
) -> Result<(BodyHandle, Option<Language>, usize), String> {
    let bytes = body.read(0, usize::try_from(body.len()).unwrap_or(usize::MAX))?;
    let text = String::from_utf8_lossy(&bytes);
    let json = apifast_domain::Json::parse(&text).ok();
    let values =
        apifast_query::select(expression, &text, json.as_ref()).map_err(|e| e.to_string())?;
    let count = values.len();
    let key = format!("{}#filter#{expression}", body.key());
    if apifast_query::Language::of(expression) == apifast_query::Language::XPath {
        let lines: Vec<String> = values.iter().map(apifast_domain::Json::to_text).collect();
        return Ok((BodyHandle::memory(key, lines.join("\n")), None, count));
    }
    let value = match values.as_slice() {
        [one] => one.clone(),
        _ => apifast_domain::Json::Array(values),
    };
    let compact = value.to_string();
    let pretty = code::format(&compact, Language::Json).unwrap_or(compact);
    Ok((BodyHandle::memory(key, pretty), Some(Language::Json), count))
}

/// The events of the stored stream, read into `parsed` the first time they
/// are asked for; whether the stream was longer than what was read.
fn parsed_events<'a>(
    parsed: &'a mut Option<(String, Vec<SseEvent>, bool)>,
    received: &Received,
) -> (bool, &'a [SseEvent]) {
    let body = &received.body;
    if parsed.as_ref().is_none_or(|(key, ..)| key != body.key()) {
        let len = usize::try_from(body.len().min(EVENTS_LIMIT)).unwrap_or(usize::MAX);
        let bytes = body.read(0, len).unwrap_or_default();
        let events = parse_events(&bytes);
        *parsed = Some((body.key().to_owned(), events, body.len() > EVENTS_LIMIT));
    }
    match parsed {
        Some((_, events, partial)) => (*partial, events),
        None => (false, &[]),
    }
}

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

/// The body's size and, when it came compressed, its size on the wire:
/// `12.0 KB (3.1 KB gzip)`.
pub fn size_text(received: &Received) -> String {
    let wire = received.head.timeline.iter().find_map(|e| match &e.kind {
        TimelineKind::Decompressed {
            encoding,
            wire_bytes,
        } => Some((encoding, *wire_bytes)),
        _ => None,
    });
    match wire {
        Some((encoding, bytes)) => format!(
            "{} ({} {encoding})",
            human_size(received.size),
            human_size(bytes)
        ),
        None => human_size(received.size),
    }
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
        TimelineKind::Dns {
            host,
            addresses,
            duration_us,
            overridden,
        } => {
            let how = if *overridden {
                strings.event_dns_override.to_owned()
            } else {
                millis(*duration_us)
            };
            format!(
                "{} {host} → {} ({how})",
                strings.event_dns,
                addresses.join(", ")
            )
        }
        TimelineKind::Connected {
            address,
            duration_us,
        } => format!(
            "{} {} ({})",
            strings.event_connected,
            address.as_deref().unwrap_or("?"),
            millis(*duration_us)
        ),
        TimelineKind::Reused { address } => format!(
            "{} {}",
            strings.event_reused,
            address.as_deref().unwrap_or("?")
        ),
        TimelineKind::Tls {
            version,
            duration_us,
        } => format!(
            "{} {} ({})",
            strings.event_tls,
            version.as_deref().unwrap_or("TLS"),
            millis(*duration_us)
        ),
        TimelineKind::Headers { status } => {
            format!("{}: {}", strings.event_headers, StatusBadge::text(*status))
        }
        TimelineKind::Body { bytes } => {
            format!("{}: {}", strings.event_body, human_size(*bytes))
        }
        TimelineKind::Decompressed {
            encoding,
            wire_bytes,
        } => format!(
            "{} {encoding}: {}",
            strings.event_decompressed,
            human_size(*wire_bytes)
        ),
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

/// "Tests 2/3", red when one failed, green when all passed.
fn tests_label(ui: &egui::Ui, received: &Received, strings: &Strings) -> RichText {
    let assertions = &received.checks.assertions;
    if assertions.is_empty() {
        return RichText::new(strings.tests);
    }
    let passed = assertions.iter().filter(|a| a.verdict.passed).count();
    let palette = theme::palette(ui);
    let color = if passed == assertions.len() {
        palette.success
    } else {
        palette.danger
    };
    RichText::new(format!("{} {passed}/{}", strings.tests, assertions.len())).color(color)
}

/// The assertions, failures first, then the captures.
fn tests(ui: &mut egui::Ui, received: &Received, strings: &Strings) {
    let palette = theme::palette(ui);
    let checks = &received.checks;
    if checks.assertions.is_empty() && checks.captures.is_empty() {
        ui.label(RichText::new(strings.tests_none).color(palette.text_muted));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("response-tests")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if !checks.assertions.is_empty() {
                ui.label(RichText::new(strings.assertions).strong());
                let mut ordered: Vec<_> = checks.assertions.iter().collect();
                ordered.sort_by_key(|a| a.verdict.passed);
                for outcome in ordered {
                    let (mark, color) = if outcome.verdict.passed {
                        (strings.pass, palette.success)
                    } else {
                        (strings.fail, palette.danger)
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(mark).color(color).strong().monospace());
                        ui.label(RichText::new(apifast_app::assertion_text(outcome)).monospace());
                    });
                    if let Some(message) = &outcome.verdict.message {
                        ui.indent("why", |ui| {
                            ui.label(RichText::new(message.as_str()).color(color));
                        });
                    }
                }
            }
            if !checks.captures.is_empty() {
                ui.add_space(theme::space::S);
                ui.label(RichText::new(strings.captures).strong());
                for capture in &checks.captures {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(capture.variable.as_str())
                                .monospace()
                                .strong(),
                        );
                        match &capture.value {
                            Ok(_) if capture.secret => {
                                ui.label(strings.captured_as);
                                ui.label(RichText::new(apifast_app::HIDDEN).monospace());
                            }
                            Ok(value) => {
                                ui.label(strings.captured_as);
                                ui.label(RichText::new(value.data.as_str()).monospace());
                            }
                            Err(reason) => {
                                ui.label(RichText::new(reason.as_str()).color(palette.warning));
                            }
                        }
                    });
                }
            }
        });
}
