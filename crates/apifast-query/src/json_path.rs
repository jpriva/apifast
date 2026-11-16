//! JSONPath, RFC 9535, over [`Json`].
//!
//! Beyond the RFC, a filter may be wrapped in parentheses as in Jayway and
//! Bruno (`[?(@.price < 10)]`), a comparison may use a query that selects
//! several values (it compares when there is exactly one), and a name after
//! a dot may contain `-`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use apifast_domain::Json;
use regex::{Regex, RegexBuilder};

use crate::cursor::Cursor;
use crate::{Budget, QueryError};

const LANGUAGE: &str = "JSONPath";

/// The largest compiled regular expression in `match()` or `search()`.
const REGEX_LIMIT: usize = 1 << 20;

/// A parsed JSONPath.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonPath {
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    /// `..`: applies to the value and all its descendants.
    descendant: bool,
    selectors: Vec<Selector>,
}

#[derive(Debug, Clone, PartialEq)]
enum Selector {
    Name(String),
    Wildcard,
    Index(i64),
    Slice(Option<i64>, Option<i64>, Option<i64>),
    Filter(Expr),
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Or(Vec<Expr>),
    And(Vec<Expr>),
    Not(Box<Expr>),
    Compare(Operand, Comparison, Operand),
    Test(Operand),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Literal(Json),
    /// `$…` when `absolute`, `@…` otherwise.
    Query {
        absolute: bool,
        segments: Vec<Segment>,
    },
    Call(Function, Vec<Operand>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Function {
    Length,
    Count,
    Match,
    Search,
    Value,
}

impl Function {
    fn named(name: &str) -> Option<(Self, usize)> {
        Some(match name {
            "length" => (Self::Length, 1),
            "count" => (Self::Count, 1),
            "match" => (Self::Match, 2),
            "search" => (Self::Search, 2),
            "value" => (Self::Value, 1),
            _ => return None,
        })
    }
}

impl JsonPath {
    /// Parses a JSONPath that starts with `$`.
    ///
    /// # Errors
    ///
    /// If it is malformed, with where.
    pub fn parse(text: &str) -> Result<Self, QueryError> {
        let mut cursor = Cursor::new(text, LANGUAGE);
        cursor.space();
        cursor.expect("$")?;
        let segments = segments(&mut cursor)?;
        cursor.space();
        if !cursor.at_end() {
            return Err(cursor.error("unexpected text"));
        }
        Ok(Self { segments })
    }

    /// The values it selects, in the RFC's order.
    ///
    /// # Errors
    ///
    /// If it visits more than [`crate::MAX_VISITS`] values, or a regular
    /// expression in a filter cannot be used.
    pub fn select<'a>(&self, root: &'a Json) -> Result<Vec<&'a Json>, QueryError> {
        let eval = Eval {
            root,
            budget: Budget::default(),
            regexes: RefCell::default(),
        };
        eval.apply(&self.segments, vec![root])
    }
}

// ---------------------------------------------------------------- parsing

fn segments(cursor: &mut Cursor<'_>) -> Result<Vec<Segment>, QueryError> {
    let mut segments = Vec::new();
    loop {
        let before = cursor.at;
        cursor.space();
        if cursor.eat("..") {
            let selectors = match cursor.peek() {
                Some('[') => bracket(cursor)?,
                Some('*') => {
                    cursor.bump();
                    vec![Selector::Wildcard]
                }
                _ => vec![Selector::Name(name(cursor)?)],
            };
            segments.push(Segment {
                descendant: true,
                selectors,
            });
        } else if cursor.eat(".") {
            let selector = if cursor.eat("*") {
                Selector::Wildcard
            } else {
                Selector::Name(name(cursor)?)
            };
            segments.push(Segment {
                descendant: false,
                selectors: vec![selector],
            });
        } else if cursor.peek() == Some('[') {
            segments.push(Segment {
                descendant: false,
                selectors: bracket(cursor)?,
            });
        } else {
            cursor.at = before;
            return Ok(segments);
        }
    }
}

