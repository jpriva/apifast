//! Code in bodies: the languages the editor knows, how their text is split
//! into highlighted tokens and how it is formatted. Pure, so it is tested
//! without a window.
//!
//! Both work on text with `{{variables}}` in it, which is not valid JSON or
//! XML: a tag is kept as it is wherever it appears.

mod format;
mod graphql_complete;
mod highlight;

pub use format::{FormatError, format};
pub use graphql_complete::{Completion, CompletionItem, complete};
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
    /// GraphQL operations.
    GraphQl,
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
            Body::GraphQl { .. } => Some(Self::GraphQl),
            Body::Text(_) => Some(Self::Text),
            _ => None,
        }
    }

    /// Whether [`format()`](fn@format) can format it.
    pub fn formats(self) -> bool {
        matches!(self, Self::Json | Self::Xml)
    }
}

/// The language of a response: from its `Content-Type`, or else from how
/// its body starts. `None` for plain text or what cannot be told.
pub fn detect(content_type: Option<&str>, start: &[u8]) -> Option<Language> {
    if let Some(content_type) = content_type {
        let essence = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if essence.ends_with("json") || essence.ends_with("+json") {
            return Some(Language::Json);
        }
        if essence.ends_with("xml") || essence == "text/html" {
            return Some(Language::Xml);
        }
        if essence == "application/sparql-query" {
            return Some(Language::Sparql);
        }
        if essence == "application/graphql" {
            return Some(Language::GraphQl);
        }
        if essence.starts_with("text/") {
            return None;
        }
    }
    match start.iter().find(|b| !b.is_ascii_whitespace()) {
        Some(b'{' | b'[') => Some(Language::Json),
        Some(b'<') => Some(Language::Xml),
        _ => None,
    }
}

/// Whether bytes look like binary data rather than text: a NUL byte, or
/// many bytes that are neither text nor UTF-8.
pub fn looks_binary(start: &[u8]) -> bool {
    if start.contains(&0) {
        return true;
    }
    // The end may cut a character in two: that is not a sign of binary.
    let text = String::from_utf8_lossy(start);
    let replaced = text.matches('\u{FFFD}').count();
    replaced > 4 && replaced * 20 > start.len().min(text.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_type_tells_the_language_and_else_the_body() {
        let cases = [
            (
                Some("application/json; charset=utf-8"),
                "x",
                Some(Language::Json),
            ),
            (Some("application/problem+json"), "", Some(Language::Json)),
            (Some("text/xml"), "", Some(Language::Xml)),
            (Some("text/html"), "", Some(Language::Xml)),
            (Some("text/plain"), "{}", None),
            (None, "  [1]", Some(Language::Json)),
            (None, "<a/>", Some(Language::Xml)),
            (Some("application/octet-stream"), "{}", Some(Language::Json)),
            (None, "hello", None),
        ];
        for (content_type, body, expected) in cases {
            assert_eq!(
                detect(content_type, body.as_bytes()),
                expected,
                "{content_type:?}"
            );
        }
    }

    #[test]
    fn binary_is_told_from_text() {
        assert!(looks_binary(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(!looks_binary("héllo wörld".as_bytes()));
        // A character cut at the end of the window is still text.
        assert!(!looks_binary(&"é".as_bytes()[..1]));
        assert!(looks_binary(&[0xff; 64]));
    }
}
