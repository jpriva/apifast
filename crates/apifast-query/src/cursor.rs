//! Reading an expression character by character, shared by both languages.

use crate::QueryError;

/// How deep parentheses, filters and calls may nest in an expression.
pub(crate) const MAX_NESTING: usize = 64;

pub(crate) struct Cursor<'a> {
    pub(crate) text: &'a str,
    pub(crate) at: usize,
    language: &'static str,
    depth: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(text: &'a str, language: &'static str) -> Self {
        Self {
            text,
            at: 0,
            language,
            depth: 0,
        }
    }

    pub(crate) fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(crate) fn peek_second(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    pub(crate) fn at_end(&self) -> bool {
        self.at >= self.text.len()
    }

    pub(crate) fn space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    /// Takes `token` if the text continues with it.
    pub(crate) fn eat(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.at += token.len();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, token: &str) -> Result<(), QueryError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{token}`")))
        }
    }

    /// Takes characters while `keep` holds.
    pub(crate) fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let start = self.at;
        while self.peek().is_some_and(&keep) {
            self.bump();
        }
        &self.text[start..self.at]
    }

    pub(crate) fn error(&self, reason: impl Into<String>) -> QueryError {
        QueryError::Syntax {
            language: self.language,
            expression: self.text.to_owned(),
            reason: reason.into(),
            at: self.text[..self.at.min(self.text.len())].chars().count() + 1,
        }
    }

    /// Enters a nested expression, refusing to go deeper than
    /// [`MAX_NESTING`].
    pub(crate) fn enter(&mut self) -> Result<(), QueryError> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            Err(self.error("nested too deep"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

    /// A number as written: `-?digits(.digits)?(e[+-]?digits)?`, or `None`
    /// if there is none here.
    pub(crate) fn number(&mut self) -> Option<&'a str> {
        let start = self.at;
        self.eat("-");
        if self.take_while(|c| c.is_ascii_digit()).is_empty() {
            self.at = start;
            return None;
        }
        let before_fraction = self.at;
        if self.eat(".") && self.take_while(|c| c.is_ascii_digit()).is_empty() {
            self.at = before_fraction;
        }
        let before_exponent = self.at;
        if self.eat("e") || self.eat("E") {
            if !self.eat("+") {
                self.eat("-");
            }
            if self.take_while(|c| c.is_ascii_digit()).is_empty() {
                self.at = before_exponent;
            }
        }
        Some(&self.text[start..self.at])
    }

    /// A string in single or double quotes, with JSON escapes when
    /// `escapes` is set (JSONPath) and none otherwise (XPath).
    pub(crate) fn quoted(&mut self, escapes: bool) -> Result<String, QueryError> {
        let Some(quote @ ('\'' | '"')) = self.peek() else {
            return Err(self.error("expected a quoted string"));
        };
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("unterminated string")),
                Some(c) if c == quote => return Ok(out),
                Some('\\') if escapes => out.push(self.escape()?),
                Some(c) => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, QueryError> {
        Ok(match self.bump() {
            Some('b') => '\u{8}',
            Some('f') => '\u{c}',
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('u') => return self.unicode(),
            Some(c @ ('\\' | '/' | '\'' | '"')) => c,
            _ => return Err(self.error("invalid escape")),
        })
    }

    fn unicode(&mut self) -> Result<char, QueryError> {
        let high = self.hex4()?;
        if !(0xD800..0xDC00).contains(&high) {
            return char::from_u32(high).ok_or_else(|| self.error("invalid \\u escape"));
        }
        if !self.eat("\\u") {
            return Err(self.error("unpaired surrogate"));
        }
        let low = self.hex4()?;
        if !(0xDC00..0xE000).contains(&low) {
            return Err(self.error("unpaired surrogate"));
        }
        char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
            .ok_or_else(|| self.error("invalid \\u escape"))
    }

    fn hex4(&mut self) -> Result<u32, QueryError> {
        let digits = self.rest().get(..4).unwrap_or("");
        if digits.len() == 4 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
            self.at += 4;
            u32::from_str_radix(digits, 16).map_err(|_| self.error("invalid \\u escape"))
        } else {
            Err(self.error("invalid \\u escape"))
        }
    }
}