fn name(cursor: &mut Cursor<'_>) -> Result<String, QueryError> {
    let name = cursor.take_while(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if name.is_empty() {
        Err(cursor.error("expected a name"))
    } else {
        Ok(name.to_owned())
    }
}

fn bracket(cursor: &mut Cursor<'_>) -> Result<Vec<Selector>, QueryError> {
    cursor.expect("[")?;
    let mut selectors = Vec::new();
    loop {
        cursor.space();
        selectors.push(selector(cursor)?);
        cursor.space();
        if cursor.eat("]") {
            return Ok(selectors);
        }
        if !cursor.eat(",") {
            return Err(cursor.error("expected `,` or `]`"));
        }
    }
}

fn selector(cursor: &mut Cursor<'_>) -> Result<Selector, QueryError> {
    match cursor.peek() {
        Some('\'' | '"') => Ok(Selector::Name(cursor.quoted(true)?)),
        Some('*') => {
            cursor.bump();
            Ok(Selector::Wildcard)
        }
        Some('?') => {
            cursor.bump();
            cursor.enter()?;
            let filter = or(cursor)?;
            cursor.leave();
            Ok(Selector::Filter(filter))
        }
        _ => index_or_slice(cursor),
    }
}

fn index_or_slice(cursor: &mut Cursor<'_>) -> Result<Selector, QueryError> {
    let start = integer(cursor)?;
    cursor.space();
    if !cursor.eat(":") {
        return start
            .map(Selector::Index)
            .ok_or_else(|| cursor.error("expected a selector"));
    }
    cursor.space();
    let end = integer(cursor)?;
    cursor.space();
    let step = if cursor.eat(":") {
        cursor.space();
        integer(cursor)?
    } else {
        None
    };
    Ok(Selector::Slice(start, end, step))
}

fn integer(cursor: &mut Cursor<'_>) -> Result<Option<i64>, QueryError> {
    let Some(text) = cursor.number() else {
        return Ok(None);
    };
    text.parse()
        .map(Some)
        .map_err(|_| cursor.error("expected an integer"))
}

fn or(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    let mut terms = vec![and(cursor)?];
    loop {
        cursor.space();
        if !cursor.eat("||") {
            break;
        }
        terms.push(and(cursor)?);
    }
    Ok(if terms.len() == 1 {
        terms.remove(0)
    } else {
        Expr::Or(terms)
    })
}

fn and(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    let mut terms = vec![unary(cursor)?];
    loop {
        cursor.space();
        if !cursor.eat("&&") {
            break;
        }
        terms.push(unary(cursor)?);
    }
    Ok(if terms.len() == 1 {
        terms.remove(0)
    } else {
        Expr::And(terms)
    })
}

fn unary(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    cursor.space();
    if cursor.peek() == Some('!') && cursor.peek_second() != Some('=') {
        cursor.bump();
        cursor.enter()?;
        let inner = unary(cursor)?;
        cursor.leave();
        return Ok(Expr::Not(Box::new(inner)));
    }
    if cursor.eat("(") {
        cursor.enter()?;
        let inner = or(cursor)?;
        cursor.leave();
        cursor.space();
        cursor.expect(")")?;
        return Ok(inner);
    }
    let left = operand(cursor)?;
    cursor.space();
    let Some(comparison) = comparison(cursor) else {
        if let Operand::Literal(_) = left {
            return Err(cursor.error("a literal alone is not a test"));
        }
        return Ok(Expr::Test(left));
    };
    let right = operand(cursor)?;
    Ok(Expr::Compare(left, comparison, right))
}

fn comparison(cursor: &mut Cursor<'_>) -> Option<Comparison> {
    const OPERATORS: [(&str, Comparison); 6] = [
        ("==", Comparison::Equal),
        ("!=", Comparison::NotEqual),
        ("<=", Comparison::LessOrEqual),
        (">=", Comparison::GreaterOrEqual),
        ("<", Comparison::Less),
        (">", Comparison::Greater),
    ];
    OPERATORS
        .iter()
        .find(|(token, _)| cursor.eat(token))
        .map(|(_, comparison)| *comparison)
}

fn operand(cursor: &mut Cursor<'_>) -> Result<Operand, QueryError> {
    cursor.space();
    match cursor.peek() {
        Some(c @ ('@' | '$')) => {
            cursor.bump();
            Ok(Operand::Query {
                absolute: c == '$',
                segments: segments(cursor)?,
            })
        }
        Some('\'' | '"') => Ok(Operand::Literal(Json::String(cursor.quoted(true)?))),
        _ => {
            if let Some(number) = cursor.number() {
                return Ok(Operand::Literal(Json::Number(number.to_owned())));
            }
            let word = cursor.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
            match word {
                "true" => Ok(Operand::Literal(Json::Bool(true))),
                "false" => Ok(Operand::Literal(Json::Bool(false))),
                "null" => Ok(Operand::Literal(Json::Null)),
                "" => Err(cursor.error("expected a value, `@` or `$`")),
                name => call(cursor, name),
            }
        }
    }
}

fn call(cursor: &mut Cursor<'_>, name: &str) -> Result<Operand, QueryError> {
    let Some((function, arity)) = Function::named(name) else {
        return Err(cursor.error(format!("unknown function `{name}`")));
    };
    cursor.space();
    cursor.expect("(")?;
    cursor.enter()?;
    let mut args = Vec::new();
    cursor.space();
    if !cursor.eat(")") {
        loop {
            args.push(operand(cursor)?);
            cursor.space();
            if cursor.eat(")") {
                break;
            }
            cursor.expect(",")?;
        }
    }
    cursor.leave();
    if args.len() != arity {
        return Err(cursor.error(format!("`{name}` takes {arity} argument(s)")));
    }
    if function == Function::Count && !matches!(args[0], Operand::Query { .. }) {
        return Err(cursor.error("`count` takes a query"));
    }
    Ok(Operand::Call(function, args))
}

// ------------------------------------------------------------- evaluation

struct Eval<'a> {
    root: &'a Json,
    budget: Budget,
    regexes: RefCell<HashMap<(String, bool), Regex>>,
}

