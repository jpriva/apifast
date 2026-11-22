//! The messages of a WebSocket connection (docs/09): a list filtered by
//! text, one line each (when, which way, the data), and the one selected
//! whole, formatted when it is JSON. While the connection is open the list
//! follows its end.

use std::fmt::Write as _;

use apifast_domain::{WsData, WsEvent, WsLogEntry, WsSide};
use eframe::egui::{self, RichText, text::LayoutJob};

use crate::code::{self, Language};
use crate::strings::Strings;
use crate::theme;

/// Height of the list, in points; the selected message goes below.
const LIST_HEIGHT: f32 = 220.0;
/// Characters of a message shown in its line.
const LINE_CHARS: usize = 160;
/// Bytes of a binary message shown in its line, in hexadecimal.
const LINE_BYTES: usize = 64;

/// The list and what it remembers.
#[derive(Debug, Default)]
pub struct MessageList {
    filter: String,
    /// The number of the selected entry, counted from the log's start.
    selected: Option<usize>,
}

impl MessageList {
    /// Draws `entries`, the first of them number `first` (from 1); `live`
    /// while the connection is open.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        (entries, first): (&[WsLogEntry], usize),
        live: bool,
        strings: &Strings,
    ) {
        let muted = theme::palette(ui).text_muted;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(strings.ws_filter)
                    .desired_width(260.0),
            )
            .accessible_name(strings.ws_filter);
            let messages = (entries.iter())
                .filter(|e| matches!(e.event, WsEvent::Sent(_) | WsEvent::Received(_)))
                .count();
            let text = strings.count(&strings.ws_message_count, messages, &[]);
            ui.label(RichText::new(text).color(muted));
        });
        if entries.is_empty() {
            ui.label(RichText::new(strings.ws_waiting).color(muted));
            return;
        }
        let filter = self.filter.trim().to_lowercase();
        let shown: Vec<usize> = (0..entries.len())
            .filter(|&i| filter.is_empty() || matches(&entries[i], &filter, strings))
            .collect();
        if shown.is_empty() {
            ui.label(RichText::new(strings.ws_none_match).color(muted));
            return;
        }
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
        egui::ScrollArea::vertical()
            .id_salt("response-messages")
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .stick_to_bottom(live)
            .show_rows(ui, row_height, shown.len(), |ui, rows| {
                for &index in &shown[rows] {
                    let number = first + index;
                    let text = line(ui, number, &entries[index], strings);
                    let selected = self.selected == Some(number);
                    if ui.add(egui::Button::selectable(selected, text)).clicked() {
                        self.selected = (!selected).then_some(number);
                    }
                }
            });
        ui.separator();
        let picked = self
            .selected
            .and_then(|number| number.checked_sub(first))
            .and_then(|index| entries.get(index));
        match picked {
            Some(entry) => details(ui, entry, strings),
            None => {
                ui.label(RichText::new(strings.ws_pick).color(muted));
            }
        }
    }
}

/// Which way the entry went, as the list says it.
fn direction<'s>(event: &WsEvent, strings: &'s Strings) -> &'s str {
    match event {
        WsEvent::Sent(_) => strings.ws_sent,
        WsEvent::Received(_) => strings.ws_received,
        WsEvent::Failed(_) => strings.ws_failed,
        WsEvent::Closed {
            by: WsSide::Client, ..
        } => strings.ws_closed_by_client,
        WsEvent::Closed { .. } => strings.ws_closed_by_server,
    }
}

/// The entry's text on one line: the data, the error, or the close's code
/// and reason.
fn summary(event: &WsEvent, strings: &Strings) -> String {
    match event {
        WsEvent::Sent(data) | WsEvent::Received(data) => match data {
            WsData::Text(text) => text.clone(),
            WsData::Binary(bytes) => {
                let shown = &bytes[..bytes.len().min(LINE_BYTES)];
                format!("{} {} B  {}", strings.ws_binary, bytes.len(), hex(shown))
            }
        },
        WsEvent::Failed(why) => why.clone(),
        WsEvent::Closed { code, reason, .. } => {
            let code = code.map(|c| c.to_string()).unwrap_or_default();
            format!("{code} {reason}").trim().to_owned()
        }
    }
}

