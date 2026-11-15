//! A one-line field for template text, with `{{variables}}` highlighted:
//! defined ones on a tint of their color, missing ones underlined, values
//! made when sending in their own color. Hovering a variable shows its
//! value and where it comes from.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use apifast_template::{Expr, Template, Token};
use eframe::egui::{self, FontId, RichText, Stroke, TextFormat, TextStyle, text::LayoutJob};

use crate::atoms::{completions, refocus, suggestions};
use crate::theme;

/// What the field shows when a defined variable is hovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableHint {
    /// The value to show: the value itself, or a placeholder for secrets.
    pub value: String,
    /// Where the value comes from, ready to show.
    pub origin: String,
}

/// Defined variables by name, as the field shows them.
pub type VariableHints = BTreeMap<String, VariableHint>;

/// How a part of the text is highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// Plain text, or an escaped `\{{`.
    Text,
    /// A defined `{{variable}}`.
    Defined,
    /// A `{{variable}}` that is not defined.
    Missing,
    /// A tag whose value is made when sending.
    Generated,
    /// Something that looks like a tag but is not valid.
    Invalid,
}

/// A part of the text: its byte range, how it is highlighted and, for a
/// variable, its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Bytes of the text it covers.
    pub range: Range<usize>,
    /// How it is highlighted.
    pub kind: SpanKind,
    /// The variable's name, for `{{name}}` tags.
    pub variable: Option<String>,
}

/// Splits `text` into highlighted spans. Pure, so it is tested on its own.
pub fn spans(text: &str, hints: &VariableHints) -> Vec<Span> {
    let template = Template::parse(text);
    let mut at = 0;
    let mut out = Vec::with_capacity(template.tokens().len());
    for token in template.tokens() {
        let range = at..at + token.source_len();
        at = range.end;
        let (kind, variable) = match token {
            Token::Text(_) | Token::Escape => (SpanKind::Text, None),
            Token::Invalid(_) => (SpanKind::Invalid, None),
            Token::Tag(tag) => match tag.expr() {
                Expr::Variable(name) => {
                    let kind = if hints.contains_key(name) {
                        SpanKind::Defined
                    } else {
                        SpanKind::Missing
                    };
                    (kind, Some(name.clone()))
                }
                _ => (SpanKind::Generated, None),
            },
        };
        out.push(Span {
            range,
            kind,
            variable,
        });
    }
    out
}

/// Texts the field shows in its hover.
#[derive(Debug, Clone, Copy)]
pub struct TemplateFieldTexts<'a> {
    /// Placeholder when empty.
    pub placeholder: &'a str,
    /// Shown for a variable that is not defined.
    pub missing: &'a str,
    /// Shown for a tag whose value is made when sending.
    pub generated: &'a str,
}

/// The field. Edits `text` in place.
pub struct TemplateField<'a> {
    text: &'a mut String,
    hints: &'a VariableHints,
    texts: TemplateFieldTexts<'a>,
    width: f32,
    id: Option<egui::Id>,
    password: bool,
    name: Option<&'a str>,
}

impl<'a> TemplateField<'a> {
    /// A field over `text`, highlighting with `hints`.
    pub fn new(
        text: &'a mut String,
        hints: &'a VariableHints,
        texts: TemplateFieldTexts<'a>,
    ) -> Self {
        Self {
            text,
            hints,
            texts,
            width: f32::INFINITY,
            id: None,
            password: false,
            name: None,
        }
    }

