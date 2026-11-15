//! Splits code into tokens to color. The lexers never fail: text they do
//! not understand is plain.

use std::ops::Range;

use super::{Language, tag_len};

/// What a token is, which gives its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Anything else: spaces, names, text between XML tags.
    Plain,
    /// A JSON key or an XML attribute name.
    Key,
    /// A string or an XML attribute value.
    String,
    /// A number.
    Number,
    /// `true`, `false`, `null`, an XML tag name or a SPARQL keyword.
    Keyword,
    /// A SPARQL `?variable`.
    Variable,
    /// A comment.
    Comment,
    /// Brackets, commas, colons and the `<`, `>` of XML tags.
    Punctuation,
}

/// A token: the bytes it covers and its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Bytes of the text it covers.
    pub range: Range<usize>,
    /// What it is.
    pub kind: TokenKind,
}

/// The tokens of `text` in `language`. They cover the whole text, in order,
/// with neighbors of one kind merged.
pub fn tokens(text: &str, language: Language) -> Vec<Token> {
    let mut out = Tokens::default();
    match language {
        Language::Json => json(text, &mut out),
        Language::Xml => xml(text, &mut out),
        Language::Sparql => sparql(text, &mut out),
        Language::Text => out.push(0..text.len(), TokenKind::Plain),
    }
    out.0
}

#[derive(Default)]
struct Tokens(Vec<Token>);

impl Tokens {
    fn push(&mut self, range: Range<usize>, kind: TokenKind) {
        if range.is_empty() {
            return;
        }
        if let Some(last) = self.0.last_mut()
            && last.kind == kind
            && last.range.end == range.start
        {
            last.range.end = range.end;
            return;
        }
        self.0.push(Token { range, kind });
    }
}

/// Byte length of the char at the start of `rest`.
fn char_len(rest: &str) -> usize {
    rest.chars().next().map_or(1, char::len_utf8)
}

/// Length of the quoted string at the start of `rest`, closed by the same
/// quote; unclosed strings end at the end of the line.
fn string_len(rest: &str) -> usize {
    let quote = rest.as_bytes()[0];
    let mut escaped = false;
    for (i, b) in rest.bytes().enumerate().skip(1) {
        match b {
            b'\n' => return i,
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            _ if b == quote => return i + 1,
            _ => {}
        }
    }
    rest.len()
}

/// Length of the run at the start of `rest` whose chars pass `keep`.
fn run_len(rest: &str, keep: impl Fn(char) -> bool) -> usize {
    rest.find(|c: char| !keep(c)).unwrap_or(rest.len())
}

fn json(text: &str, out: &mut Tokens) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.as_bytes()[0];
        let (len, kind) = if let Some(len) = tag_len(rest) {
            (len, TokenKind::Plain)
        } else {
            match c {
                b'"' => {
                    let len = string_len(rest);
                    // A string followed by `:` is a key.
                    let after = rest[len..].trim_start();
                    let kind = if after.starts_with(':') {
                        TokenKind::Key
                    } else {
                        TokenKind::String
                    };
                    (len, kind)
                }
                b'{' | b'}' | b'[' | b']' | b',' | b':' => (1, TokenKind::Punctuation),
                b'-' | b'0'..=b'9' => (
                    1 + run_len(&rest[1..], |c| {
                        c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')
                    }),
                    TokenKind::Number,
                ),
                _ if c.is_ascii_alphabetic() => {
                    let len = run_len(rest, |c| c.is_ascii_alphanumeric());
                    let kind = if matches!(&rest[..len], "true" | "false" | "null") {
                        TokenKind::Keyword
                    } else {
                        TokenKind::Plain
                    };
                    (len, kind)
                }
                _ => (char_len(rest), TokenKind::Plain),
            }
        };
        out.push(at..at + len, kind);
        at += len;
    }
}

