//! XPath 1.0 over an XML text, read with `roxmltree`.
//!
//! A name without a prefix matches by local name in any namespace, so
//! `//Body` finds `<soap:Body>` without declaring namespaces; `soap:Body`
//! matches only that namespace, with the prefix the document declares.
//! Variables (`$x`) are not supported. The result is a list of values: the
//! text of each selected node, or the one number, string or boolean.

use apifast_domain::Json;
use roxmltree::{Document, Node, ParsingOptions};

use crate::cursor::Cursor;
use crate::{Budget, QueryError};

const LANGUAGE: &str = "XPath";

/// A parsed XPath.
#[derive(Debug, Clone, PartialEq)]
pub struct XPath {
    expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Compare(Box<Expr>, Comparison, Box<Expr>),
    Arithmetic(Box<Expr>, Arithmetic, Box<Expr>),
    Negate(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Path {
        absolute: bool,
        steps: Vec<Step>,
    },
    Filter {
        primary: Box<Expr>,
        predicates: Vec<Expr>,
        steps: Vec<Step>,
    },
    Literal(String),
    Number(f64),
    Call(String, Vec<Expr>),
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

#[derive(Debug, Clone, PartialEq)]
struct Step {
    axis: Axis,
    test: Test,
    predicates: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Parent,
    Ancestor,
    AncestorOrSelf,
    FollowingSibling,
    PrecedingSibling,
    Following,
    Preceding,
    Attribute,
    SelfNode,
}

impl Axis {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "child" => Self::Child,
            "descendant" => Self::Descendant,
            "descendant-or-self" => Self::DescendantOrSelf,
            "parent" => Self::Parent,
            "ancestor" => Self::Ancestor,
            "ancestor-or-self" => Self::AncestorOrSelf,
            "following-sibling" => Self::FollowingSibling,
            "preceding-sibling" => Self::PrecedingSibling,
            "following" => Self::Following,
            "preceding" => Self::Preceding,
            "attribute" => Self::Attribute,
            "self" => Self::SelfNode,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Test {
    /// A name, or `*` when `local` is `None`.
    Name {
        prefix: Option<String>,
        local: Option<String>,
    },
    Text,
    Comment,
    Node,
    Instruction(Option<String>),
}

/// Functions and how many arguments they take (at least, at most).
const FUNCTIONS: &[(&str, usize, usize)] = &[
    ("last", 0, 0),
    ("position", 0, 0),
    ("count", 1, 1),
    ("local-name", 0, 1),
    ("name", 0, 1),
    ("namespace-uri", 0, 1),
    ("string", 0, 1),
    ("concat", 2, usize::MAX),
    ("starts-with", 2, 2),
    ("ends-with", 2, 2),
    ("contains", 2, 2),
    ("substring-before", 2, 2),
    ("substring-after", 2, 2),
    ("substring", 2, 3),
    ("string-length", 0, 1),
    ("normalize-space", 0, 1),
    ("translate", 3, 3),
    ("not", 1, 1),
    ("true", 0, 0),
    ("false", 0, 0),
    ("boolean", 1, 1),
    ("number", 0, 1),
    ("sum", 1, 1),
    ("floor", 1, 1),
    ("ceiling", 1, 1),
    ("round", 1, 1),
];

impl XPath {
    /// Parses an XPath 1.0 expression.
    ///
    /// # Errors
    ///
    /// If it is malformed, with where.
    pub fn parse(text: &str) -> Result<Self, QueryError> {
        let mut cursor = Cursor::new(text, LANGUAGE);
        let expr = or(&mut cursor)?;
        cursor.space();
        if !cursor.at_end() {
            return Err(cursor.error("unexpected text"));
        }
        Ok(Self { expr })
    }

    /// Reads `xml` and evaluates the expression with the document as the
    /// context: the text of each selected node, in document order, or the
    /// single value of an expression that is not a node set.
    ///
    /// # Errors
    ///
    /// If `xml` is not XML, or the expression visits more than
    /// [`crate::MAX_VISITS`] nodes.
    pub fn evaluate(&self, xml: &str) -> Result<Vec<Json>, QueryError> {
        let options = ParsingOptions {
            // roxmltree guards against entity expansion attacks itself.
            allow_dtd: true,
            ..ParsingOptions::default()
        };
        let document = Document::parse_with_options(xml, options)
            .map_err(|e| QueryError::NotXml(e.to_string()))?;
        let eval = Eval {
            budget: Budget::default(),
        };
        let context = Context {
            node: XNode::Node(document.root()),
            position: 1,
            size: 1,
        };
        Ok(match eval.expr(&self.expr, &context)? {
            Value::Nodes(nodes) => nodes.iter().map(|n| Json::String(n.string())).collect(),
            Value::Number(n) => vec![number_json(n)],
            Value::Text(text) => vec![Json::String(text)],
            Value::Bool(b) => vec![Json::Bool(b)],
        })
    }
}

fn number_json(n: f64) -> Json {
    if n.is_finite() {
        Json::Number(number_text(n))
    } else {
        Json::String(number_text(n))
    }
}

// ---------------------------------------------------------------- parsing

/// Builds the expression of a binary operator from its operands.
type Make = Box<dyn Fn(Expr, Expr) -> Expr>;

fn binary(
    cursor: &mut Cursor<'_>,
    next: fn(&mut Cursor<'_>) -> Result<Expr, QueryError>,
    operator: fn(&mut Cursor<'_>) -> Option<Make>,
) -> Result<Expr, QueryError> {
    cursor.enter()?;
    let mut left = next(cursor)?;
    loop {
        cursor.space();
        let Some(make) = operator(cursor) else {
            break;
        };
        let right = next(cursor)?;
        left = make(left, right);
    }
    cursor.leave();
    Ok(left)
}

/// Takes the operator name `word` when it is not the start of a longer name.
fn word(cursor: &mut Cursor<'_>, word: &str) -> bool {
    let rest = cursor.rest();
    let follows = rest[word.len().min(rest.len())..].chars().next();
    if rest.starts_with(word) && !follows.is_some_and(name_char) {
        cursor.at += word.len();
        true
    } else {
        false
    }
}

fn or(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, and, |c| {
        word(c, "or").then(|| Box::new(|a, b| Expr::Or(Box::new(a), Box::new(b))) as _)
    })
}

fn and(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, equality, |c| {
        word(c, "and").then(|| Box::new(|a, b| Expr::And(Box::new(a), Box::new(b))) as _)
    })
}

fn compare_with(comparison: Comparison) -> Make {
    Box::new(move |a, b| Expr::Compare(Box::new(a), comparison, Box::new(b)))
}

fn equality(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, relational, |c| {
        if c.eat("!=") {
            Some(compare_with(Comparison::NotEqual))
        } else if c.eat("=") {
            Some(compare_with(Comparison::Equal))
        } else {
            None
        }
    })
}

fn relational(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, additive, |c| {
        let comparison = if c.eat("<=") {
            Comparison::LessOrEqual
        } else if c.eat(">=") {
            Comparison::GreaterOrEqual
        } else if c.eat("<") {
            Comparison::Less
        } else if c.eat(">") {
            Comparison::Greater
        } else {
            return None;
        };
        Some(compare_with(comparison))
    })
}

fn arithmetic_with(operator: Arithmetic) -> Make {
    Box::new(move |a, b| Expr::Arithmetic(Box::new(a), operator, Box::new(b)))
}

fn additive(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, multiplicative, |c| {
        if c.eat("+") {
            Some(arithmetic_with(Arithmetic::Add))
        } else if c.eat("-") {
            Some(arithmetic_with(Arithmetic::Subtract))
        } else {
            None
        }
    })
}

