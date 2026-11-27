//! What a request checks and keeps from its response, without code
//! (docs/08): assertions (`runtime.assertions`) and captures
//! (`runtime.actions` of type `set-variable`), as Bruno writes them.

/// An operator of an assertion. The list is Bruno's, so imported
/// collections work unchanged; one this version does not know is kept as
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operator {
    /// Equal.
    Eq,
    /// Not equal.
    Neq,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Between two values, both included: `"1,10"`.
    Between,
    /// One of a list: `"a,b,c"`.
    In,
    /// None of a list.
    NotIn,
    /// Contains: text in text, or an item in a list.
    Contains,
    /// Does not contain.
    NotContains,
    /// Text starts with.
    StartsWith,
    /// Text ends with.
    EndsWith,
    /// Text matches a regular expression.
    Matches,
    /// Text does not match a regular expression.
    NotMatches,
    /// Length of a text, list or object.
    Length,
    /// Empty text, list or object.
    IsEmpty,
    /// Not empty.
    IsNotEmpty,
    /// `null`.
    IsNull,
    /// Not there at all.
    IsUndefined,
    /// There, even if `null`.
    IsDefined,
    /// Truthy, as in JavaScript.
    IsTruthy,
    /// Falsy, as in JavaScript.
    IsFalsy,
    /// A JSON object.
    IsJson,
    /// A number.
    IsNumber,
    /// A text.
    IsString,
    /// A boolean.
    IsBoolean,
    /// A list.
    IsArray,
    /// One this version does not know, as written.
    Other(String),
}

/// Every known operator with its name in the files.
const NAMES: &[(&str, Operator)] = &[
    ("eq", Operator::Eq),
    ("neq", Operator::Neq),
    ("gt", Operator::Gt),
    ("gte", Operator::Gte),
    ("lt", Operator::Lt),
    ("lte", Operator::Lte),
    ("between", Operator::Between),
    ("in", Operator::In),
    ("notIn", Operator::NotIn),
    ("contains", Operator::Contains),
    ("notContains", Operator::NotContains),
    ("startsWith", Operator::StartsWith),
    ("endsWith", Operator::EndsWith),
    ("matches", Operator::Matches),
    ("notMatches", Operator::NotMatches),
    ("length", Operator::Length),
    ("isEmpty", Operator::IsEmpty),
    ("isNotEmpty", Operator::IsNotEmpty),
    ("isNull", Operator::IsNull),
    ("isUndefined", Operator::IsUndefined),
    ("isDefined", Operator::IsDefined),
    ("isTruthy", Operator::IsTruthy),
    ("isFalsy", Operator::IsFalsy),
    ("isJson", Operator::IsJson),
    ("isNumber", Operator::IsNumber),
    ("isString", Operator::IsString),
    ("isBoolean", Operator::IsBoolean),
    ("isArray", Operator::IsArray),
];

impl Operator {
    /// The operator named `name` in a file.
    pub fn parse(name: &str) -> Self {
        NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| Self::Other(name.to_owned()), |(_, op)| op.clone())
    }

    /// Its name in the files.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Other(name) => name,
            known => NAMES
                .iter()
                .find(|(_, op)| op == known)
                .map_or("", |(name, _)| name),
        }
    }

    /// Every known operator, in the order of `docs/08`.
    pub fn all() -> impl Iterator<Item = Self> {
        NAMES.iter().map(|(_, op)| op.clone())
    }

    /// Whether it compares against an expected value.
    pub fn takes_value(&self) -> bool {
        !matches!(
            self,
            Self::IsEmpty
                | Self::IsNotEmpty
                | Self::IsNull
                | Self::IsUndefined
                | Self::IsDefined
                | Self::IsTruthy
                | Self::IsFalsy
                | Self::IsJson
                | Self::IsNumber
                | Self::IsString
                | Self::IsBoolean
                | Self::IsArray
        )
    }
}

/// A check on the response: `res.status eq 200`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    /// What is checked: `res.status`, `res.body.items[0].id`…
    pub expression: String,
    /// How.
    pub operator: Operator,
    /// The expected value, as written (it may hold `{{variables}}`); `None`
    /// for operators that take none.
    pub value: Option<String>,
    /// When `false`, it is not checked.
    pub enabled: bool,
}

impl Assertion {
    /// An enabled assertion.
    pub fn new(expression: impl Into<String>, operator: Operator, value: Option<&str>) -> Self {
        Self {
            expression: expression.into(),
            operator,
            value: value.map(str::to_owned),
            enabled: true,
        }
    }
}

/// Where a captured value goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureScope {
    /// Kept until the collection is closed or the run ends.
    Runtime,
    /// Only for this request.
    Request,
    /// Written to the active environment.
    Environment,
    /// Written to the collection's variables (`opencollection.yml`).
    Collection,
    /// One this version does not know, as written.
    Other(String),
}

impl CaptureScope {
    /// The scope named `name` in a file.
    pub fn parse(name: &str) -> Self {
        match name {
            "runtime" => Self::Runtime,
            "request" => Self::Request,
            "environment" => Self::Environment,
            "collection" => Self::Collection,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Its name in the files.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Runtime => "runtime",
            Self::Request => "request",
            Self::Environment => "environment",
            Self::Collection => "collection",
            Self::Other(name) => name,
        }
    }
}

/// A value taken from the response into a variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// Where the value is: `res.body.access_token`.
    pub expression: String,
    /// How `expression` is read (`jsonq` in Bruno's files).
    pub method: String,
    /// When it runs (`after-response` in Bruno's files).
    pub phase: String,
    /// The variable it sets.
    pub variable: String,
    /// Where the variable lives.
    pub scope: CaptureScope,
    /// When `false`, it does not run.
    pub enabled: bool,
}

impl Capture {
    /// An enabled capture of `expression` into the runtime variable
    /// `variable`, after the response, as Bruno writes it.
    pub fn new(expression: impl Into<String>, variable: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            method: "jsonq".into(),
            phase: "after-response".into(),
            variable: variable.into(),
            scope: CaptureScope::Runtime,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_round_trip_by_name_and_unknown_ones_are_kept() {
        for operator in Operator::all() {
            assert_eq!(Operator::parse(operator.as_str()), operator);
        }
        assert_eq!(Operator::all().count(), 28);
        let unknown = Operator::parse("isFuture");
        assert_eq!(unknown, Operator::Other("isFuture".into()));
        assert_eq!(unknown.as_str(), "isFuture");
        assert!(Operator::Eq.takes_value() && !Operator::IsNull.takes_value());
    }

    #[test]
    fn scopes_round_trip_by_name() {
        for name in ["runtime", "request", "environment", "global"] {
            assert_eq!(CaptureScope::parse(name).as_str(), name);
        }
    }
}
