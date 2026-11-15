use std::fmt;

/// A parsed template.
///
/// Parsing never fails and is lossless: `Template::parse(s).to_string() == s`
/// for every input. A malformed tag becomes an [`Token::Invalid`] that renders
/// as literal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    tokens: Vec<Token>,
}

/// A piece of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Literal text.
    Text(String),
    /// `\{{`, which renders as a literal `{{`.
    Escape,
    /// A well-formed `{{ ... }}` tag.
    Tag(Tag),
    /// Something that looks like a tag but is not one. Renders as its raw text.
    Invalid(Invalid),
}

impl Token {
    /// How many bytes of the template source this token covers. Parsing is
    /// lossless, so the lengths of the tokens add up to the source's: an
    /// editor uses them to find where each token is.
    pub fn source_len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Escape => 3,
            Self::Tag(tag) => tag.inner.len() + 4,
            Self::Invalid(invalid) => invalid.raw.len(),
        }
    }
}

/// A well-formed tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Text between the braces, exactly as written (with its whitespace).
    inner: String,
    expr: Expr,
}

impl Tag {
    /// The expression inside the tag.
    pub fn expr(&self) -> &Expr {
        &self.expr
    }
}

/// A malformed tag, kept verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    /// The raw text, including the braces.
    pub raw: String,
    /// Why it is not a valid tag.
    pub reason: InvalidReason,
}

/// Why a tag is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidReason {
    /// `{{` with no matching `}}`.
    #[error("tag is not closed")]
    Unclosed,
    /// `{{ }}` with nothing inside.
    #[error("tag is empty")]
    Empty,
    /// The contents are not a valid expression.
    #[error("invalid expression")]
    Syntax,
}

/// The expression inside a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// `{{name}}`
    Variable(String),
    /// `{{$name}}`, generated on every send.
    Dynamic(String),
    /// `{{?Label}}`, asked to the user before sending.
    Prompt(String),
    /// `{{process.env.NAME}}`
    ProcessEnv(String),
    /// `'text'`
    Str(String),
    /// `true` or `false`.
    Bool(bool),
    /// `null`
    Null,
    /// `name(arg='value', other=variable)`, with named arguments.
    Function {
        /// Function name.
        name: String,
        /// Named arguments, in order.
        args: Vec<(String, Expr)>,
    },
}

const PROCESS_ENV: &str = "process.env.";

impl Template {
    /// Parses a template.
    pub fn parse(input: &str) -> Self {
        let mut tokens = Vec::new();
        let mut text_start = 0;
        let mut i = 0;
        let bytes = input.as_bytes();

        let flush = |tokens: &mut Vec<Token>, from: usize, to: usize| {
            if from < to {
                tokens.push(Token::Text(input[from..to].to_owned()));
            }
        };

        while i < bytes.len() {
            // Compare bytes: slicing `input` here could split a multibyte char.
            if bytes[i..].starts_with(b"\\{{") {
                flush(&mut tokens, text_start, i);
                tokens.push(Token::Escape);
                i += 3;
                text_start = i;
            } else if bytes[i..].starts_with(b"{{") {
                flush(&mut tokens, text_start, i);
                if let Some(close) = find_close(input, i + 2) {
                    let inner = &input[i + 2..close];
                    tokens.push(match parse_tag(inner) {
                        Ok(expr) => Token::Tag(Tag {
                            inner: inner.to_owned(),
                            expr,
                        }),
                        Err(reason) => Token::Invalid(Invalid {
                            raw: input[i..close + 2].to_owned(),
                            reason,
                        }),
                    });
                    i = close + 2;
                } else {
                    tokens.push(Token::Invalid(Invalid {
                        raw: input[i..].to_owned(),
                        reason: InvalidReason::Unclosed,
                    }));
                    i = bytes.len();
                }
                text_start = i;
            } else {
                i += 1;
            }
        }
        flush(&mut tokens, text_start, bytes.len());
        Self { tokens }
    }

    /// The tokens, in order.
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Malformed tags, for the editor to flag.
    pub fn invalid_tags(&self) -> impl Iterator<Item = &Invalid> {
        self.tokens.iter().filter_map(|t| match t {
            Token::Invalid(invalid) => Some(invalid),
            _ => None,
        })
    }