/// What an operand yields inside a filter.
enum Value<'a> {
    Nodes(Vec<&'a Json>),
    /// A value, or nothing (`None`).
    Single(Option<Cow<'a, Json>>),
    Logical(bool),
}

impl<'a> Value<'a> {
    fn single(self) -> Option<Cow<'a, Json>> {
        match self {
            Self::Nodes(mut nodes) if nodes.len() == 1 => nodes.pop().map(Cow::Borrowed),
            Self::Nodes(_) => None,
            Self::Single(value) => value,
            Self::Logical(b) => Some(Cow::Owned(Json::Bool(b))),
        }
    }

    fn truth(&self) -> bool {
        match self {
            Self::Nodes(nodes) => !nodes.is_empty(),
            Self::Single(value) => value.is_some(),
            Self::Logical(b) => *b,
        }
    }
}

impl<'a> Eval<'a> {
    fn apply(
        &self,
        segments: &[Segment],
        mut nodes: Vec<&'a Json>,
    ) -> Result<Vec<&'a Json>, QueryError> {
        for segment in segments {
            let mut out = Vec::new();
            for node in nodes {
                if segment.descendant {
                    for each in self.descendants(node)? {
                        self.select(each, &segment.selectors, &mut out)?;
                    }
                } else {
                    self.select(node, &segment.selectors, &mut out)?;
                }
            }
            nodes = out;
        }
        Ok(nodes)
    }

    /// The value and all its descendants, each before its children.
    fn descendants(&self, node: &'a Json) -> Result<Vec<&'a Json>, QueryError> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(next) = stack.pop() {
            self.budget.spend(1)?;
            out.push(next);
            let children = children(next);
            stack.extend(children.into_iter().rev());
        }
        Ok(out)
    }

    fn select(
        &self,
        node: &'a Json,
        selectors: &[Selector],
        out: &mut Vec<&'a Json>,
    ) -> Result<(), QueryError> {
        self.budget.spend(1)?;
        for selector in selectors {
            match selector {
                Selector::Name(name) => out.extend(node.get(name)),
                Selector::Wildcard => out.extend(children(node)),
                Selector::Index(i) => {
                    if let Json::Array(items) = node {
                        out.extend(index(items.len(), *i).and_then(|i| items.get(i)));
                    }
                }
                Selector::Slice(start, end, step) => {
                    if let Json::Array(items) = node {
                        out.extend(
                            slice(items.len(), *start, *end, *step)
                                .into_iter()
                                .filter_map(|i| items.get(i)),
                        );
                    }
                }
                Selector::Filter(expr) => {
                    for child in children(node) {
                        self.budget.spend(1)?;
                        if self.test(expr, child)? {
                            out.push(child);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn test(&self, expr: &Expr, current: &'a Json) -> Result<bool, QueryError> {
        Ok(match expr {
            Expr::Or(terms) => {
                for term in terms {
                    if self.test(term, current)? {
                        return Ok(true);
                    }
                }
                false
            }
            Expr::And(terms) => {
                for term in terms {
                    if !self.test(term, current)? {
                        return Ok(false);
                    }
                }
                true
            }
            Expr::Not(inner) => !self.test(inner, current)?,
            Expr::Test(operand) => self.operand(operand, current)?.truth(),
            Expr::Compare(left, comparison, right) => {
                let left = self.operand(left, current)?.single();
                let right = self.operand(right, current)?.single();
                compare(left.as_deref(), *comparison, right.as_deref())
            }
        })
    }

    fn operand(&self, operand: &Operand, current: &'a Json) -> Result<Value<'a>, QueryError> {
        Ok(match operand {
            // Literals are small; owning a copy keeps the query's lifetime
            // apart from the body's.
            Operand::Literal(value) => Value::Single(Some(Cow::Owned(value.clone()))),
            Operand::Query { absolute, segments } => {
                let start = if *absolute { self.root } else { current };
                Value::Nodes(self.apply(segments, vec![start])?)
            }
            Operand::Call(function, args) => self.call(*function, args, current)?,
        })
    }

    fn call(
        &self,
        function: Function,
        args: &[Operand],
        current: &'a Json,
    ) -> Result<Value<'a>, QueryError> {
        let arg = |i: usize| self.operand(&args[i], current);
        Ok(match function {
            Function::Length => {
                Value::Single(arg(0)?.single().and_then(|v| length(&v)).map(Cow::Owned))
            }
            Function::Count => {
                let count = match arg(0)? {
                    Value::Nodes(nodes) => nodes.len(),
                    _ => 0,
                };
                Value::Single(Some(Cow::Owned(Json::Number(count.to_string()))))
            }
            Function::Value => Value::Single(arg(0)?.single()),
            Function::Match | Function::Search => {
                let text = arg(0)?.single();
                let pattern = arg(1)?.single();
                let (Some(Json::String(text)), Some(Json::String(pattern))) =
                    (text.as_deref(), pattern.as_deref())
                else {
                    return Ok(Value::Logical(false));
                };
                Value::Logical(
                    self.regex(pattern, function == Function::Match)?
                        .is_match(text),
                )
            }
        })
    }

    fn regex(&self, pattern: &str, whole: bool) -> Result<Regex, QueryError> {
        let key = (pattern.to_owned(), whole);
        if let Some(regex) = self.regexes.borrow().get(&key) {
            return Ok(regex.clone());
        }
        let source = if whole {
            format!("^(?:{pattern})$")
        } else {
            pattern.to_owned()
        };
        let regex = RegexBuilder::new(&source)
            .size_limit(REGEX_LIMIT)
            .build()
            .map_err(|_| QueryError::Regex(pattern.to_owned()))?;
        self.regexes.borrow_mut().insert(key, regex.clone());
        Ok(regex)
    }
}

fn children(node: &Json) -> Vec<&Json> {
    match node {
        Json::Array(items) => items.iter().collect(),
        Json::Object(pairs) => pairs.iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    }
}

fn length(value: &Json) -> Option<Json> {
    let n = match value {
        Json::String(text) => text.chars().count(),
        Json::Array(items) => items.len(),
        Json::Object(pairs) => pairs.len(),
        _ => return None,
    };
    Some(Json::Number(n.to_string()))
}

/// `i` counted from the end when negative.
fn index(len: usize, i: i64) -> Option<usize> {
    let len = i64::try_from(len).ok()?;
    let i = if i < 0 { len.checked_add(i)? } else { i };
    usize::try_from(i).ok().filter(|_| i < len)
}

/// The indexes of a slice, as in RFC 9535 section 2.3.4.
fn slice(len: usize, start: Option<i64>, end: Option<i64>, step: Option<i64>) -> Vec<usize> {
    let Ok(len) = i64::try_from(len) else {
        return Vec::new();
    };
    let step = step.unwrap_or(1);
    let normal = |i: i64| if i >= 0 { i } else { len.saturating_add(i) };
    let mut out = Vec::new();
    if step > 0 {
        let lower = normal(start.unwrap_or(0)).clamp(0, len);
        let upper = normal(end.unwrap_or(len)).clamp(0, len);
        let mut i = lower;
        while i < upper {
            out.extend(usize::try_from(i));
            i = i.saturating_add(step);
        }
    } else if step < 0 {
        let upper = start.map_or(len - 1, normal).clamp(-1, len - 1);
        let lower = end.map_or(-1, normal).clamp(-1, len - 1);
        let mut i = upper;
        while lower < i {
            out.extend(usize::try_from(i));
            i = i.saturating_add(step);
        }
    }
    out
}

fn compare(left: Option<&Json>, comparison: Comparison, right: Option<&Json>) -> bool {
    match comparison {
        Comparison::Equal => equal(left, right),
        Comparison::NotEqual => !equal(left, right),
        Comparison::Less => less(left, right),
        Comparison::LessOrEqual => less(left, right) || equal(left, right),
        Comparison::Greater => less(right, left),
        Comparison::GreaterOrEqual => less(right, left) || equal(left, right),
    }
}

fn equal(left: Option<&Json>, right: Option<&Json>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b),
        _ => false,
    }
}

/// Equality with numbers compared by value, so `1 == 1.0`.
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(_), Json::Number(_)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= f64::EPSILON * x.abs().max(y.abs()),
            _ => a == b,
        },
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| b.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

fn less(left: Option<&Json>, right: Option<&Json>) -> bool {
    match (left, right) {
        (Some(Json::Number(_)), Some(Json::Number(_))) => {
            match (left.and_then(Json::as_f64), right.and_then(Json::as_f64)) {
                (Some(x), Some(y)) => x < y,
                _ => false,
            }
        }
        (Some(Json::String(a)), Some(Json::String(b))) => a < b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_count_from_the_end_when_negative() {
        assert_eq!(index(3, 0), Some(0));
        assert_eq!(index(3, -1), Some(2));
        assert_eq!(index(3, 3), None);
        assert_eq!(index(3, -4), None);
        assert_eq!(index(3, i64::MIN), None);
    }

    #[test]
    fn slices_follow_the_rfc() {
        assert_eq!(slice(5, Some(1), Some(3), None), vec![1, 2]);
        assert_eq!(slice(5, None, None, Some(2)), vec![0, 2, 4]);
        assert_eq!(slice(5, None, None, Some(-1)), vec![4, 3, 2, 1, 0]);
        assert_eq!(slice(5, Some(-2), None, None), vec![3, 4]);
        assert_eq!(slice(5, None, None, Some(0)), Vec::<usize>::new());
        assert_eq!(
            slice(5, Some(i64::MIN), Some(i64::MAX), Some(i64::MAX)),
            vec![0]
        );
    }

    #[test]
    fn numbers_are_equal_by_value() {
        let one = Json::Number("1".into());
        let one_point_zero = Json::Number("1.0".into());
        assert!(same(&one, &one_point_zero));
        assert!(!less(Some(&one), Some(&Json::String("2".into()))));
    }
}