    /// Names the field for screen readers, when no visible label does.
    #[must_use]
    pub fn accessible_name(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    /// A fixed id, so the field keeps its focus when the rows around it
    /// change (a table that adds a row while typing).
    #[must_use]
    pub fn id(mut self, id: egui::Id) -> Self {
        self.id = Some(id);
        self
    }

    /// Hides the text, for passwords.
    #[must_use]
    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    /// The width it takes, at most the space available.
    #[must_use]
    pub fn desired_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Draws the field. Returns its response; `changed()` tells if the user
    /// edited the text.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let Self {
            text,
            hints,
            texts,
            width,
            id,
            password,
            name,
        } = self;
        let font = TextStyle::Monospace.resolve(ui.style());
        let palette = theme::palette(ui);
        // Laid out every frame: a URL is short, and egui caches the galley
        // of an unchanged layout job. A long editor would memoize this.
        let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = layout_job(
                buffer.as_str(),
                hints,
                &font,
                palette,
                ui.visuals().text_color(),
            );
            job.wrap.max_width = wrap;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let mut edit = egui::TextEdit::singleline(text)
            .hint_text(texts.placeholder)
            .desired_width(width.min(ui.available_width()));
        if let Some(id) = id {
            edit = edit.id(id);
        }
        // A hidden text is drawn as dots: highlighting it would show its
        // shape, and hovering would show the value.
        let output = if password {
            edit.password(true).show(ui)
        } else {
            edit.layouter(&mut layouter).show(ui)
        };
        if let Some(name) = name {
            output.response.response.clone().accessible_name(name);
        }
        let mut response = output.response.response.clone();
        if password {
            return response;
        }
        // Completes `{{na` with the variables it can be.
        let cursor = output.state.cursor.char_range().map(|r| r.primary.index.0);
        if let Some(prefix) = cursor.and_then(|at| variable_prefix(text, at)) {
            let names = completions(prefix, hints.keys().map(String::as_str));
            if let (Some(index), Some(at)) = (suggestions(ui, &response, text, &names), cursor) {
                let (completed, at) = complete_variable(text, at, names[index]);
                *text = completed;
                refocus(ui, &response, at);
                response.mark_changed();
            }
        }
        if let Some(pointer) = response.hover_pos() {
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

/// The start of a variable name being typed at character `cursor`: the
/// text after an unclosed `{{` up to the cursor, if it can be a name.
pub fn variable_prefix(text: &str, cursor: usize) -> Option<&str> {
    let byte = byte_at(text, cursor);
    let before = &text[..byte];
    let open = before.rfind("{{")?;
    if before[..open].ends_with('\\') {
        return None; // `\{{` is not a tag.
    }
    let typed = before[open + 2..].trim_start();
    typed
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .then_some(typed)
}

/// `text` with the name being typed at character `cursor` completed to
/// `name` and the tag closed, and the character the cursor goes to: after
/// the `}}`.
pub fn complete_variable(text: &str, cursor: usize, name: &str) -> (String, usize) {
    let byte = byte_at(text, cursor);
    let typed = variable_prefix(text, cursor).unwrap_or_default();
    let start = byte - typed.len();
    let after = &text[byte..];
    // Keep a `}}` that is already there, and what follows it.
    let rest =
        after.trim_start_matches(|c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    let rest = rest.strip_prefix("}}").unwrap_or(rest);
    let mut out = String::with_capacity(text.len() + name.len() + 2);
    out.push_str(&text[..start]);
    out.push_str(name);
    out.push_str("}}");
    let cursor = out.chars().count();
    out.push_str(rest);
    (out, cursor)
}

fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(byte, _)| byte)
}

/// The variable or generated tag under character `index`, if any.
pub(crate) fn span_at(text: &str, index: usize, hints: &VariableHints) -> Option<Span> {
    let byte = text
        .char_indices()
        .nth(index)
        .map_or(text.len(), |(byte, _)| byte);
    spans(text, hints).into_iter().find(|span| {
        span.range.contains(&byte) && !matches!(span.kind, SpanKind::Text | SpanKind::Invalid)
    })
}

pub(crate) fn hover(
    ui: &mut egui::Ui,
    span: &Span,
    hints: &VariableHints,
    texts: TemplateFieldTexts<'_>,
) {
    let palette = theme::palette(ui);
    let Some(name) = &span.variable else {
        ui.label(texts.generated);
        return;
    };
    ui.label(RichText::new(name.as_str()).monospace().strong());
    match hints.get(name) {
        Some(hint) => {
            ui.label(RichText::new(hint.value.as_str()).monospace());
            ui.label(RichText::new(hint.origin.as_str()).color(palette.text_muted));
        }
        None => {
            ui.label(RichText::new(texts.missing).color(palette.variable_missing));
        }
    }
}

fn layout_job(
    text: &str,
    hints: &VariableHints,
    font: &FontId,
    palette: &theme::Palette,
    text_color: egui::Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans(text, hints) {
        let plain = TextFormat::simple(font.clone(), text_color);
        job.append(
            &text[span.range],
            0.0,
            variable_format(span.kind, plain, palette),
        );
    }
    job
}
/// `plain` as a span of `kind` is drawn.
pub(crate) fn variable_format(
    kind: SpanKind,
    plain: TextFormat,
    palette: &theme::Palette,
) -> TextFormat {
    match kind {
        SpanKind::Text => plain,
        SpanKind::Defined => TextFormat {
            color: palette.variable,
            background: palette.variable.gamma_multiply(theme::BADGE_TINT),
            ..plain
        },
        // Underlined too: color is never the only signal.
        SpanKind::Missing => TextFormat {
            color: palette.variable_missing,
            underline: Stroke::new(1.0, palette.variable_missing),
            ..plain
        },
        SpanKind::Generated => TextFormat {
            color: palette.variable_generated,
            ..plain
        },
        SpanKind::Invalid => TextFormat {
            color: palette.variable_missing,
            italics: true,
            ..plain
        },
    }
}

/// Keeps a built [`VariableHints`] until its source changes, so screens do
/// not rebuild it every frame.
#[derive(Debug, Default)]
pub struct HintsCache {
    key: Option<u64>,
    hints: Arc<VariableHints>,
}

impl HintsCache {
    /// The hints for `key`, built with `build` only when `key` changed.
    pub fn get(&mut self, key: u64, build: impl FnOnce() -> VariableHints) -> Arc<VariableHints> {
        if self.key != Some(key) {
            self.hints = Arc::new(build());
            self.key = Some(key);
        }
        self.hints.clone()
    }
}