    /// Names of the variables the template references, including inside
    /// function arguments, in order of appearance (with repeats).
    pub fn variables(&self) -> Vec<&str> {
        fn walk<'a>(expr: &'a Expr, out: &mut Vec<&'a str>) {
            match expr {
                Expr::Variable(name) => out.push(name),
                Expr::Function { args, .. } => args.iter().for_each(|(_, e)| walk(e, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for token in &self.tokens {
            if let Token::Tag(tag) = token {
                walk(&tag.expr, &mut out);
            }
        }
        out
    }

    /// Renames variable `from` to `to` everywhere, and returns how many
    /// references changed. Only the affected tags are rewritten; their
    /// surrounding whitespace and every other byte of the template are kept.
    pub fn rename_variable(&mut self, from: &str, to: &str) -> usize {
        fn walk(expr: &mut Expr, from: &str, to: &str) -> usize {
            match expr {
                Expr::Variable(name) if name == from => {
                    to.clone_into(name);
                    1
                }
                Expr::Function { args, .. } => {
                    args.iter_mut().map(|(_, e)| walk(e, from, to)).sum()
                }
                _ => 0,
            }
        }
        let mut count = 0;
        for token in &mut self.tokens {
            if let Token::Tag(tag) = token {
                let changed = walk(&mut tag.expr, from, to);
                if changed > 0 {
                    let lead = tag.inner.len() - tag.inner.trim_start().len();
                    let trail = tag.inner.len() - tag.inner.trim_end().len();
                    let (lead, trail) = (&tag.inner[..lead], &tag.inner[tag.inner.len() - trail..]);
                    tag.inner = format!("{lead}{}{trail}", tag.expr);
                    count += changed;
                }
            }
        }
        count
    }
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for token in &self.tokens {
            match token {
                Token::Text(text) => f.write_str(text)?,
                Token::Escape => f.write_str("\\{{")?,
                Token::Tag(tag) => write!(f, "{{{{{}}}}}", tag.inner)?,
                Token::Invalid(invalid) => f.write_str(&invalid.raw)?,
            }
        }
        Ok(())
    }
}

/// Canonical form of an expression, as written by [`Template::rename_variable`].
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Variable(name) => f.write_str(name),
            Self::Dynamic(name) => write!(f, "${name}"),
            Self::Prompt(label) => write!(f, "?{label}"),
            Self::ProcessEnv(name) => write!(f, "{PROCESS_ENV}{name}"),
            Self::Str(text) => {
                f.write_str("'")?;
                for c in text.chars() {
                    if matches!(c, '\'' | '\\') {
                        f.write_str("\\")?;
                    }
                    write!(f, "{c}")?;
                }
                f.write_str("'")
            }
            Self::Bool(value) => write!(f, "{value}"),
            Self::Null => f.write_str("null"),
            Self::Function { name, args } => {
                write!(f, "{name}(")?;
                for (i, (arg, value)) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{arg}={value}")?;
                }
                f.write_str(")")
            }
        }
    }
}

/// Finds the `}}` that closes a tag opened before `from`, skipping `}}` inside
/// single-quoted strings.
fn find_close(input: &str, from: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = from;
    let mut in_string = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => i += 1,
            b'\'' => in_string = !in_string,
            b'}' if !in_string && bytes.get(i + 1) == Some(&b'}') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    // An unbalanced quote must not hide the closing braces.
    if in_string {
        input[from..].find("}}").map(|p| p + from)
    } else {
        None
    }
}

