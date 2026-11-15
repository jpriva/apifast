//! A multi-line editor for code, with its syntax colored and `{{variables}}`
//! highlighted on top, as in [`TemplateField`](super::TemplateField).
//!
//! Highlighting a long text on every key is slow in egui, so the layout is
//! kept until the text changes and texts over
//! [`HIGHLIGHT_LIMIT`](crate::code::HIGHLIGHT_LIMIT) are edited plain.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, FontId, Galley, TextFormat, TextStyle, text::LayoutJob};

use super::template_field::{hover, span_at, variable_format};
use super::{SpanKind, TemplateFieldTexts, VariableHints, spans};
use crate::code::{HIGHLIGHT_LIMIT, Language, TokenKind, tokens};
use crate::theme;

/// The highlighted layout of `text`: its syntax colored, with variables
/// drawn over it as in a template field.
pub fn code_layout(
    text: &str,
    language: Language,
    hints: &VariableHints,
    font: &FontId,
    palette: &theme::Palette,
    text_color: egui::Color32,
) -> LayoutJob {
    let syntax = tokens(text, language);
    let variables = spans(text, hints);
    let mut job = LayoutJob::default();
    let (mut syntax, mut variables) = (syntax.iter().peekable(), variables.iter().peekable());
    let mut at = 0;
    // Both lists cover the whole text in order: walk them together and cut
    // where either changes.
    while at < text.len() {
        while syntax.next_if(|t| t.range.end <= at).is_some() {}
        while variables.next_if(|s| s.range.end <= at).is_some() {}
        let (Some(token), Some(span)) = (syntax.peek(), variables.peek()) else {
            break;
        };
        let end = token.range.end.min(span.range.end);
        let plain = TextFormat::simple(font.clone(), text_color);
        let format = if span.kind == SpanKind::Text {
            syntax_format(token.kind, plain, palette)
        } else {
            variable_format(span.kind, plain, palette)
        };
        job.append(&text[at..end], 0.0, format);
        at = end;
    }
    job
}

/// The layout of `text` with only its syntax colored: for responses, where
/// `{{...}}` is not a variable.
pub fn syntax_layout(
    text: &str,
    language: Language,
    font: &FontId,
    palette: &theme::Palette,
    text_color: egui::Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for token in tokens(text, language) {
        let plain = TextFormat::simple(font.clone(), text_color);
        job.append(
            &text[token.range],
            0.0,
            syntax_format(token.kind, plain, palette),
        );
    }
    job
}

fn syntax_format(kind: TokenKind, plain: TextFormat, palette: &theme::Palette) -> TextFormat {
    let color = match kind {
        TokenKind::Plain => return plain,
        TokenKind::Key | TokenKind::Variable => palette.syntax_key,
        TokenKind::String => palette.syntax_string,
        TokenKind::Number => palette.syntax_number,
        TokenKind::Keyword => palette.syntax_keyword,
        TokenKind::Punctuation => palette.text_muted,
        TokenKind::Comment => {
            return TextFormat {
                color: palette.text_muted,
                italics: true,
                ..plain
            };
        }
    };
    TextFormat { color, ..plain }
}

/// The last layout, kept while its text, width and theme stay the same.
#[derive(Clone)]
struct Highlighted {
    key: u64,
    galley: Arc<Galley>,
}

/// The editor. Edits `text` in place.
pub struct CodeEditor<'a> {
    text: &'a mut String,
    language: Language,
    hints: &'a VariableHints,
    texts: TemplateFieldTexts<'a>,
    id: Option<egui::Id>,
    rows: usize,
    name: Option<&'a str>,
}

impl<'a> CodeEditor<'a> {
    /// An editor over `text` in `language`, highlighting variables with
    /// `hints`.
    pub fn new(
        text: &'a mut String,
        language: Language,
        hints: &'a VariableHints,
        texts: TemplateFieldTexts<'a>,
    ) -> Self {
        Self {
            text,
            language,
            hints,
            texts,
            id: None,
            rows: 12,
            name: None,
        }
    }

    /// A fixed id, so others can tell whether it has the focus.
    #[must_use]
    pub fn id(mut self, id: egui::Id) -> Self {
        self.id = Some(id);
        self
    }

    /// Rows it shows before growing.
    #[must_use]
    pub fn desired_rows(mut self, rows: usize) -> Self {
        self.rows = rows;
        self
    }

    /// Names it for screen readers.
    #[must_use]
    pub fn accessible_name(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    /// Draws the editor. Returns its response; `changed()` tells if the
    /// user edited the text.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let Self {
            text,
            language,
            hints,
            texts,
            id,
            rows,
            name,
        } = self;
        let id = id.unwrap_or_else(|| ui.make_persistent_id("code-editor"));
        let font = TextStyle::Monospace.resolve(ui.style());
        let palette = theme::palette(ui);
        let highlight = text.len() <= HIGHLIGHT_LIMIT;
        let cache = id.with("highlight");
        let mut hints_hash = DefaultHasher::new();
        for name in hints.keys() {
            name.hash(&mut hints_hash);
        }
        let hints_hash = hints_hash.finish();
        let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
            let text = buffer.as_str();
            let mut key = DefaultHasher::new();
            (
                text,
                wrap.to_bits(),
                ui.visuals().dark_mode,
                hints_hash,
                language,
            )
                .hash(&mut key);
            let key = key.finish();
            if let Some(kept) = ui
                .data(|d| d.get_temp::<Highlighted>(cache))
                .filter(|kept| kept.key == key)
            {
                return kept.galley;
            }
            let mut job = code_layout(
                text,
                language,
                hints,
                &font,
                palette,
                ui.visuals().text_color(),
            );
            job.wrap.max_width = wrap;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            ui.data_mut(|d| {
                d.insert_temp(
                    cache,
                    Highlighted {
                        key,
                        galley: galley.clone(),
                    },
                );
            });
            galley
        };
        let edit = egui::TextEdit::multiline(text)
            .id(id)
            .code_editor()
            .desired_rows(rows)
            .desired_width(f32::INFINITY);
        let output = if highlight {
            edit.layouter(&mut layouter).show(ui)
        } else {
            edit.show(ui)
        };
        let response = output.response.response;
        if let Some(name) = name {
            response.clone().accessible_name(name);
        }
        if highlight && let Some(pointer) = response.hover_pos() {
            let index = output
                .galley
                .cursor_from_pos(pointer - output.galley_pos)
                .index
                .0;
            if let Some(span) = span_at(text, index, hints) {
                response
                    .clone()
                    .on_hover_ui_at_pointer(|ui| hover(ui, &span, hints, texts));
            }
        }
        response
    }
}