fn multiplicative(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    binary(cursor, unary, |c| {
        if c.eat("*") {
            Some(arithmetic_with(Arithmetic::Multiply))
        } else if word(c, "div") {
            Some(arithmetic_with(Arithmetic::Divide))
        } else if word(c, "mod") {
            Some(arithmetic_with(Arithmetic::Modulo))
        } else {
            None
        }
    })
}

fn unary(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    cursor.space();
    if cursor.eat("-") {
        cursor.enter()?;
        let inner = unary(cursor)?;
        cursor.leave();
        return Ok(Expr::Negate(Box::new(inner)));
    }
    binary(cursor, path, |c| {
        c.eat("|")
            .then(|| Box::new(|a, b| Expr::Union(Box::new(a), Box::new(b))) as _)
    })
}

fn name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{b7}')
}

fn ncname<'a>(cursor: &mut Cursor<'a>) -> &'a str {
    if cursor.peek().is_some_and(name_start) {
        cursor.take_while(name_char)
    } else {
        ""
    }
}

/// What follows a name, after spaces, without taking it.
fn after_name<'a>(cursor: &Cursor<'a>, name_end: usize) -> &'a str {
    cursor.text[name_end..].trim_start()
}

fn path(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    cursor.space();
    if cursor.peek() == Some('/') {
        let mut steps = Vec::new();
        if cursor.eat("//") {
            steps.push(descendant_or_self());
            relative(cursor, &mut steps)?;
        } else {
            cursor.bump();
            cursor.space();
            if starts_step(cursor) {
                relative(cursor, &mut steps)?;
            }
        }
        return Ok(Expr::Path {
            absolute: true,
            steps,
        });
    }
    if starts_primary(cursor) {
        let primary = primary(cursor)?;
        let predicates = predicates(cursor)?;
        cursor.space();
        let mut steps = Vec::new();
        if cursor.eat("//") {
            steps.push(descendant_or_self());
            relative(cursor, &mut steps)?;
        } else if cursor.eat("/") {
            relative(cursor, &mut steps)?;
        }
        if predicates.is_empty() && steps.is_empty() {
            return Ok(primary);
        }
        return Ok(Expr::Filter {
            primary: Box::new(primary),
            predicates,
            steps,
        });
    }
    let mut steps = Vec::new();
    relative(cursor, &mut steps)?;
    Ok(Expr::Path {
        absolute: false,
        steps,
    })
}

