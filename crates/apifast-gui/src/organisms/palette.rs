//! The command palette: one search for actions, requests and environments.
//! Fuzzy matching, recent picks first, each action's shortcut on the right.

use apifast_app::RequestPath;
use apifast_domain::Method;
use eframe::egui::{self, Key, RichText};

use crate::actions::Action;
use crate::atoms::MethodBadge;
use crate::strings::Strings;
use crate::theme;

/// Rows shown at once.
const ROWS: usize = 12;
/// Picks remembered as recent.
const RECENT: usize = 20;
/// Width of the palette, in points.
const WIDTH: f32 = 560.0;

/// Something the palette can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteItem {
    /// An action, with its shortcut written out.
    Action(Action, String, Option<String>),
    /// A request: its path, name and method.
    Request(RequestPath, String, Method),
    /// An environment to make active, or none.
    Environment(Option<String>, String),
}

impl PaletteItem {
    /// The text it is searched by.
    fn text(&self) -> String {
        match self {
            Self::Action(_, label, _) | Self::Environment(_, label) => label.clone(),
            Self::Request(path, name, _) => format!("{name} {}", path.as_str()),
        }
    }

    /// What tells it apart from every other item, for the recent list.
    fn key(&self) -> String {
        match self {
            Self::Action(action, ..) => format!("action:{}", action.id()),
            Self::Request(path, ..) => format!("request:{}", path.as_str()),
            Self::Environment(name, _) => format!("environment:{}", name.as_deref().unwrap_or("")),
        }
    }
}

/// How well `query` matches `text`: its characters in order, ignoring case.
/// Higher is better; `None` if they are not all there. Consecutive
/// characters and the starts of words score more, and shorter texts win
/// ties.
pub fn fuzzy(query: &str, text: &str) -> Option<i64> {
    let query: Vec<char> = query
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut at = 0;
    let mut previous: Option<usize> = None;
    for wanted in query {
        let found = (at..chars.len()).find(|&i| chars[i] == wanted)?;
        score += 1;
        if previous.is_some_and(|p| p + 1 == found) {
            score += 5;
        }
        let word_start = found == 0 || !chars[found - 1].is_alphanumeric();
        if word_start {
            score += 8;
        }
        previous = Some(found);
        at = found + 1;
    }
    let length = i64::try_from(chars.len()).unwrap_or(i64::MAX);
    Some(score * 100 - length)
}

/// What the palette searches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PaletteMode {
    /// Everything.
    #[default]
    All,
    /// Requests only.
    Requests,
}

/// The palette's state.
#[derive(Debug, Default)]
pub struct CommandPalette {
    /// Whether it is open, and what it searches.
    pub open: Option<PaletteMode>,
    query: String,
    selected: usize,
    /// Keys of recent picks, most recent first.
    recent: Vec<String>,
    /// Whether the field still needs the focus, just after opening.
    focus: bool,
}

impl CommandPalette {
    /// Opens it on `mode`, empty.
    pub fn open(&mut self, mode: PaletteMode) {
        self.open = Some(mode);
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }

    /// The items that match the query, best first.
    pub fn matches<'a>(&self, items: &'a [PaletteItem]) -> Vec<&'a PaletteItem> {
        let mode = self.open.unwrap_or_default();
        let recent = |item: &PaletteItem| {
            let key = item.key();
            self.recent
                .iter()
                .position(|k| *k == key)
                .unwrap_or(usize::MAX)
        };
        let mut found: Vec<(i64, usize, usize, &PaletteItem)> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                mode == PaletteMode::All || matches!(item, PaletteItem::Request(..))
            })
            .filter_map(|(order, item)| {
                Some((fuzzy(&self.query, &item.text())?, recent(item), order, item))
            })
            .collect();
        // Better match first; then recent first; then as given.
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        if self.query.trim().is_empty() {
            found.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));
        }
        found.into_iter().map(|(.., item)| item).collect()
    }

    /// Draws the palette, if open. Returns the item picked.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        items: &[PaletteItem],
        strings: &Strings,
    ) -> Option<PaletteItem> {
        let mode = self.open?;
        let found: Vec<PaletteItem> = self.matches(items).into_iter().cloned().collect();
        // Keys first, before the field takes them.
        let (up, down, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
            )
        });
        let last = found.len().saturating_sub(1);
        if down {
            self.selected = (self.selected + 1).min(last);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        self.selected = self.selected.min(last);
        let mut picked = enter.then(|| found.get(self.selected).cloned()).flatten();

        let modal = egui::Modal::new(egui::Id::unique("command-palette")).show(ctx, |ui| {
            ui.set_width(WIDTH);
            let hint = match mode {
                PaletteMode::All => strings.palette_hint,
                PaletteMode::Requests => strings.palette_requests_hint,
            };
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name(hint);
            if self.focus {
                field.request_focus();
                self.focus = false;
            }
            if field.changed() {
                self.selected = 0;
            }
            ui.separator();
            if found.is_empty() {
                ui.label(
                    RichText::new(strings.palette_nothing).color(theme::palette(ui).text_muted),
                );
            }
            // A window of rows around the selection.
            let start = self.selected.saturating_sub(ROWS - 1);
            for (index, item) in found.iter().enumerate().skip(start).take(ROWS) {
                if row(ui, item, index == self.selected, strings).clicked() {
                    picked = Some(item.clone());
                }
            }
        });
        if picked.is_some() || modal.should_close() {
            self.open = None;
        }
        if let Some(item) = &picked {
            let key = item.key();
            self.recent.retain(|k| *k != key);
            self.recent.insert(0, key);
            self.recent.truncate(RECENT);
        }
        picked
    }
}

fn row(ui: &mut egui::Ui, item: &PaletteItem, selected: bool, strings: &Strings) -> egui::Response {
    let muted = theme::palette(ui).text_muted;
    ui.horizontal(|ui| {
        let response = match item {
            PaletteItem::Action(_, label, shortcut) => {
                let response = ui.selectable_label(selected, label.as_str());
                if let Some(shortcut) = shortcut {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(shortcut.as_str()).monospace().color(muted));
                    });
                }
                response
            }
            PaletteItem::Request(path, name, method) => {
                ui.add(MethodBadge::new(method));
                let response = ui.selectable_label(selected, name.as_str());
                ui.label(RichText::new(path.as_str()).color(muted).small());
                response
            }
            PaletteItem::Environment(_, label) => {
                ui.label(RichText::new(strings.palette_environment).color(muted));
                ui.selectable_label(selected, label.as_str())
            }
        };
        if selected {
            response.scroll_to_me(None);
        }
        response
    })
    .inner
}
