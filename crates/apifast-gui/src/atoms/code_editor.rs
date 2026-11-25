//! A multi-line editor for code, with its syntax colored and `{{variables}}`
//! highlighted on top, as in [`TemplateField`](super::TemplateField).
//!
//! Highlighting a long text on every key is slow in egui, so the layout is
//! kept until the text changes and texts over
//! [`HIGHLIGHT_LIMIT`](crate::code::HIGHLIGHT_LIMIT) are edited plain.
//!
//! What the Tab key inserts, whether lines wrap and whether their numbers
//! show come from the settings, through [`EditorOptions`].

mod completion;

pub use completion::Completer;

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
    if language == Language::JavaScript {
        return syntax_layout(text, language, font, palette, text_color);
    }
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

/// How every code editor behaves, as the settings say: set once a frame
/// with [`EditorOptions::set`], and read by each editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EditorOptions {
    /// What the Tab key inserts: a tab (0) or that many spaces.
    pub tab_size: u8,
    /// Long lines wrap; else they scroll sideways.
    pub wrap: bool,
    /// Line numbers show beside the text.
    pub line_numbers: bool,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            tab_size: 2,
            wrap: true,
            line_numbers: false,
        }
    }
}

impl EditorOptions {
    fn id() -> egui::Id {
        egui::Id::unique("apifast-editor-options")
    }

    /// Makes these the options of every editor in `ctx`.
    pub fn set(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }

    /// The options of the editors in `ctx`; the defaults if none were set.
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }
}

/// Turns a Tab pressed in the editor `id`, if it has the focus, into the
/// spaces of `options`; a tab stays one when it says 0. Shift+Tab still
/// takes indentation away.
fn tab_as_spaces(ui: &egui::Ui, id: egui::Id, options: EditorOptions) {
    if options.tab_size == 0 || !ui.memory(|m| m.has_focus(id)) {
        return;
    }
    let spaces = " ".repeat(usize::from(options.tab_size));
    ui.input_mut(|i| {
        for event in &mut i.events {
            if let egui::Event::Key {
                key: egui::Key::Tab,
                pressed: true,
                modifiers,
                ..
            } = event
                && modifiers.is_none()
            {
                *event = egui::Event::Text(spaces.clone());
            }
        }
    });
}

/// Draws the number of each line of `galley` (drawn at `galley_pos`) in
/// the gutter that ends at `right`: on the first row of each, not on the
/// rows a wrapped line goes on to.
fn paint_line_numbers(
    ui: &egui::Ui,
    galley: &Galley,
    galley_pos: egui::Pos2,
    right: f32,
    font: &FontId,
) {
    let color = theme::palette(ui).text_muted;
    let painter = ui.painter();
    let mut line = 1;
    let mut starts_line = true;
    for row in &galley.rows {
        if starts_line {
            painter.text(
                egui::pos2(right, galley_pos.y + row.pos.y),
                egui::Align2::RIGHT_TOP,
                line.to_string(),
                font.clone(),
                color,
            );
            line += 1;
        }
        starts_line = row.ends_with_newline;
    }
}

/// What lays out the text of an editor.
type Layouter<'a> = dyn FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<Galley> + 'a;

/// The text edit of an editor: `rows` high, laid out by `layouter` if
/// given, and scrolling sideways when it does not `wrap`.
fn text_edit(
    ui: &mut egui::Ui,
    text: &mut String,
    (id, rows, wrap): (egui::Id, usize, bool),
    layouter: Option<&mut Layouter<'_>>,
) -> egui::text_edit::TextEditOutput {
    let edit = egui::TextEdit::multiline(text)
        .id(id)
        .code_editor()
        .desired_rows(rows)
        .desired_width(f32::INFINITY);
    let edit = match layouter {
        Some(layouter) => edit.layouter(layouter),
        None => edit,
    };
    if wrap {
        edit.show(ui)
    } else {
        egui::ScrollArea::horizontal()
            .id_salt(id.with("sideways"))
            .auto_shrink([false, true])
            .show(ui, |ui| edit.show(ui))
            .inner
    }
}

/// `edit`, with a gutter of `width` before it for the line numbers, if
/// given.
fn with_gutter(
    ui: &mut egui::Ui,
    width: Option<f32>,
    font: &FontId,
    edit: impl FnOnce(&mut egui::Ui) -> egui::text_edit::TextEditOutput,
) -> egui::text_edit::TextEditOutput {
    let Some(width) = width else {
        return edit(ui);
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 0.0), egui::Sense::hover());
        let output = edit(ui);
        let right = rect.right() - theme::space::S;
        paint_line_numbers(ui, &output.galley, output.galley_pos, right, font);
        output
    })
    .inner
}

/// How wide the gutter for the line numbers of `text` is.
fn gutter_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    let lines = text.split('\n').count();
    let digits = lines.to_string().len().max(2);
    let digit = ui.fonts_mut(|f| f.glyph_width(font, '0'));
    #[expect(clippy::cast_precision_loss, reason = "a handful of digits")]
    let digits = digits as f32;
    digit * digits + theme::space::M
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
    completer: Option<Completer<'a>>,
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
            completer: None,
        }
    }

    /// Suggests what to type at the cursor, with `completer`.
    #[must_use]
    pub fn completer(mut self, completer: Completer<'a>) -> Self {
        self.completer = Some(completer);
        self
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
            completer,
        } = self;
        let id = id.unwrap_or_else(|| ui.make_persistent_id("code-editor"));
        let (picked, forced) = match completer {
            Some(_) => completion::before(ui, id, text),
            None => (false, false),
        };
        let options = EditorOptions::of(ui.ctx());
        tab_as_spaces(ui, id, options);
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
                palette.accent,
                hints_hash,
                language,
                (options.wrap, font.size.to_bits()),
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
            job.wrap.max_width = if options.wrap { wrap } else { f32::INFINITY };
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
        let gutter = options.line_numbers.then(|| gutter_width(ui, text, &font));
        let layouter: Option<&mut Layouter<'_>> = highlight.then_some(&mut layouter);
        let output = with_gutter(ui, gutter, &font, |ui| {
            text_edit(ui, text, (id, rows, options.wrap), layouter)
        });
        if let Some(completer) = completer {
            let typed = output.response.response.changed();
            completion::after(ui, (id, &output), text, (completer, typed, forced));
        }
        let mut response = output.response.response;
        if picked {
            response.mark_changed();
        }
        if let Some(name) = name {
            response.clone().accessible_name(name);
        }
        if highlight
            && language != Language::JavaScript
            && let Some(pointer) = response.hover_pos()
        {
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
