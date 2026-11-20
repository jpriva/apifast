/// When a script runs (`type` in a `scripts` list of a file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptKind {
    /// Before the request is rendered and sent (`before-request`).
    BeforeRequest,
    /// Once the response arrived (`after-response`).
    AfterResponse,
    /// Its tests, last (`tests`).
    Tests,
    /// One apifast does not run (such as Bruno's `grpc:before-call-start`),
    /// as written: kept, not run.
    Other(String),
}

impl ScriptKind {
    /// The kind named `name` in a file.
    pub fn parse(name: &str) -> Self {
        match name {
            "before-request" => Self::BeforeRequest,
            "after-response" => Self::AfterResponse,
            "tests" => Self::Tests,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Its name in a file.
    pub fn as_str(&self) -> &str {
        match self {
            Self::BeforeRequest => "before-request",
            Self::AfterResponse => "after-response",
            Self::Tests => "tests",
            Self::Other(name) => name,
        }
    }
}

/// A JavaScript script of a request, a folder or the collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptBlock {
    /// When it runs.
    pub kind: ScriptKind,
    /// Its source.
    pub code: String,
}

impl ScriptBlock {
    /// A script of `kind`.
    pub fn new(kind: ScriptKind, code: impl Into<String>) -> Self {
        Self {
            kind,
            code: code.into(),
        }
    }
}

/// The code of the scripts of `kind` in `scripts` that have any, joined in
/// order (a file may have several of one kind).
pub fn script_of(scripts: &[ScriptBlock], kind: &ScriptKind) -> Option<String> {
    let codes: Vec<&str> = (scripts.iter())
        .filter(|s| s.kind == *kind && !s.code.trim().is_empty())
        .map(|s| s.code.as_str())
        .collect();
    (!codes.is_empty()).then(|| codes.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_and_unknown_ones_are_kept() {
        for name in [
            "before-request",
            "after-response",
            "tests",
            "grpc:after-call-end",
        ] {
            assert_eq!(ScriptKind::parse(name).as_str(), name);
        }
    }

    #[test]
    fn blank_scripts_do_not_count() {
        let scripts = [
            ScriptBlock::new(ScriptKind::Tests, "  \n"),
            ScriptBlock::new(ScriptKind::BeforeRequest, "a()"),
            ScriptBlock::new(ScriptKind::BeforeRequest, "b()"),
        ];
        assert_eq!(script_of(&scripts, &ScriptKind::Tests), None);
        assert_eq!(
            script_of(&scripts, &ScriptKind::BeforeRequest).as_deref(),
            Some("a()\nb()")
        );
    }
}