fn descendant_or_self() -> Step {
    Step {
        axis: Axis::DescendantOrSelf,
        test: Test::Node,
        predicates: Vec::new(),
    }
}

fn starts_step(cursor: &Cursor<'_>) -> bool {
    cursor
        .peek()
        .is_some_and(|c| name_start(c) || matches!(c, '.' | '@' | '*'))
}

fn starts_primary(cursor: &Cursor<'_>) -> bool {
    match cursor.peek() {
        Some('(' | '\'' | '"' | '$') => true,
        Some(c) if c.is_ascii_digit() => true,
        Some('.') => cursor.peek_second().is_some_and(|c| c.is_ascii_digit()),
        Some(c) if name_start(c) => {
            let mut probe = Cursor::new(cursor.text, LANGUAGE);
            probe.at = cursor.at;
            let name = qname_text(&mut probe);
            let follows = after_name(&probe, probe.at);
            follows.starts_with('(') && !is_node_type(name)
        }
        _ => false,
    }
}

fn is_node_type(name: &str) -> bool {
    matches!(name, "text" | "node" | "comment" | "processing-instruction")
}

/// A name with an optional `prefix:`, as written.
fn qname_text<'a>(cursor: &mut Cursor<'a>) -> &'a str {
    let start = cursor.at;
    ncname(cursor);
    if cursor.rest().starts_with(':') && !cursor.rest().starts_with("::") {
        let before = cursor.at;
        cursor.bump();
        if ncname(cursor).is_empty() {
            cursor.at = before;
        }
    }
    &cursor.text[start..cursor.at]
}

fn primary(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    match cursor.peek() {
        Some('(') => {
            cursor.bump();
            let inner = or(cursor)?;
            cursor.space();
            cursor.expect(")")?;
            Ok(inner)
        }
        Some('\'' | '"') => Ok(Expr::Literal(cursor.quoted(false)?)),
        Some('$') => Err(cursor.error("variables are not supported")),
        Some(c) if c.is_ascii_digit() || c == '.' => {
            let text = cursor.take_while(|c| c.is_ascii_digit() || c == '.');
            text.parse()
                .map(Expr::Number)
                .map_err(|_| cursor.error("invalid number"))
        }
        _ => call(cursor),
    }
}

fn call(cursor: &mut Cursor<'_>) -> Result<Expr, QueryError> {
    let name = qname_text(cursor).to_owned();
    let Some(&(_, least, most)) = FUNCTIONS.iter().find(|(n, _, _)| *n == name) else {
        return Err(cursor.error(format!("unknown function `{name}`")));
    };
    cursor.space();
    cursor.expect("(")?;
    cursor.enter()?;
    let mut args = Vec::new();
    cursor.space();
    if !cursor.eat(")") {
        loop {
            args.push(or(cursor)?);
            cursor.space();
            if cursor.eat(")") {
                break;
            }
            cursor.expect(",")?;
        }
    }
    cursor.leave();
    if args.len() < least || args.len() > most {
        return Err(cursor.error(format!("wrong number of arguments to `{name}`")));
    }
    Ok(Expr::Call(name, args))
}

fn relative(cursor: &mut Cursor<'_>, steps: &mut Vec<Step>) -> Result<(), QueryError> {
    loop {
        cursor.space();
        steps.push(step(cursor)?);
        cursor.space();
        if cursor.eat("//") {
            steps.push(descendant_or_self());
        } else if !cursor.eat("/") {
            return Ok(());
        }
    }
}

