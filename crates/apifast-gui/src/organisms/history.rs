//! The history of sends: newest first, with a filter. Picking an entry shows
//! its response in the request's tab.

use apifast_app::{HistoryEntry, HistoryOutcome};
use apifast_domain::{Method, RequestKind, is_grpc_response, is_websocket_handshake};
use eframe::egui::{self, RichText};

use crate::atoms::{MethodBadge, StatusBadge};
use crate::strings::Strings;
use crate::theme;

/// The history panel and its filter.
#[derive(Debug, Default)]
pub struct HistoryView {
    /// Text that entries must contain, in their request, method or URL.
    pub filter: String,
}

/// How long ago `at_ms` was, at `now_ms`: `12 s ago`, `5 min ago`,
/// `3 h ago`, `2 d ago`.
pub fn ago(now_ms: i64, at_ms: i64, strings: &Strings) -> String {
    let seconds = now_ms.saturating_sub(at_ms).max(0) / 1000;
    let (n, template) = match seconds {
        0..60 => (seconds, strings.seconds_ago),
        60..3600 => (seconds / 60, strings.minutes_ago),
        3600..86_400 => (seconds / 3600, strings.hours_ago),
        _ => (seconds / 86_400, strings.days_ago),
    };
    crate::strings::fill(template, &[("n", &n)])
}

/// Whether `entry` matches `filter`, ignoring case.
pub fn matches(entry: &HistoryEntry, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || [entry.request.as_str(), &entry.method, &entry.url]
            .iter()
            .any(|field| field.to_lowercase().contains(&filter))
}

impl HistoryView {
    /// Draws `entries` (oldest first, as kept) newest first, `now_ms` being
    /// the current time. Returns the index of the entry picked.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        entries: &[HistoryEntry],
        now_ms: i64,
        strings: &Strings,
    ) -> Option<usize> {
        let muted = theme::palette(ui).text_muted;
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(strings.filter)
                .desired_width(f32::INFINITY),
        )
        .accessible_name(strings.filter_history);
        if entries.is_empty() {
            ui.label(RichText::new(strings.history_empty).color(muted));
            return None;
        }
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("history")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let shown = entries
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, entry)| matches(entry, &self.filter));
                for (index, entry) in shown {
                    if row(ui, entry, now_ms, strings) {
                        picked = Some(index);
                    }
                }
            });
        picked
    }
}

/// What the entry was: a WebSocket connection is told by its handshake,
/// a gRPC call by its content type, or, when it failed, by the URL's
/// scheme.
fn kind(entry: &HistoryEntry) -> RequestKind {
    let url = entry.url.trim_start().to_ascii_lowercase();
    let scheme = |schemes: &[&str]| schemes.iter().any(|s| url.starts_with(s));
    match &entry.outcome {
        HistoryOutcome::Received { head, .. } if is_grpc_response(&head.headers) => {
            RequestKind::Grpc
        }
        HistoryOutcome::Received { head, .. }
            if is_websocket_handshake(head.status, &head.headers) =>
        {
            RequestKind::WebSocket
        }
        HistoryOutcome::Failed(_) if scheme(&["ws://", "wss://"]) => RequestKind::WebSocket,
        HistoryOutcome::Failed(_) if scheme(&["grpc://", "grpcs://"]) => RequestKind::Grpc,
        _ => RequestKind::Http,
    }
}

/// One entry: method, status and request, then when and how long. Returns
/// whether it was clicked.
fn row(ui: &mut egui::Ui, entry: &HistoryEntry, now_ms: i64, strings: &Strings) -> bool {
    let palette = theme::palette(ui);
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add(MethodBadge::new(&Method::parse(&entry.method)).kind(kind(entry)));
        let (status, elapsed) = match &entry.outcome {
            HistoryOutcome::Received { head, .. } => {
                ui.add(StatusBadge::new(head.status));
                (None, Some(head.elapsed_ms))
            }
            HistoryOutcome::Failed(message) => (Some(message.as_str()), None),
        };
        if status.is_some() {
            ui.label(RichText::new(strings.failed).color(palette.danger));
        }
        let response = ui
            .selectable_label(false, entry.request.as_str())
            .on_hover_ui(|ui| {
                ui.label(RichText::new(entry.url.as_str()).monospace());
                if let Some(message) = status {
                    ui.label(RichText::new(message).color(palette.danger));
                }
            });
        clicked = response.clicked();
        let mut when = ago(now_ms, entry.at_ms, strings);
        if let Some(ms) = elapsed {
            when = format!("{when}, {ms} ms");
        }
        ui.label(RichText::new(when).color(palette.text_muted).small());
    });
    clicked
}
