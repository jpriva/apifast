//! The history of sends: newest first, with a filter. Picking an entry shows
//! its response in the request's tab; its menu renames it (a named entry is
//! kept whatever the limits) or deletes it, and the whole history can be
//! cleared. The response panel lists a request's entries the same way.

use apifast_app::{HistoryEntry, HistoryOutcome};
use apifast_domain::{Method, RequestKind, is_grpc_response, is_websocket_handshake};
use eframe::egui::{self, RichText};

use crate::atoms::{MethodBadge, StatusBadge};
use crate::strings::Strings;
use crate::theme;

/// What the user did in a history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// Picked entry number `.0` of the list given: show its response.
    Open(usize),
    /// Named entry `id`; an empty name takes it away.
    Rename {
        /// The entry's id.
        id: String,
        /// Its new name.
        name: String,
    },
    /// Delete entry `id`.
    Remove(String),
    /// Clear the whole history, once confirmed.
    Clear,
}

/// The history panel and its filter.
#[derive(Debug, Default)]
pub struct HistoryView {
    /// Text that entries must contain, in their name, request, method or
    /// URL.
    pub filter: String,
    /// The entry being renamed, by id, and the name typed so far.
    renaming: Option<(String, String)>,
    /// The question before clearing the history is open.
    confirm_clear: bool,
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

/// Whether `entry` matches `filter`, ignoring case: in its name, request,
/// method or URL.
pub fn matches(entry: &HistoryEntry, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    let name = entry.name().unwrap_or_default();
    filter.is_empty()
        || [name, entry.request.as_str(), &entry.method, &entry.url]
            .iter()
            .any(|field| field.to_lowercase().contains(&filter))
}

impl HistoryView {
    /// Draws `entries` (oldest first, as kept) newest first, `now_ms` being
    /// the current time, with a filter and a button that clears them.
    /// Returns what the user did; [`HistoryEvent::Open`] gives an index of
    /// `entries`.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        entries: &[HistoryEntry],
        now_ms: i64,
        strings: &Strings,
    ) -> Option<HistoryEvent> {
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
        let mut event = None;
        if ui.small_button(strings.history_clear).clicked() {
            self.confirm_clear = true;
        }
        if self.confirm_clear && self.ask_to_clear(ui.ctx(), strings) {
            event = Some(HistoryEvent::Clear);
        }
        egui::ScrollArea::vertical()
            .id_salt("history")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let filter = self.filter.clone();
                let shown =
                    (entries.iter().enumerate().rev()).filter(|(_, entry)| matches(entry, &filter));
                event = event.take().or(self.list(ui, shown, now_ms, strings));
            });
        event
    }

    /// Draws the rows of `shown`, each with its index, in that order.
    /// Returns what the user did.
    pub fn list<'e>(
        &mut self,
        ui: &mut egui::Ui,
        shown: impl Iterator<Item = (usize, &'e HistoryEntry)>,
        now_ms: i64,
        strings: &Strings,
    ) -> Option<HistoryEvent> {
        let mut event = None;
        for (index, entry) in shown {
            if let Some(done) = self.row(ui, (index, entry), now_ms, strings) {
                event = Some(done);
            }
        }
        event
    }

    /// Asks before clearing the history. Returns whether the user agreed.
    fn ask_to_clear(&mut self, ctx: &egui::Context, strings: &Strings) -> bool {
        let modal = egui::Modal::new(egui::Id::unique("confirm-clear-history")).show(ctx, |ui| {
            ui.label(RichText::new(strings.history_clear_title).strong());
            ui.label(strings.history_clear_warning);
            ui.add_space(theme::space::M);
            ui.horizontal(|ui| {
                if ui.button(strings.history_clear_confirm).clicked() {
                    Some(true)
                } else if ui.button(strings.cancel).clicked() {
                    Some(false)
                } else {
                    None
                }
            })
            .inner
        });
        let choice = match modal.inner {
            None if modal.should_close() => Some(false),
            choice => choice,
        };
        if choice.is_some() {
            self.confirm_clear = false;
        }
        choice == Some(true)
    }

    /// One entry: method, status, its name or request, then when and how
    /// long; or, while it is renamed, the field for its name. Returns what
    /// the user did with it.
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        (index, entry): (usize, &HistoryEntry),
        now_ms: i64,
        strings: &Strings,
    ) -> Option<HistoryEvent> {
        if let Some((id, name)) = &mut self.renaming
            && *id == entry.id
        {
            let field = ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text(strings.history_name_hint)
                    .desired_width(f32::INFINITY),
            );
            let field = field.accessible_name(strings.history_name_hint);
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            if field.lost_focus() {
                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let (id, name) = self.renaming.take()?;
                return (!cancelled).then_some(HistoryEvent::Rename { id, name });
            }
            return None;
        }
        let palette = theme::palette(ui);
        let mut event = None;
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
            let title = match entry.name() {
                Some(name) => RichText::new(name).strong(),
                None => RichText::new(entry.request.as_str()),
            };
            let response = ui.selectable_label(false, title).on_hover_ui(|ui| {
                if entry.name().is_some() {
                    ui.label(entry.request.as_str());
                    ui.label(RichText::new(strings.history_named).color(palette.text_muted));
                }
                ui.label(RichText::new(entry.url.as_str()).monospace());
                if let Some(message) = status {
                    ui.label(RichText::new(message).color(palette.danger));
                }
            });
            if response.clicked() {
                event = Some(HistoryEvent::Open(index));
            }
            response.context_menu(|ui| {
                if ui.button(strings.history_rename).clicked() {
                    let name = entry.name().unwrap_or_default().to_owned();
                    self.renaming = Some((entry.id.clone(), name));
                    ui.close();
                }
                if ui.button(strings.delete).clicked() {
                    event = Some(HistoryEvent::Remove(entry.id.clone()));
                    ui.close();
                }
            });
            let mut when = ago(now_ms, entry.at_ms, strings);
            if let Some(ms) = elapsed {
                when = format!("{when}, {ms} ms");
            }
            ui.label(RichText::new(when).color(palette.text_muted).small());
        });
        event
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