fn step(cursor: &mut Cursor<'_>) -> Result<Step, QueryError> {
    if cursor.eat("..") {
        return Ok(Step {
            axis: Axis::Parent,
            test: Test::Node,
            predicates: Vec::new(),
        });
    }
    if cursor.peek() == Some('.') {
        cursor.bump();
        return Ok(Step {
            axis: Axis::SelfNode,
            test: Test::Node,
            predicates: Vec::new(),
        });
    }
    let axis = if cursor.eat("@") {
        Axis::Attribute
    } else {
        let start = cursor.at;
        let name = ncname(cursor);
        cursor.space();
        if !name.is_empty() && cursor.eat("::") {
            Axis::named(name).ok_or_else(|| cursor.error(format!("unknown axis `{name}`")))?
        } else {
            cursor.at = start;
            Axis::Child
        }
    };
    cursor.space();
    let test = test(cursor)?;
    let predicates = predicates(cursor)?;
    Ok(Step {
        axis,
        test,
        predicates,
    })
}

fn test(cursor: &mut Cursor<'_>) -> Result<Test, QueryError> {
    if cursor.eat("*") {
        return Ok(Test::Name {
            prefix: None,
            local: None,
        });
    }
    let prefix = ncname(cursor);
    if prefix.is_empty() {
        return Err(cursor.error("expected a step"));
    }
    if cursor.rest().starts_with(':') && !cursor.rest().starts_with("::") {
        cursor.bump();
        if cursor.eat("*") {
            return Ok(Test::Name {
                prefix: Some(prefix.to_owned()),
                local: None,
            });
        }
        let local = ncname(cursor);
        if local.is_empty() {
            return Err(cursor.error("expected a name after the prefix"));
        }
        return Ok(Test::Name {
            prefix: Some(prefix.to_owned()),
            local: Some(local.to_owned()),
        });
    }
    let name_end = cursor.at;
    if is_node_type(prefix) && after_name(cursor, name_end).starts_with('(') {
        cursor.space();
        cursor.expect("(")?;
        cursor.space();
        let test = match prefix {
            "text" => Test::Text,
            "comment" => Test::Comment,
            "node" => Test::Node,
            _ => {
                let target = if matches!(cursor.peek(), Some('\'' | '"')) {
                    Some(cursor.quoted(false)?)
                } else {
                    None
                };
                cursor.space();
                Test::Instruction(target)
            }
        };
        cursor.expect(")")?;
        return Ok(test);
    }
    cursor.at = name_end;
    Ok(Test::Name {
        prefix: None,
        local: Some(prefix.to_owned()),
    })
}

fn predicates(cursor: &mut Cursor<'_>) -> Result<Vec<Expr>, QueryError> {
    let mut predicates = Vec::new();
    loop {
        cursor.space();
        if !cursor.eat("[") {
            return Ok(predicates);
        }
        cursor.enter()?;
        predicates.push(or(cursor)?);
        cursor.leave();
        cursor.space();
        cursor.expect("]")?;
    }
}

// ------------------------------------------------------------- evaluation

/// A node of the document, or one of an element's attributes (roxmltree
/// keeps those apart from nodes).
#[derive(Debug, Clone, Copy)]
enum XNode<'a, 'i> {
    Node(Node<'a, 'i>),
    Attribute(Node<'a, 'i>, usize),
}

impl<'a, 'i> XNode<'a, 'i> {
    /// Its place in document order: an element's attributes come right after
    /// it and before its children, which have larger ids.
    fn order(&self) -> (usize, usize) {
        match self {
            Self::Node(n) => (n.id().get_usize(), 0),
            Self::Attribute(n, i) => (n.id().get_usize(), i + 1),
        }
    }

    /// The node itself, or the element an attribute is on.
    fn node(&self) -> Node<'a, 'i> {
        match self {
            Self::Node(n) | Self::Attribute(n, _) => *n,
        }
    }

    /// Its string value: the text in it, or the attribute's value.
    fn string(&self) -> String {
        match self {
            Self::Attribute(n, i) => n
                .attributes()
                .nth(*i)
                .map(|a| a.value().to_owned())
                .unwrap_or_default(),
            Self::Node(n) if n.is_element() || n.is_root() => n
                .descendants()
                .filter(Node::is_text)
                .filter_map(|t| t.text())
                .collect(),
            Self::Node(n) => match n.pi() {
                Some(pi) => pi.value.unwrap_or_default().to_owned(),
                None => n.text().unwrap_or_default().to_owned(),
            },
        }
    }

    /// Its local name and namespace, if it has a name.
    fn name(&self) -> Option<(&'i str, Option<&'a str>)> {
        match self {
            Self::Attribute(n, i) => n.attributes().nth(*i).map(|a| (a.name(), a.namespace())),
            Self::Node(n) if n.is_element() => {
                Some((n.tag_name().name(), n.tag_name().namespace()))
            }
            Self::Node(n) => n.pi().map(|pi| (pi.target, None)),
        }
    }
}

