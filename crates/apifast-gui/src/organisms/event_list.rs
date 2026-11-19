//! The events of an event stream (docs/09): a list filtered by name, id or
//! data, one line each, and the data of the one selected, formatted when it
//! is JSON. While the stream arrives the list follows its end.

use apifast_domain::SseEvent;
use eframe::egui::{self, RichText, text::LayoutJob};

use crate::code::{self, Language};
use crate::strings::Strings;
use crate::theme;

/// Height of the list, in points; the selected event's data goes below.
const LIST_HEIGHT: f32 = 220.0;
/// Characters of an event's data shown in its line.
const LINE_CHARS: usize = 160;
/// Widest event name column, in characters.
const NAME_CHARS: usize = 16;

/// The list and what it remembers.
#[derive(Debug, Default)]
pub struct EventList {
    filter: String,
    /// The number of the selected event, counted from the stream's start.
    selected: Option<usize>,
}

impl EventList {
    /// Draws `events`, the first of them number `first` (from 1); `live`
    /// while the stream arrives.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        (events, first): (&[SseEvent], usize),
        live: bool,
        strings: &Strings,
    ) {
        let muted = theme::palette(ui).text_muted;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(strings.events_filter)
                    .desired_width(260.0),
            )
            .accessible_name(strings.events_filter);
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    first + events.len() - 1,
                    strings.events.to_lowercase()
                ))
                .color(muted),
            );
        });
        if events.is_empty() {
            ui.label(RichText::new(strings.events_waiting).color(muted));
            return;
        }
        let filter = self.filter.trim().to_lowercase();
        let shown: Vec<usize> = (0..events.len())
            .filter(|&i| filter.is_empty() || matches(&events[i], &filter))
            .collect();
        if shown.is_empty() {
            ui.label(RichText::new(strings.events_none_match).color(muted));
            return;
        }
        let width = (events.iter().map(|e| e.event.chars().count()).max())
            .unwrap_or(0)
            .min(NAME_CHARS);
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
        egui::ScrollArea::vertical()
            .id_salt("response-events")
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .stick_to_bottom(live)
            .show_rows(ui, row_height, shown.len(), |ui, rows| {
                for &index in &shown[rows] {
                    let number = first + index;
                    let event = &events[index];
                    let text = line(ui, number, event, width);
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
            .and_then(|index| events.get(index));
        match picked {
            Some(event) => details(ui, event, strings),
            None => {
                ui.label(RichText::new(strings.event_pick).color(muted));
            }
        }
    }
}

/// Whether `event` has `filter` (lowercase) in its name, id or data.
fn matches(event: &SseEvent, filter: &str) -> bool {
    event.event.to_lowercase().contains(filter)
        || event
            .id
            .as_ref()
            .is_some_and(|id| id.to_lowercase().contains(filter))
        || event.data.to_lowercase().contains(filter)
}

/// `#3  tick  id 7  {"n": 3}`, in columns.
fn line(ui: &egui::Ui, number: usize, event: &SseEvent, width: usize) -> LayoutJob {
    let palette = theme::palette(ui);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = LayoutJob::default();
    let mut add = |text: &str, color| {
        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
    };
    add(&format!("{number:>5}  "), palette.text_muted);
    let name: String = event.event.chars().take(width).collect();
    add(&format!("{name:<width$}  "), palette.accent);
    if let Some(id) = &event.id {
        add(&format!("{id}  "), palette.text_muted);
    }
    let data: String = event
        .data
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .take(LINE_CHARS)
        .collect();
    add(&data, ui.visuals().text_color());
    job
}

/// The selected event: name, id and retry, then its data.
fn details(ui: &mut egui::Ui, event: &SseEvent, strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(strings.event_name).color(muted));
        ui.label(RichText::new(&event.event).monospace());
        if let Some(id) = &event.id {
            ui.label(RichText::new(strings.event_id).color(muted));
            ui.label(RichText::new(id).monospace());
        }
        if let Some(retry) = event.retry {
            ui.label(RichText::new(strings.event_retry).color(muted));
            ui.label(RichText::new(format!("{retry} ms")).monospace());
        }
    });
    let data = pretty(&event.data);
    egui::ScrollArea::vertical()
        .id_salt("response-event-data")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(data).monospace()).selectable(true));
        });
}

/// The data formatted when it is JSON, as it came otherwise.
fn pretty(data: &str) -> String {
    let trimmed = data.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        code::format(data, Language::Json).unwrap_or_else(|_| data.to_owned())
    } else {
        data.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, data: &str, id: Option<&str>) -> SseEvent {
        SseEvent {
            event: name.into(),
            data: data.into(),
            id: id.map(Into::into),
            retry: None,
        }
    }

    #[test]
    fn the_filter_looks_at_name_id_and_data() {
        let tick = event("tick", "{\"n\": 1}", Some("A7"));
        assert!(matches(&tick, "tic"));
        assert!(matches(&tick, "a7"));
        assert!(matches(&tick, "\"n\""));
        assert!(!matches(&tick, "tock"));
    }

    #[test]
    fn json_data_is_formatted_and_other_data_kept() {
        assert_eq!(pretty("{\"n\":1}"), "{\n  \"n\": 1\n}");
        assert_eq!(pretty("{broken"), "{broken");
        assert_eq!(pretty("plain text"), "plain text");
    }
}