fn parse_tag(inner: &str) -> Result<Expr, InvalidReason> {
    let s = inner.trim();
    if s.is_empty() {
        return Err(InvalidReason::Empty);
    }
    if let Some(name) = s.strip_prefix('$') {
        return if !name.is_empty() && name.chars().all(is_ident_char) {
            Ok(Expr::Dynamic(name.to_owned()))
        } else {
            Err(InvalidReason::Syntax)
        };
    }
    if let Some(label) = s.strip_prefix('?') {
        let label = label.trim();
        return if label.is_empty() {
            Err(InvalidReason::Syntax)
        } else {
            Ok(Expr::Prompt(label.to_owned()))
        };
    }
    let mut parser = Parser { s, pos: 0 };
    let expr = parser.value()?;
    parser.skip_ws();
    if parser.pos == s.len() {
        Ok(expr)
    } else {
        Err(InvalidReason::Syntax)
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_name_char(c: char) -> bool {
    is_ident_char(c) || c == '-' || c == '.'
}

/// Recursive-descent parser for the expression inside a tag.
struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.s[self.pos..]
    }

    fn skip_ws(&mut self) {
        self.pos = self.s.len() - self.rest().trim_start().len();
    }

    fn eat(&mut self, c: char) -> bool {
        if self.rest().starts_with(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &str {
        let start = self.pos;
        let len = self.rest().find(|c| !f(c)).unwrap_or(self.rest().len());
        self.pos += len;
        &self.s[start..self.pos]
    }

    fn value(&mut self) -> Result<Expr, InvalidReason> {
        self.skip_ws();
        if self.eat('\'') {
            return self.string();
        }
        let name = self.take_while(is_name_char).to_owned();
        if name.is_empty() {
            return Err(InvalidReason::Syntax);
        }
        self.skip_ws();
        if self.eat('(') {
            if !name.chars().all(is_ident_char) {
                return Err(InvalidReason::Syntax);
            }
            return Ok(Expr::Function {
                name,
                args: self.args()?,
            });
        }
        Ok(match name.as_str() {
            "true" => Expr::Bool(true),
            "false" => Expr::Bool(false),
            "null" => Expr::Null,
            _ => match name.strip_prefix(PROCESS_ENV) {
                Some(var) if !var.is_empty() => Expr::ProcessEnv(var.to_owned()),
                Some(_) => return Err(InvalidReason::Syntax),
                None => Expr::Variable(name),
            },
        })
    }

    /// A single-quoted string, after the opening quote. `\'` and `\\` escape.
    fn string(&mut self) -> Result<Expr, InvalidReason> {
        let mut out = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\'' => {
                    self.pos += i + 1;
                    return Ok(Expr::Str(out));
                }
                '\\' => match chars.next() {
                    Some((_, escaped)) => out.push(escaped),
                    None => break,
                },
                _ => out.push(c),
            }
        }
        Err(InvalidReason::Syntax)
    }

    /// Named arguments, after the opening parenthesis.
    fn args(&mut self) -> Result<Vec<(String, Expr)>, InvalidReason> {
        let mut args = Vec::new();
        self.skip_ws();
        if self.eat(')') {
            return Ok(args);
        }
        loop {
            self.skip_ws();
            let name = self.take_while(is_ident_char).to_owned();
            self.skip_ws();
            if name.is_empty() || !self.eat('=') {
                return Err(InvalidReason::Syntax);
            }
            args.push((name, self.value()?));
            self.skip_ws();
            if self.eat(')') {
                return Ok(args);
            }
            if !self.eat(',') {
                return Err(InvalidReason::Syntax);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn single_expr(input: &str) -> Expr {
        match Template::parse(input).tokens() {
            [Token::Tag(tag)] => tag.expr().clone(),
            other => panic!("expected one tag, got {other:?}"),
        }
    }

    fn invalid_reason(input: &str) -> InvalidReason {
        let template = Template::parse(input);
        let reasons: Vec<_> = template.invalid_tags().map(|i| i.reason).collect();
        match reasons.as_slice() {
            [reason] => *reason,
            other => panic!("expected one invalid tag, got {other:?}"),
        }
    }

    #[test]
    fn parses_every_kind_of_expression() {
        assert_eq!(single_expr("{{host}}"), Expr::Variable("host".into()));
        assert_eq!(
            single_expr("{{ user.address-1 }}"),
            Expr::Variable("user.address-1".into())
        );
        assert_eq!(
            single_expr("{{$randomUUID}}"),
            Expr::Dynamic("randomUUID".into())
        );
        assert_eq!(
            single_expr("{{?OTP code}}"),
            Expr::Prompt("OTP code".into())
        );
        assert_eq!(
            single_expr("{{process.env.API_KEY}}"),
            Expr::ProcessEnv("API_KEY".into())
        );
        assert_eq!(single_expr("{{'it\\'s'}}"), Expr::Str("it's".into()));
        assert_eq!(single_expr("{{true}}"), Expr::Bool(true));
        assert_eq!(single_expr("{{null}}"), Expr::Null);
    }

    #[test]
    fn parses_functions_with_named_arguments() {
        assert_eq!(
            single_expr(
                "{{ response(request='auth/login', path='$.token', fresh=true, key=api_key, inner=base64(value='x')) }}"
            ),
            Expr::Function {
                name: "response".into(),
                args: vec![
                    ("request".into(), Expr::Str("auth/login".into())),
                    ("path".into(), Expr::Str("$.token".into())),
                    ("fresh".into(), Expr::Bool(true)),
                    ("key".into(), Expr::Variable("api_key".into())),
                    (
                        "inner".into(),
                        Expr::Function {
                            name: "base64".into(),
                            args: vec![("value".into(), Expr::Str("x".into()))],
                        }
                    ),
                ],
            }
        );
        assert_eq!(
            single_expr("{{now()}}"),
            Expr::Function {
                name: "now".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn closing_braces_inside_strings_do_not_close_the_tag() {
        assert_eq!(
            single_expr("{{ f(a='}}') }}"),
            Expr::Function {
                name: "f".into(),
                args: vec![("a".into(), Expr::Str("}}".into()))],
            }
        );
    }

    #[test]
    fn malformed_tags_become_invalid_tokens() {
        assert_eq!(invalid_reason("a {{host"), InvalidReason::Unclosed);
        assert_eq!(invalid_reason("{{ }}"), InvalidReason::Empty);
        assert_eq!(invalid_reason("{{two words}}"), InvalidReason::Syntax);
        assert_eq!(invalid_reason("{{f(positional)}}"), InvalidReason::Syntax);
        assert_eq!(invalid_reason("{{f(a='x',)}}"), InvalidReason::Syntax);
        assert_eq!(invalid_reason("{{process.env.}}"), InvalidReason::Syntax);
        assert_eq!(invalid_reason("{{$}}"), InvalidReason::Syntax);
        assert_eq!(invalid_reason("{{a.b(x=1)}}"), InvalidReason::Syntax);
    }

    #[test]
    fn escape_produces_its_own_token() {
        assert_eq!(
            Template::parse("\\{{literal}}").tokens(),
            [Token::Escape, Token::Text("literal}}".into())]
        );
    }

    #[test]
    fn lists_referenced_variables_including_function_arguments() {
        let template = Template::parse("{{host}}/{{ f(a=token, b='x') }}/{{$timestamp}}/{{host}}");
        assert_eq!(template.variables(), ["host", "token", "host"]);
    }

    #[test]
    fn renaming_rewrites_only_the_affected_tags() {
        let mut template = Template::parse("{{ host }}/{{other}}/{{  f(a=host,b='x')  }}/{{ host");
        assert_eq!(template.rename_variable("host", "base_url"), 2);
        assert_eq!(
            template.to_string(),
            "{{ base_url }}/{{other}}/{{  f(a=base_url, b='x')  }}/{{ host"
        );
    }

    #[test]
    fn canonical_strings_escape_quotes_and_backslashes() {
        let expr = Expr::Str("it's a \\ path".into());
        assert_eq!(expr.to_string(), "'it\\'s a \\\\ path'");
        assert_eq!(single_expr(&format!("{{{{{expr}}}}}")), expr);
    }

    proptest! {
        #[test]
        fn parsing_is_lossless(input in ".*") {
            prop_assert_eq!(Template::parse(&input).to_string(), input);
        }

        #[test]
        fn parsing_is_lossless_with_many_braces(input in "[a-z {}'\\\\$?.()=,]*") {
            prop_assert_eq!(Template::parse(&input).to_string(), input);
        }

        /// Each token covers exactly its own part of the source.
        #[test]
        fn token_lengths_cover_the_source(input in "[a-z {}'\\\\$?.()=,ñ]*") {
            let template = Template::parse(&input);
            let mut at = 0;
            for token in template.tokens() {
                let end = at + token.source_len();
                let one = Template { tokens: vec![token.clone()] }.to_string();
                prop_assert_eq!(&input[at..end], one.as_str());
                at = end;
            }
            prop_assert_eq!(at, input.len());
        }
    }
}
