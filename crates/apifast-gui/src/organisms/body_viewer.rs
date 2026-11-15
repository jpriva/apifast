//! A read-only viewer for bodies of any size. It indexes the rows of the
//! body a few megabytes per frame, and each frame reads and draws only the
//! rows on screen, so the body is never held in memory.

use std::ops::Range;

use eframe::egui::{self, RichText, TextStyle};

use crate::body::{BodyHandle, Row, RowIndex};
use crate::strings::Strings;
use crate::theme;

/// Bytes read at a time while indexing.
const SCAN_CHUNK: usize = 1024 * 1024;
/// Bytes indexed per frame, at most: a few milliseconds of work.
const SCAN_PER_FRAME: usize = 8 * SCAN_CHUNK;

/// The rows on screen and their bytes, kept while the view does not move.
#[derive(Debug)]
struct Window {
    rows: Range<usize>,
    /// The rows found in `bytes`.
    found: Vec<Row>,
    offset: u64,
    bytes: Vec<u8>,
}

/// The viewer and what it knows of the body it shows.
#[derive(Debug, Default)]
pub struct BodyViewer {
    /// Key of the body indexed, to start again when another is shown.
    key: Option<String>,
    index: RowIndex,
    window: Option<Window>,
    error: Option<String>,
    /// A row to bring to the top on the next frame.
    scroll_to: Option<usize>,
}

impl BodyViewer {
    /// Whether the whole body has been indexed.
    pub fn indexed(&self, body: &BodyHandle) -> bool {
        self.key.as_deref() == Some(body.key()) && self.index.scanned() >= body.len()
    }

    /// Rows found so far.
    pub fn rows(&self) -> usize {
        self.index.rows()
    }

    /// Brings `row` to the top of the view on the next frame.
    pub fn scroll_to_row(&mut self, row: usize) {
        self.scroll_to = Some(row);
    }

    /// Draws `body`, filling the space available.
    pub fn show(&mut self, ui: &mut egui::Ui, body: &BodyHandle, strings: &Strings) {
        if self.key.as_deref() != Some(body.key()) {
            *self = Self {
                key: Some(body.key().to_owned()),
                scroll_to: self.scroll_to,
                ..Self::default()
            };
        }
        self.scan(body);
        let muted = theme::palette(ui).text_muted;
        if let Some(error) = &self.error {
            let message = format!("{} {error}", strings.body_unreadable);
            ui.label(RichText::new(message).color(theme::palette(ui).danger));
        } else if !self.indexed(body) {
            // Still reading: the rows found so far can already be seen.
            ui.ctx().request_repaint();
            let percent = self.index.scanned().saturating_mul(100) / body.len().max(1);
            ui.label(RichText::new(format!("{} {percent}%", strings.indexing)).color(muted));
        }
        let row_height = ui.text_style_height(&TextStyle::Monospace);
        let digits = self.rows().to_string().len();
        let mut area = egui::ScrollArea::both()
            .id_salt("body-viewer")
            .auto_shrink([false, false]);
        if let Some(row) = self.scroll_to.take() {
            let spacing = ui.spacing().item_spacing.y;
            #[allow(clippy::cast_precision_loss, reason = "a pixel offset")]
            let offset = row.min(self.rows()) as f32 * (row_height + spacing);
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, row_height, self.rows(), |ui, rows| {
            self.draw(ui, body, rows, row_height, digits, muted);
        });
    }

    /// Indexes the next part of the body, up to the budget of a frame.
    fn scan(&mut self, body: &BodyHandle) {
        let mut budget = SCAN_PER_FRAME;
        while self.error.is_none() && self.index.scanned() < body.len() && budget > 0 {
            match body.read(self.index.scanned(), SCAN_CHUNK.min(budget)) {
                // A body that shrank: stop where it ends.
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => {
                    budget = budget.saturating_sub(chunk.len());
                    self.index.feed(&chunk);
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        body: &BodyHandle,
        rows: Range<usize>,
        row_height: f32,
        digits: usize,
        muted: egui::Color32,
    ) {
        let window = match self.window.take() {
            // Indexing may still add rows: a window is kept only while it
            // has all of its own.
            Some(window) if window.rows == rows && window.found.len() == rows.len() => window,
            _ => {
                let span = self.index.span(&rows);
                let len = usize::try_from(span.end - span.start).unwrap_or(usize::MAX);
                let bytes = body.read(span.start, len).unwrap_or_else(|error| {
                    self.error = Some(error);
                    Vec::new()
                });
                Window {
                    found: self.index.split(&rows, &bytes),
                    rows,
                    offset: span.start,
                    bytes,
                }
            }
        };
        ui.spacing_mut().item_spacing.x = theme::space::M;
        for row in &window.found {
            let start = usize::try_from(row.range.start - window.offset).unwrap_or(usize::MAX);
            let end = usize::try_from(row.range.end - window.offset).unwrap_or(usize::MAX);
            let bytes = window.bytes.get(start..end).unwrap_or_default();
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
            let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
            let line = row.line.map_or_else(String::new, |line| line.to_string());
            ui.horizontal(|ui| {
                ui.set_height(row_height);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{line:>digits$}"))
                            .monospace()
                            .color(muted),
                    )
                    .selectable(false),
                );
                // An empty label has nothing to name for screen readers.
                if !bytes.is_empty() {
                    let text = String::from_utf8_lossy(bytes);
                    ui.add(egui::Label::new(RichText::new(text).monospace()).extend());
                }
            });
        }
        self.window = Some(window);
    }
}