enum Value<'a, 'i> {
    Nodes(Vec<XNode<'a, 'i>>),
    Number(f64),
    Text(String),
    Bool(bool),
}

impl Value<'_, '_> {
    fn boolean(&self) -> bool {
        match self {
            Self::Nodes(nodes) => !nodes.is_empty(),
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Text(text) => !text.is_empty(),
            Self::Bool(b) => *b,
        }
    }

    fn number(&self) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::Bool(b) => f64::from(u8::from(*b)),
            _ => text_number(&self.text()),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Nodes(nodes) => nodes.first().map(XNode::string).unwrap_or_default(),
            Self::Number(n) => number_text(*n),
            Self::Text(text) => text.clone(),
            Self::Bool(b) => b.to_string(),
        }
    }
}

fn text_number(text: &str) -> f64 {
    let text = text.trim();
    let digits = text.strip_prefix('-').unwrap_or(text);
    let valid = !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && digits.matches('.').count() <= 1
        && digits != ".";
    if valid {
        text.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

#[expect(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    reason = "an integral value below 1e15 is exact as i64"
)]
fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else if n == n.trunc() && n.abs() < 1e15 {
        // Also turns -0 into 0, as XPath does.
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[derive(Clone, Copy)]
struct Context<'a, 'i> {
    node: XNode<'a, 'i>,
    position: usize,
    size: usize,
}

struct Eval {
    budget: Budget,
}

impl Eval {
    fn expr<'a, 'i>(
        &self,
        expr: &Expr,
        context: &Context<'a, 'i>,
    ) -> Result<Value<'a, 'i>, QueryError> {
        Ok(match expr {
            Expr::Or(a, b) => {
                Value::Bool(self.expr(a, context)?.boolean() || self.expr(b, context)?.boolean())
            }
            Expr::And(a, b) => {
                Value::Bool(self.expr(a, context)?.boolean() && self.expr(b, context)?.boolean())
            }
            Expr::Compare(a, comparison, b) => {
                let a = self.expr(a, context)?;
                let b = self.expr(b, context)?;
                Value::Bool(compare(&a, *comparison, &b))
            }
            Expr::Arithmetic(a, operator, b) => {
                let a = self.expr(a, context)?.number();
                let b = self.expr(b, context)?.number();
                Value::Number(match operator {
                    Arithmetic::Add => a + b,
                    Arithmetic::Subtract => a - b,
                    Arithmetic::Multiply => a * b,
                    Arithmetic::Divide => a / b,
                    Arithmetic::Modulo => a % b,
                })
            }
            Expr::Negate(inner) => Value::Number(-self.expr(inner, context)?.number()),
            Expr::Union(a, b) => {
                let mut nodes = self.nodes(a, context)?;
                nodes.extend(self.nodes(b, context)?);
                Value::Nodes(sorted(nodes))
            }
            Expr::Path { absolute, steps } => {
                let start = if *absolute {
                    XNode::Node(context.node.node().document().root())
                } else {
                    context.node
                };
                Value::Nodes(self.steps(steps, vec![start])?)
            }
            Expr::Filter {
                primary,
                predicates,
                steps,
            } => {
                let nodes = self.nodes(primary, context)?;
                let nodes = self.filter(nodes, predicates)?;
                Value::Nodes(self.steps(steps, nodes)?)
            }
            Expr::Literal(text) => Value::Text(text.clone()),
            Expr::Number(n) => Value::Number(*n),
            Expr::Call(name, args) => self.call(name, args, context)?,
        })
    }

    fn nodes<'a, 'i>(
        &self,
        expr: &Expr,
        context: &Context<'a, 'i>,
    ) -> Result<Vec<XNode<'a, 'i>>, QueryError> {
        match self.expr(expr, context)? {
            Value::Nodes(nodes) => Ok(nodes),
            _ => Err(QueryError::Syntax {
                language: LANGUAGE,
                expression: String::new(),
                reason: "expected a node set".into(),
                at: 0,
            }),
        }
    }

    fn steps<'a, 'i>(
        &self,
        steps: &[Step],
        mut nodes: Vec<XNode<'a, 'i>>,
    ) -> Result<Vec<XNode<'a, 'i>>, QueryError> {
        for step in steps {
            let mut out = Vec::new();
            for node in &nodes {
                let candidates: Vec<XNode<'a, 'i>> = axis(*node, step.axis)
                    .into_iter()
                    .filter(|n| matches(n, &step.test, step.axis))
                    .collect();
                self.budget.spend(candidates.len().max(1))?;
                out.extend(self.filter(candidates, &step.predicates)?);
            }
            nodes = sorted(out);
        }
        Ok(nodes)
    }

    /// Keeps the nodes each predicate holds for, with positions counted in
    /// the order the nodes come in.
    fn filter<'a, 'i>(
        &self,
        mut nodes: Vec<XNode<'a, 'i>>,
        predicates: &[Expr],
    ) -> Result<Vec<XNode<'a, 'i>>, QueryError> {
        for predicate in predicates {
            let size = nodes.len();
            let mut kept = Vec::new();
            for (i, node) in nodes.into_iter().enumerate() {
                self.budget.spend(1)?;
                let context = Context {
                    node,
                    position: i + 1,
                    size,
                };
                let holds = match self.expr(predicate, &context)? {
                    #[expect(
                        clippy::cast_precision_loss,
                        clippy::float_cmp,
                        reason = "positions are small integers, exact in f64"
                    )]
                    Value::Number(n) => n == (i + 1) as f64,
                    other => other.boolean(),
                };
                if holds {
                    kept.push(node);
                }
            }
            nodes = kept;
        }
        Ok(nodes)
    }

    fn call<'a, 'i>(
        &self,
        name: &str,
        args: &[Expr],
        context: &Context<'a, 'i>,
    ) -> Result<Value<'a, 'i>, QueryError> {
        let arg = |i: usize| self.expr(&args[i], context);
        let text_arg = |i: usize| -> Result<String, QueryError> {
            match args.get(i) {
                Some(expr) => Ok(self.expr(expr, context)?.text()),
                None => Ok(context.node.string()),
            }
        };
        let node_arg = |i: usize| -> Result<Option<XNode<'a, 'i>>, QueryError> {
            match args.get(i) {
                Some(expr) => Ok(self.nodes(expr, context)?.first().copied()),
                None => Ok(Some(context.node)),
            }
        };
        #[expect(clippy::cast_precision_loss, reason = "counts are far below 2^52")]
        let count = |n: usize| Value::Number(n as f64);
        Ok(match name {
            "last" => count(context.size),
            "position" => count(context.position),
            "count" => count(self.nodes(&args[0], context)?.len()),
            "local-name" => Value::Text(
                node_arg(0)?
                    .and_then(|n| n.name())
                    .map(|(l, _)| l.to_owned())
                    .unwrap_or_default(),
            ),
            "name" => Value::Text(node_arg(0)?.map(|n| qualified_name(&n)).unwrap_or_default()),
            "namespace-uri" => Value::Text(
                node_arg(0)?
                    .and_then(|n| n.name())
                    .and_then(|(_, ns)| ns)
                    .unwrap_or_default()
                    .to_owned(),
            ),
            "string" => Value::Text(text_arg(0)?),
            "concat" => {
                let mut out = String::new();
                for i in 0..args.len() {
                    out.push_str(&text_arg(i)?);
                }
                Value::Text(out)
            }
            "starts-with" => Value::Bool(text_arg(0)?.starts_with(&text_arg(1)?)),
            "ends-with" => Value::Bool(text_arg(0)?.ends_with(&text_arg(1)?)),
            "contains" => Value::Bool(text_arg(0)?.contains(&text_arg(1)?)),
            "substring-before" => {
                let text = text_arg(0)?;
                Value::Text(
                    text.split_once(&text_arg(1)?)
                        .map(|(a, _)| a.to_owned())
                        .unwrap_or_default(),
                )
            }
            "substring-after" => {
                let text = text_arg(0)?;
                let needle = text_arg(1)?;
                Value::Text(
                    text.split_once(&needle)
                        .map(|(_, b)| b.to_owned())
                        .unwrap_or_default(),
                )
            }
            "substring" => {
                let length = if args.len() == 3 {
                    arg(2)?.number()
                } else {
                    f64::INFINITY
                };
                Value::Text(substring(&text_arg(0)?, arg(1)?.number(), length))
            }
            "string-length" => count(text_arg(0)?.chars().count()),
            "normalize-space" => Value::Text(
                text_arg(0)?
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            "translate" => Value::Text(translate(&text_arg(0)?, &text_arg(1)?, &text_arg(2)?)),
            "not" => Value::Bool(!arg(0)?.boolean()),
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "boolean" => Value::Bool(arg(0)?.boolean()),
            "number" => Value::Number(match args.first() {
                Some(expr) => self.expr(expr, context)?.number(),
                None => text_number(&context.node.string()),
            }),
            "sum" => Value::Number(
                self.nodes(&args[0], context)?
                    .iter()
                    .map(|n| text_number(&n.string()))
                    .sum(),
            ),
            "floor" => Value::Number(arg(0)?.number().floor()),
            "ceiling" => Value::Number(arg(0)?.number().ceil()),
            "round" => Value::Number(round(arg(0)?.number())),
            _ => unreachable!("functions are checked when parsing"),
        })
    }
}