fn xml(text: &str, out: &mut Tokens) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("<!--") {
            let len = rest.find("-->").map_or(rest.len(), |end| end + 3);
            out.push(at..at + len, TokenKind::Comment);
            at += len;
        } else if rest.starts_with("<![CDATA[") {
            let len = rest.find("]]>").map_or(rest.len(), |end| end + 3);
            out.push(at..at + len, TokenKind::String);
            at += len;
        } else if rest.starts_with('<') {
            at = xml_tag(text, at, out);
        } else {
            // Text up to the next tag; entities stand out.
            let len = rest.find('<').unwrap_or(rest.len());
            let mut start = at;
            let end = at + len;
            while let Some(amp) = text[start..end].find('&') {
                let amp = start + amp;
                out.push(start..amp, TokenKind::Plain);
                let entity = text[amp..end].find(';').filter(|&n| {
                    n > 1
                        && text[amp + 1..amp + n]
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '#')
                });
                let entity_len = entity.map_or(1, |n| n + 1);
                let kind = if entity.is_some() {
                    TokenKind::Keyword
                } else {
                    TokenKind::Plain
                };
                out.push(amp..amp + entity_len, kind);
                start = amp + entity_len;
            }
            out.push(start..end, TokenKind::Plain);
            at = end;
        }
    }
}

/// The tag starting at `at`, which is a `<`. Returns where it ends.
fn xml_tag(text: &str, mut at: usize, out: &mut Tokens) -> usize {
    let open = 1 + run_len(&text[at + 1..], |c| matches!(c, '/' | '?' | '!'));
    out.push(at..at + open, TokenKind::Punctuation);
    at += open;
    let name = run_len(&text[at..], is_name_char);
    out.push(at..at + name, TokenKind::Keyword);
    at += name;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.as_bytes()[0];
        let (len, kind) = match c {
            b'>' => {
                out.push(at..at + 1, TokenKind::Punctuation);
                return at + 1;
            }
            b'/' | b'?' if rest[1..].starts_with('>') => {
                out.push(at..at + 2, TokenKind::Punctuation);
                return at + 2;
            }
            // A new tag before this one closed: leave it to the caller.
            b'<' => return at,
            b'"' | b'\'' => (string_len(rest), TokenKind::String),
            b'=' => (1, TokenKind::Punctuation),
            _ if is_name_char(char::from(c)) => (run_len(rest, is_name_char), TokenKind::Key),
            _ => (char_len(rest), TokenKind::Plain),
        };
        out.push(at..at + len, kind);
        at += len;
    }
    at
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}

/// SPARQL keywords, upper case; they are matched in any case.
const SPARQL_KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "AS",
    "ASC",
    "ASK",
    "BASE",
    "BIND",
    "BY",
    "CLEAR",
    "CONSTRUCT",
    "COPY",
    "CREATE",
    "DATA",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DROP",
    "EXISTS",
    "FILTER",
    "FROM",
    "GRAPH",
    "GROUP",
    "HAVING",
    "IN",
    "INSERT",
    "INTO",
    "LIMIT",
    "LOAD",
    "MINUS",
    "MOVE",
    "NAMED",
    "NOT",
    "OFFSET",
    "OPTIONAL",
    "ORDER",
    "PREFIX",
    "REDUCED",
    "SELECT",
    "SERVICE",
    "SILENT",
    "TO",
    "UNION",
    "USING",
    "VALUES",
    "WHERE",
    "WITH",
];

