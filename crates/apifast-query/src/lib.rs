//! JSONPath and XPath queries over response bodies (docs/05): what
//! `response(path=…)`, assertions and the response filter use.
//!
//! - [`JsonPath`] follows RFC 9535 over the domain's [`Json`], and also takes
//!   the `[?(…)]` filter form of Jayway and Bruno.
//! - [`XPath`] is XPath 1.0 over an XML text.
//!
//! Both are pure and bounded: a query that would visit more than
//! [`MAX_VISITS`] values or nodes stops with [`QueryError::TooExpensive`]
//! instead of running for minutes on a large body.

mod cursor;
mod json_path;
mod xpath;

pub use json_path::JsonPath;
pub use xpath::XPath;

use apifast_domain::Json;

/// How many values or nodes a query may visit.
pub const MAX_VISITS: u64 = 20_000_000;

/// Why a query gives no result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// The expression is malformed.
    #[error("invalid {language} `{expression}`: {reason} at character {at}")]
    Syntax {
        /// `JSONPath` or `XPath`.
        language: &'static str,
        /// The expression, as written.
        expression: String,
        /// What is wrong.
        reason: String,
        /// Where, counting characters from 1.
        at: usize,
    },
    /// A JSONPath on a body that is not JSON.
    #[error("the body is not JSON")]
    NotJson,
    /// An XPath on a body that is not XML.
    #[error("the body is not XML: {0}")]
    NotXml(String),
    /// A query that visits too much.
    #[error("the query visits more than {MAX_VISITS} values; make it narrower")]
    TooExpensive,
    /// A regular expression in `match()` or `search()` that cannot be used.
    #[error("invalid regular expression `{0}`")]
    Regex(String),
}

/// The language of an expression: JSONPath when it starts with `$` (or `@`),
/// XPath otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// RFC 9535.
    JsonPath,
    /// XPath 1.0.
    XPath,
}

impl Language {
    /// The language `expression` is written in.
    pub fn of(expression: &str) -> Self {
        match expression.trim_start().chars().next() {
            Some('$' | '@') => Self::JsonPath,
            _ => Self::XPath,
        }
    }
}

/// A parsed expression in either language.
#[derive(Debug, Clone)]
pub enum Query {
    /// A JSONPath.
    JsonPath(JsonPath),
    /// An XPath.
    XPath(XPath),
}

impl Query {
    /// Parses `expression` in the language it is written in.
    ///
    /// # Errors
    ///
    /// If it is malformed.
    pub fn parse(expression: &str) -> Result<Self, QueryError> {
        match Language::of(expression) {
            Language::JsonPath => JsonPath::parse(expression).map(Self::JsonPath),
            Language::XPath => XPath::parse(expression).map(Self::XPath),
        }
    }

    /// The values it selects from a body. `json` is the body already read as
    /// JSON, when it is JSON; a JSONPath needs it and an XPath reads `text`.
    ///
    /// # Errors
    ///
    /// If the body is not in the query's language, or the query is too
    /// expensive.
    pub fn select(&self, text: &str, json: Option<&Json>) -> Result<Vec<Json>, QueryError> {
        match self {
            Self::JsonPath(path) => {
                let json = json.ok_or(QueryError::NotJson)?;
                Ok(path.select(json)?.into_iter().cloned().collect())
            }
            Self::XPath(path) => path.evaluate(text),
        }
    }
}

/// Parses `expression` and selects from a body in one step; see
/// [`Query::select`].
///
/// # Errors
///
/// As [`Query::parse`] and [`Query::select`].
pub fn select(expression: &str, text: &str, json: Option<&Json>) -> Result<Vec<Json>, QueryError> {
    Query::parse(expression)?.select(text, json)
}

/// Counts what a query visits, to stop it at [`MAX_VISITS`].
#[derive(Debug, Default)]
pub(crate) struct Budget {
    spent: std::cell::Cell<u64>,
}

impl Budget {
    /// Spends `n` visits.
    pub(crate) fn spend(&self, n: usize) -> Result<(), QueryError> {
        let spent = self.spent.get().saturating_add(n as u64);
        self.spent.set(spent);
        if spent > MAX_VISITS {
            Err(QueryError::TooExpensive)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_follows_the_first_character() {
        assert_eq!(Language::of("$.a"), Language::JsonPath);
        assert_eq!(Language::of("  @.a"), Language::JsonPath);
        assert_eq!(Language::of("/a/b"), Language::XPath);
        assert_eq!(Language::of("count(//a)"), Language::XPath);
    }

    #[test]
    fn select_picks_the_language_and_the_body() {
        let json = Json::parse(r#"{"a":[1,2]}"#).ok();
        assert_eq!(
            select("$.a[*]", "", json.as_ref()),
            Ok(vec![Json::Number("1".into()), Json::Number("2".into())])
        );
        assert_eq!(select("$.a", "<a/>", None), Err(QueryError::NotJson));
        assert_eq!(
            select("/a/b", "<a><b>x</b></a>", None),
            Ok(vec![Json::String("x".into())])
        );
        assert!(matches!(
            select("/a", "{}", json.as_ref()),
            Err(QueryError::NotXml(_))
        ));
    }
}