fn qualified_name(node: &XNode<'_, '_>) -> String {
    let Some((local, namespace)) = node.name() else {
        return String::new();
    };
    match namespace.and_then(|ns| node.node().lookup_prefix(ns)) {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{local}"),
        _ => local.to_owned(),
    }
}

fn round(n: f64) -> f64 {
    if n.is_finite() { (n + 0.5).floor() } else { n }
}

/// XPath's `substring`: the characters at positions `p` (from 1) with
/// `round(start) <= p < round(start) + round(length)`.
fn substring(text: &str, start: f64, length: f64) -> String {
    let first = round(start);
    let end = first + round(length);
    text.chars()
        .enumerate()
        .filter(|(i, _)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "string positions are far below 2^52"
            )]
            let p = (i + 1) as f64;
            p >= first && p < end
        })
        .map(|(_, c)| c)
        .collect()
}

fn translate(text: &str, from: &str, to: &str) -> String {
    let from: Vec<char> = from.chars().collect();
    let to: Vec<char> = to.chars().collect();
    text.chars()
        .filter_map(|c| match from.iter().position(|f| *f == c) {
            Some(i) => to.get(i).copied(),
            None => Some(c),
        })
        .collect()
}

fn sorted<'a, 'i>(mut nodes: Vec<XNode<'a, 'i>>) -> Vec<XNode<'a, 'i>> {
    nodes.sort_by_key(XNode::order);
    nodes.dedup_by_key(|n| n.order());
    nodes
}