fn sparql(text: &str, out: &mut Tokens) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.as_bytes()[0];
        let (len, kind) = if let Some(len) = tag_len(rest) {
            (len, TokenKind::Plain)
        } else {
            match c {
                b'#' => (rest.find('\n').unwrap_or(rest.len()), TokenKind::Comment),
                b'"' | b'\'' => (string_len(rest), TokenKind::String),
                b'<' => {
                    // An IRI has no spaces; otherwise it is `<`, less than.
                    let iri = rest
                        .find(['>', ' ', '\n'])
                        .filter(|&n| rest.as_bytes()[n] == b'>');
                    match iri {
                        Some(end) => (end + 1, TokenKind::String),
                        None => (1, TokenKind::Punctuation),
                    }
                }
                b'?' | b'$' => (
                    1 + run_len(&rest[1..], |c| c.is_alphanumeric() || c == '_'),
                    TokenKind::Variable,
                ),
                b'0'..=b'9' => (
                    run_len(rest, |c| c.is_ascii_digit() || c == '.'),
                    TokenKind::Number,
                ),
                b'{' | b'}' | b'(' | b')' | b'.' | b';' | b',' => (1, TokenKind::Punctuation),
                _ if c.is_ascii_alphabetic() => {
                    let len = run_len(rest, |c| c.is_alphanumeric() || matches!(c, '_' | '-'));
                    let word = &rest[..len];
                    // `rdf:type`: a prefix, not a keyword.
                    let prefixed = rest[len..].starts_with(':');
                    let keyword = !prefixed
                        && (word == "a"
                            || SPARQL_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word)));
                    let kind = if keyword {
                        TokenKind::Keyword
                    } else {
                        TokenKind::Plain
                    };
                    (len, kind)
                }
                _ => (char_len(rest), TokenKind::Plain),
            }
        };
        out.push(at..at + len, kind);
        at += len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, language: Language) -> Vec<(&str, TokenKind)> {
        tokens(text, language)
            .into_iter()
            .map(|t| (&text[t.range], t.kind))
            .collect()
    }

    #[test]
    fn tokens_cover_the_whole_text_in_order() {
        for (text, language) in [
            (r#"{"a": [1, true, "x\"y"], "é": {{id}}}"#, Language::Json),
            ("<a b=\"1\">x &amp; y<!-- c --></a>", Language::Xml),
            ("SELECT ?s WHERE { ?s a <http://x> } # c", Language::Sparql),
            ("hola {{x}}", Language::Text),
            ("\"unclosed\n{{open\n<", Language::Json),
            ("<a b=\"unclosed\n<c", Language::Xml),
        ] {
            let mut at = 0;
            for token in tokens(text, language) {
                assert_eq!(token.range.start, at, "{text}");
                at = token.range.end;
            }
            assert_eq!(at, text.len(), "{text}");
        }
    }

    #[test]
    fn json_tells_keys_from_strings() {
        use TokenKind::*;
        assert_eq!(
            kinds(r#"{"id": "x", "n": -1.5e3, "ok": null}"#, Language::Json),
            [
                ("{", Punctuation),
                (r#""id""#, Key),
                (":", Punctuation),
                (" ", Plain),
                (r#""x""#, String),
                (",", Punctuation),
                (" ", Plain),
                (r#""n""#, Key),
                (":", Punctuation),
                (" ", Plain),
                ("-1.5e3", Number),
                (",", Punctuation),
                (" ", Plain),
                (r#""ok""#, Key),
                (":", Punctuation),
                (" ", Plain),
                ("null", Keyword),
                ("}", Punctuation),
            ]
        );
    }

    #[test]
    fn a_template_tag_in_json_is_not_punctuation() {
        let found = kinds(r#"{"id": {{id}}}"#, Language::Json);
        // Plain, merged with the space before it.
        assert!(found.contains(&(" {{id}}", TokenKind::Plain)), "{found:?}");
    }

    #[test]
    fn xml_tags_attributes_and_entities() {
        use TokenKind::*;
        assert_eq!(
            kinds("<a id='1'>x&lt;</a>", Language::Xml),
            [
                ("<", Punctuation),
                ("a", Keyword),
                (" ", Plain),
                ("id", Key),
                ("=", Punctuation),
                ("'1'", String),
                (">", Punctuation),
                ("x", Plain),
                ("&lt;", Keyword),
                ("</", Punctuation),
                ("a", Keyword),
                (">", Punctuation),
            ]
        );
    }

    #[test]
    fn sparql_keywords_variables_and_prefixes() {
        let found = kinds("select ?name { ?s foaf:name ?name }", Language::Sparql);
        assert!(found.contains(&("select", TokenKind::Keyword)));
        assert!(found.contains(&("?name", TokenKind::Variable)));
        assert!(
            found
                .iter()
                .all(|(text, kind)| !text.contains("foaf") || *kind == TokenKind::Plain)
        );
    }
}
