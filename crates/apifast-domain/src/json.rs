//! A small JSON reader: what assertions, captures and object variables need
//! (docs/08), without `serde` in the domain.
//!
//! Numbers keep their text, so `12345678901234567890` is shown as written
//! and compared as a number. Nesting deeper than [`MAX_DEPTH`] is refused
//! instead of overflowing the stack on a hostile body.

use std::fmt::{self, Write as _};

/// How deep arrays and objects may nest.
pub const MAX_DEPTH: usize = 128;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written.
    Number(String),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, its keys in order.
    Object(Vec<(String, Json)>),
}

/// Why a text is not JSON.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not JSON: {reason} at byte {at}")]
pub struct JsonError {
    /// What is wrong.
    pub reason: &'static str,
    /// Where.
    pub at: usize,
}

/// Why a path cannot be followed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid path `{0}`")]
pub struct PathError(pub String);

impl Json {
    /// Reads a JSON text, with optional whitespace around it.
    ///
    /// # Errors
    ///
    /// If it is not exactly one JSON value, or nests too deep.
    pub fn parse(text: &str) -> Result<Self, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            text,
            at: 0,
        };
        parser.space();
        let value = parser.value(0)?;
        parser.space();
        if parser.at < parser.bytes.len() {
            return Err(parser.error("text after the value"));
        }
        Ok(value)
    }

    /// The value of `key`, if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value at `path`: keys separated by dots and indexes in brackets,
    /// as in `items[0].name`; `["a.b"]` names a key with dots. An empty path
    /// is the value itself. `None` when something on the way is missing.
    ///
    /// # Errors
    ///
    /// If the path is malformed.
    pub fn at(&self, path: &str) -> Result<Option<&Self>, PathError> {
        let mut current = self;
        for step in steps(path)? {
            let next = match (&step, current) {
                (Step::Key(key), _) => current.get(key),
                (Step::Index(i), Self::Array(items)) => items.get(*i),
                (Step::Index(_), _) => None,
            };
            match next {
                Some(value) => current = value,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// Its number, if it is one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// The name of its type, for messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Its text: a string as it is, anything else as compact JSON. This is
    /// what a captured value becomes in a variable.
    pub fn to_text(&self) -> String {
        match self {
            Self::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

/// Compact JSON.
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Number(text) => f.write_str(text),
            Self::String(text) => quote(f, text),
            Self::Array(items) => {
                f.write_char('[')?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_char(',')?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_char(']')
            }
            Self::Object(pairs) => {
                f.write_char('{')?;
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        f.write_char(',')?;
                    }
                    quote(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_char('}')
            }
        }
    }
}

fn quote(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_char('"')?;
    for c in text.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if u32::from(c) < 0x20 => write!(f, "\\u{:04x}", u32::from(c))?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

#[derive(Debug, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
}

fn steps(path: &str) -> Result<Vec<Step>, PathError> {
    let bad = || PathError(path.to_owned());
    let mut out = Vec::new();
    let mut rest = path;
    let mut expect_key = true;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('[') {
            let end = inner.find(']').ok_or_else(bad)?;
            let content = &inner[..end];
            let quoted = content
                .strip_prefix('"')
                .and_then(|c| c.strip_suffix('"'))
                .or_else(|| {
                    content
                        .strip_prefix('\'')
                        .and_then(|c| c.strip_suffix('\''))
                });
            out.push(match quoted {
                Some(key) => Step::Key(key.to_owned()),
                None => Step::Index(content.trim().parse().map_err(|_| bad())?),
            });
            rest = &inner[end + 1..];
            expect_key = false;
        } else if let Some(after) = rest.strip_prefix('.') {
            if expect_key || after.is_empty() {
                return Err(bad());
            }
            rest = after;
            expect_key = true;
        } else {
            if !expect_key {
                return Err(bad());
            }
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            out.push(Step::Key(rest[..end].to_owned()));
            rest = &rest[end..];
            expect_key = false;
        }
    }
    Ok(out)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn error(&self, reason: &'static str) -> JsonError {
        JsonError {
            reason,
            at: self.at,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        match self.peek() {
            None => Err(self.error("unexpected end")),
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'[' | b'{') if depth >= MAX_DEPTH => Err(self.error("nested too deep")),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let first = self.peek();
        let whole = self.digits();
        if whole == 0 || (first == Some(b'0') && whole > 1) {
            return Err(self.error("invalid number"));
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.error("invalid number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("invalid number"));
            }
        }
        Ok(Json::Number(self.text[start..self.at].to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let hex = self
            .text
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.error("invalid escape"))?;
        let code = u32::from_str_radix(hex, 16).map_err(|_| self.error("invalid escape"))?;
        self.at += 4;
        Ok(code)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.peek(), None | Some(b'"' | b'\\' | 0..0x20)) {
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = self.peek().ok_or_else(|| self.error("unexpected end"))?;
                    self.at += 1;
                    out.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return Err(self.error("invalid escape")),
                    });
                }
                None => return Err(self.error("unterminated string")),
                Some(_) => return Err(self.error("control character in string")),
            }
        }
    }

    /// The character of a `\u` escape, joining a surrogate pair; a lone
    /// surrogate becomes U+FFFD, as browsers do.
    fn unicode(&mut self) -> Result<char, JsonError> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) && self.bytes[self.at..].starts_with(b"\\u") {
            let save = self.at;
            self.at += 2;
            let low = self.hex4()?;
            if (0xDC00..0xE000).contains(&low) {
                let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return Ok(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            self.at = save;
        }
        Ok(char::from_u32(high).unwrap_or('\u{FFFD}'))
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut pairs = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.error("expected `:`"));
            }
            self.at += 1;
            self.space();
            pairs.push((key, self.value(depth + 1)?));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(pairs));
                }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    #[test]
    fn values_are_read_and_written_back() {
        let text = r#"{"a":[1,-2.5e3,true,null],"b":{"c":"x\"y\n\u00e9\ud83d\ude00"},"0":12345678901234567890}"#;
        let value = json(&format!(" \n{text}\t"));
        assert_eq!(
            value.to_string(),
            text.replace(r"\u00e9", "é").replace(r"\ud83d\ude00", "😀")
        );
        assert_eq!(
            value.get("0"),
            Some(&Json::Number("12345678901234567890".into()))
        );
        assert_eq!(json("\"\\ud800\"").to_text(), "\u{FFFD}");
        assert_eq!(json("[]"), Json::Array(Vec::new()));
        assert_eq!(json("{ }"), Json::Object(Vec::new()));
    }

    #[test]
    fn invalid_texts_are_refused() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "-",
            "1e",
            "tru",
            "\"a",
            "\"\u{1}\"",
            "\"\\x\"",
            "1 2",
            "{a:1}",
            "[1 2]",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(Json::parse(&deep).unwrap_err().reason, "nested too deep");
        let fits = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(Json::parse(&fits).is_ok());
    }

    #[test]
    fn paths_reach_keys_and_indexes() {
        let value =
            json(r#"{"items":[{"id":7,"tags":["a"]}],"a.b":1,"user":{"address":{"city":"Lima"}}}"#);
        let at = |path| value.at(path).unwrap().map(Json::to_text);
        assert_eq!(at("items[0].id").as_deref(), Some("7"));
        assert_eq!(at("items[0].tags[0]").as_deref(), Some("a"));
        assert_eq!(at("user.address.city").as_deref(), Some("Lima"));
        assert_eq!(at("[\"a.b\"]").as_deref(), Some("1"));
        assert_eq!(at("items[0]").as_deref(), Some(r#"{"id":7,"tags":["a"]}"#));
        assert_eq!(at("").as_deref(), Some(value.to_string().as_str()));
        assert_eq!(at("items[3]"), None);
        assert_eq!(at("user.name"), None);
        assert_eq!(at("user.address.city.x"), None);
        for bad in ["a..b", ".a", "a.", "items[x]", "items[0", "items[0]id"] {
            assert!(value.at(bad).is_err(), "{bad}");
        }
    }
}