/// The nodes on `axis` from `node`, in the axis's order: the reverse axes
/// (parent, ancestors, preceding) nearest first, so positions in their
/// predicates count backwards.
fn axis<'a, 'i>(node: XNode<'a, 'i>, axis: Axis) -> Vec<XNode<'a, 'i>> {
    let wrap = |n: Node<'a, 'i>| XNode::Node(n);
    let (element, on_attribute) = match node {
        XNode::Node(n) => (n, false),
        XNode::Attribute(n, _) => (n, true),
    };
    match axis {
        Axis::SelfNode => vec![node],
        Axis::Attribute if !on_attribute && element.is_element() => (0..element.attributes().len())
            .map(|i| XNode::Attribute(element, i))
            .collect(),
        Axis::Child
        | Axis::Descendant
        | Axis::DescendantOrSelf
        | Axis::Attribute
        | Axis::FollowingSibling
        | Axis::PrecedingSibling
            if on_attribute =>
        {
            if axis == Axis::DescendantOrSelf {
                vec![node]
            } else {
                Vec::new()
            }
        }
        Axis::Attribute => Vec::new(),
        Axis::Child => element.children().map(wrap).collect(),
        Axis::Descendant => element.descendants().skip(1).map(wrap).collect(),
        Axis::DescendantOrSelf => element.descendants().map(wrap).collect(),
        Axis::Parent if on_attribute => vec![wrap(element)],
        Axis::Parent => element.parent().map(wrap).into_iter().collect(),
        Axis::Ancestor | Axis::AncestorOrSelf => {
            let mut out: Vec<XNode<'a, 'i>> = Vec::new();
            if axis == Axis::AncestorOrSelf {
                out.push(node);
            }
            let skip = usize::from(!on_attribute);
            out.extend(element.ancestors().skip(skip).map(wrap));
            out
        }
        Axis::FollowingSibling => element.next_siblings().skip(1).map(wrap).collect(),
        Axis::PrecedingSibling => element.prev_siblings().skip(1).map(wrap).collect(),
        Axis::Following => {
            let after = element
                .descendants()
                .next_back()
                .unwrap_or(element)
                .id()
                .get_usize();
            let mut out: Vec<XNode<'a, 'i>> = Vec::new();
            if on_attribute {
                out.extend(element.descendants().skip(1).map(wrap));
            }
            out.extend(
                element
                    .document()
                    .descendants()
                    .filter(|n| n.id().get_usize() > after)
                    .map(wrap),
            );
            out
        }
        Axis::Preceding => {
            let ancestors: Vec<_> = element.ancestors().map(|n| n.id()).collect();
            let mut out: Vec<XNode<'a, 'i>> = element
                .document()
                .descendants()
                .take_while(|n| n.id().get_usize() < element.id().get_usize())
                .filter(|n| !ancestors.contains(&n.id()))
                .map(wrap)
                .collect();
            out.reverse();
            out
        }
    }
}

