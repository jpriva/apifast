//! Code in bodies: the languages the editor knows, how their text is split
//! into highlighted tokens and how it is formatted. Pure, so it is tested
//! without a window.
//!
//! Both work on text with `{{variables}}` in it, which is not valid JSON or
//! XML: a tag is kept as it is wherever it appears.

mod format;
mod highlight;

pub use format::{FormatError, format};
pub use highlight::{Token, TokenKind, tokens};

use apifast_domain::Body;

/// Bodies larger than this, in bytes, are edited without highlighting:
/// with more text, egui takes too long on every key (spike 2).
pub const HIGHLIGHT_LIMIT: usize = 200 * 1024;

/// The language of a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// JSON.
    Json,
    /// XML, and HTML close enough to it.
    Xml,
    /// SPARQL queries.
    Sparql,
    /// Plain text: only variables are highlighted.
    Text,
}

impl Language {
    /// The language of a body's text, if it has text.
    pub fn of(body: &Body) -> Option<Self> {
        match body {
            Body::Json(_) => Some(Self::Json),
            Body::Xml(_) => Some(Self::Xml),
            Body::Sparql(_) => Some(Self::Sparql),
            Body::Text(_) => Some(Self::Text),
            _ => None,
        }
    }

    /// Whether [`format`] can format it.
    pub fn formats(self) -> bool {
        matches!(self, Self::Json | Self::Xml)
    }
}

/// The 1-based line of byte `offset` in `text`.
pub fn line_of(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text.get(..offset).unwrap_or(text).matches('\n').count() + 1
}

/// The length in bytes of a `{{...}}` tag starting at `rest`, if one does.
/// An unclosed tag runs to the end of the line, so the rest of the text is
/// not taken as part of it.
fn tag_len(rest: &str) -> Option<usize> {
    if !rest.starts_with("{{") {
        return None;
    }
    let line = rest.find('\n').unwrap_or(rest.len());
    Some(rest[..line].find("}}").map_or(line, |end| end + 2))
}