/// Whether `entry` has `filter` (lowercase) in its text or its direction.
fn matches(entry: &WsLogEntry, filter: &str, strings: &Strings) -> bool {
    summary(&entry.event, strings)
        .to_lowercase()
        .contains(filter)
        || direction(&entry.event, strings).contains(filter)
}

/// `#3  +12 ms  received  {"n": 3}`, in columns.
fn line(ui: &egui::Ui, number: usize, entry: &WsLogEntry, strings: &Strings) -> LayoutJob {
    let palette = theme::palette(ui);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = LayoutJob::default();
    let mut add = |text: &str, color| {
        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
    };
    add(&format!("{number:>5}  "), palette.text_muted);
    add(&format!("{:>8}  ", at(entry.at_ms)), palette.text_muted);
    let color = match entry.event {
        WsEvent::Sent(_) => palette.accent,
        WsEvent::Received(_) => palette.success,
        WsEvent::Failed(_) => palette.danger,
        WsEvent::Closed { .. } => palette.warning,
    };
    let way = direction(&entry.event, strings);
    add(&format!("{way:<8}  "), color);
    let text: String = summary(&entry.event, strings)
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .take(LINE_CHARS)
        .collect();
    add(&text, ui.visuals().text_color());
    job
}

/// The selected entry whole: text formatted when it is JSON, binary data in
/// hexadecimal.
fn details(ui: &mut egui::Ui, entry: &WsLogEntry, strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(direction(&entry.event, strings)).color(muted));
        ui.label(RichText::new(at(entry.at_ms)).monospace());
    });
    let text = match &entry.event {
        WsEvent::Sent(WsData::Text(text)) | WsEvent::Received(WsData::Text(text)) => pretty(text),
        WsEvent::Sent(WsData::Binary(bytes)) | WsEvent::Received(WsData::Binary(bytes)) => {
            hex(bytes)
        }
        other => summary(other, strings),
    };
    egui::ScrollArea::vertical()
        .id_salt("response-message-data")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
        });
}

/// When, since the connection opened: `+12 ms`, `+3.4 s`.
fn at(ms: u64) -> String {
    if ms < 10_000 {
        format!("+{ms} ms")
    } else {
        let tenths = ms / 100;
        format!("+{}.{} s", tenths / 10, tenths % 10)
    }
}

/// Bytes in hexadecimal, spaced.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, b| {
        if !hex.is_empty() {
            hex.push(' ');
        }
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// The text formatted when it is JSON, as it came otherwise.
fn pretty(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        code::format(text, Language::Json).unwrap_or_else(|_| text.to_owned())
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::EN;

    fn entry(event: WsEvent) -> WsLogEntry {
        WsLogEntry { at_ms: 0, event }
    }

    #[test]
    fn the_filter_looks_at_the_text_and_the_direction() {
        let hello = entry(WsEvent::Received(WsData::Text("Hello".into())));
        assert!(matches(&hello, "hell", &EN));
        assert!(matches(&hello, "received", &EN));
        assert!(!matches(&hello, "sent", &EN));
        let closed = entry(WsEvent::Closed {
            by: WsSide::Server,
            code: Some(4000),
            reason: "done".into(),
        });
        assert!(matches(&closed, "4000", &EN));
        assert_eq!(summary(&closed.event, &EN), "4000 done");
    }

    #[test]
    fn binary_data_is_shown_in_hexadecimal_and_times_grow_units() {
        let data = WsEvent::Sent(WsData::Binary(vec![0, 255, 16]));
        assert_eq!(summary(&data, &EN), "binary 3 B  00 ff 10");
        assert_eq!(at(12), "+12 ms");
        assert_eq!(at(12_345), "+12.3 s");
    }
}