fn matches(node: &XNode<'_, '_>, test: &Test, axis: Axis) -> bool {
    match test {
        Test::Node => true,
        Test::Text => matches!(node, XNode::Node(n) if n.is_text()),
        Test::Comment => matches!(node, XNode::Node(n) if n.is_comment()),
        Test::Instruction(target) => match node {
            XNode::Node(n) => n
                .pi()
                .is_some_and(|pi| target.as_deref().is_none_or(|t| t == pi.target)),
            XNode::Attribute(..) => false,
        },
        Test::Name { prefix, local } => {
            // The principal node type: attributes on the attribute axis,
            // elements elsewhere.
            let principal = match node {
                XNode::Attribute(..) => axis == Axis::Attribute,
                XNode::Node(n) => axis != Axis::Attribute && n.is_element(),
            };
            if !principal {
                return false;
            }
            let Some((name, namespace)) = node.name() else {
                return false;
            };
            if local.as_deref().is_some_and(|l| l != name) {
                return false;
            }
            match prefix {
                None => true,
                Some(prefix) => {
                    let declared = node.node().lookup_namespace_uri(Some(prefix));
                    declared.is_some() && declared == namespace
                }
            }
        }
    }
}

fn compare(a: &Value<'_, '_>, comparison: Comparison, b: &Value<'_, '_>) -> bool {
    match (a, b) {
        (Value::Nodes(nodes), Value::Bool(other)) => atoms(
            &Atom::Bool(!nodes.is_empty()),
            comparison,
            &Atom::Bool(*other),
        ),
        (Value::Bool(other), Value::Nodes(nodes)) => atoms(
            &Atom::Bool(*other),
            comparison,
            &Atom::Bool(!nodes.is_empty()),
        ),
        _ => {
            let left = expand(a);
            let right = expand(b);
            left.iter()
                .any(|x| right.iter().any(|y| atoms(x, comparison, y)))
        }
    }
}

enum Atom {
    Number(f64),
    Text(String),
    Bool(bool),
}

impl Atom {
    fn number(&self) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::Text(text) => text_number(text),
            Self::Bool(b) => f64::from(u8::from(*b)),
        }
    }

    fn boolean(&self) -> bool {
        match self {
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Text(text) => !text.is_empty(),
            Self::Bool(b) => *b,
        }
    }
}

fn expand(value: &Value<'_, '_>) -> Vec<Atom> {
    match value {
        Value::Nodes(nodes) => nodes.iter().map(|n| Atom::Text(n.string())).collect(),
        Value::Number(n) => vec![Atom::Number(*n)],
        Value::Text(text) => vec![Atom::Text(text.clone())],
        Value::Bool(b) => vec![Atom::Bool(*b)],
    }
}

#[expect(clippy::float_cmp, reason = "XPath compares numbers exactly")]
fn atoms(a: &Atom, comparison: Comparison, b: &Atom) -> bool {
    let equal = || match (a, b) {
        (Atom::Bool(_), _) | (_, Atom::Bool(_)) => a.boolean() == b.boolean(),
        (Atom::Number(_), _) | (_, Atom::Number(_)) => a.number() == b.number(),
        (Atom::Text(x), Atom::Text(y)) => x == y,
    };
    match comparison {
        Comparison::Equal => equal(),
        Comparison::NotEqual => !equal(),
        Comparison::Less => a.number() < b.number(),
        Comparison::LessOrEqual => a.number() <= b.number(),
        Comparison::Greater => a.number() > b.number(),
        Comparison::GreaterOrEqual => a.number() >= b.number(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_and_print_as_in_xpath() {
        assert_eq!(text_number(" 12.5 "), 12.5);
        assert!(text_number("1e3").is_nan());
        assert!(text_number("").is_nan());
        assert!(text_number(".").is_nan());
        assert_eq!(number_text(3.0), "3");
        assert_eq!(number_text(-0.0), "0");
        assert_eq!(number_text(0.5), "0.5");
        assert_eq!(number_text(f64::NAN), "NaN");
        assert_eq!(number_text(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn string_functions_follow_the_standard() {
        assert_eq!(substring("12345", 1.5, 2.6), "234");
        assert_eq!(substring("12345", 0.0, 3.0), "12");
        assert_eq!(substring("12345", f64::NAN, 3.0), "");
        assert_eq!(translate("--aaa--", "abc-", "ABC"), "AAA");
        assert_eq!(round(-0.5), 0.0);
        assert_eq!(round(2.5), 3.0);
    }
}
