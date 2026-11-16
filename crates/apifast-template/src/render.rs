use std::borrow::Cow;

use apifast_domain::{ResolvedVariables, Secret};

use crate::dynamic;
use crate::parse::{Expr, Template, Token};

/// Maximum nesting of variables whose values contain templates.
pub const MAX_DEPTH: usize = 50;

/// Everything the renderer needs that is not pure: time, randomness, the
/// process environment, prompts and functions. The application layer
/// provides it.
pub trait Host {
    /// Current time, in milliseconds since the Unix epoch.
    fn unix_millis(&mut self) -> i64;

    /// A uniformly random number.
    fn random_u64(&mut self) -> u64;

    /// Value of `{{process.env.NAME}}`, from `.env` or the system.
    fn process_env(&self, _name: &str) -> Option<String> {
        None
    }

    /// Value the user typed for `{{?Label}}`.
    fn prompt(&mut self, _label: &str) -> Option<String> {
        None
    }

    /// Calls a template function such as `response(...)`.
    ///
    /// # Errors
    ///
    /// A message for the user when the function is unknown or fails. It must
    /// not contain secret values.
    fn call(&mut self, name: &str, _args: &[(String, FunctionArg)]) -> Result<String, String> {
        Err(format!("unknown function `{name}`"))
    }
}

/// A function argument, already rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionArg {
    /// Text: a string, a variable, a dynamic variable or another function.
    Text(String),
    /// `true` or `false`.
    Bool(bool),
    /// `null`
    Null,
}

/// What to do when a value is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Fail on the first problem. Used when sending.
    Strict,
    /// Leave missing values empty and report every problem. Used for previews.
    Lenient,
}

/// Something that kept a tag from rendering. Never contains secret values,
/// only names.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Problem {
    /// No scope defines the variable.
    #[error("variable `{0}` is not defined")]
    MissingVariable(String),
    /// The variable is a secret whose value is not available (vault locked or
    /// value not set).
    #[error("secret `{0}` has no value available")]
    SecretUnavailable(String),
    /// Variables reference each other in a loop.
    #[error("variables reference each other in a loop: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// Variables nest deeper than [`MAX_DEPTH`].
    #[error("variable `{0}` nests more than {MAX_DEPTH} levels deep")]
    TooDeep(String),
    /// Unknown `{{$name}}`.
    #[error("unknown dynamic variable `${0}`")]
    UnknownDynamic(String),
    /// `{{process.env.NAME}}` is not set.
    #[error("process variable `{0}` is not set")]
    MissingProcessEnv(String),
    /// The user gave no value for `{{?Label}}`.
    #[error("no value given for prompt `{0}`")]
    MissingPrompt(String),
    /// A function failed.
    #[error("function `{name}` failed: {message}")]
    Function {
        /// Function name.
        name: String,
        /// Message from the [`Host`].
        message: String,
    },
}

/// Error from [`render`] in [`Mode::Strict`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(transparent)]
pub struct RenderError(#[from] pub Problem);

/// Result of rendering a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered<'a> {
    /// The rendered text. Borrowed when the input had no tags.
    pub text: Cow<'a, str>,
    /// Whether a secret went into the text. If so, the timeline, history and
    /// logs must mask it.
    pub contains_secret: bool,
    /// The values of the secrets that went into the text, to mask them.
    pub secrets: Vec<Secret>,
    /// Problems found; empty in [`Mode::Strict`], since the first one fails.
    pub problems: Vec<Problem>,
}

/// Renders `input` with the given variables.
///
/// A variable's value is itself a template and is rendered recursively, up to
/// [`MAX_DEPTH`] levels. Malformed tags are kept as literal text.
///
/// # Errors
///
/// In [`Mode::Strict`], the first [`Problem`] found.
pub fn render<'a>(
    input: &'a str,
    variables: &ResolvedVariables,
    host: &mut dyn Host,
    mode: Mode,
) -> Result<Rendered<'a>, RenderError> {
    if !input.contains("{{") {
        return Ok(Rendered {
            text: Cow::Borrowed(input),
            contains_secret: false,
            secrets: Vec::new(),
            problems: Vec::new(),
        });
    }
    let mut renderer = Renderer {
        variables,
        host,
        mode,
        contains_secret: false,
        secrets: Vec::new(),
        problems: Vec::new(),
        stack: Vec::new(),
    };
    let mut out = String::with_capacity(input.len());
    renderer.render_into(input, &mut out)?;
    Ok(Rendered {
        text: Cow::Owned(out),
        contains_secret: renderer.contains_secret,
        secrets: renderer.secrets,
        problems: renderer.problems,
    })
}

struct Renderer<'v, 'h> {
    variables: &'v ResolvedVariables,
    host: &'h mut dyn Host,
    mode: Mode,
    contains_secret: bool,
    secrets: Vec<Secret>,
    problems: Vec<Problem>,
    /// Variables being rendered, to detect cycles.
    stack: Vec<String>,
}

impl Renderer<'_, '_> {
    /// Records a problem, or fails in strict mode. The caller leaves the value empty.
    fn problem(&mut self, problem: Problem) -> Result<(), RenderError> {
        match self.mode {
            Mode::Strict => Err(problem.into()),
            Mode::Lenient => {
                self.problems.push(problem);
                Ok(())
            }
        }
    }

    fn render_into(&mut self, input: &str, out: &mut String) -> Result<(), RenderError> {
        for token in Template::parse(input).tokens() {
            match token {
                Token::Text(text) => out.push_str(text),
                Token::Escape => out.push_str("{{"),
                Token::Invalid(invalid) => out.push_str(&invalid.raw),
                Token::Tag(tag) => self.eval_into(tag.expr(), out)?,
            }
        }
        Ok(())
    }

    fn eval_into(&mut self, expr: &Expr, out: &mut String) -> Result<(), RenderError> {
        match expr {
            Expr::Variable(name) => self.variable_into(name, out)?,
            Expr::Dynamic(name) => {
                if !dynamic::generate(name, self.host, out) {
                    self.problem(Problem::UnknownDynamic(name.clone()))?;
                }
            }
            Expr::Prompt(label) => match self.host.prompt(label) {
                Some(value) => out.push_str(&value),
                None => self.problem(Problem::MissingPrompt(label.clone()))?,
            },
            Expr::ProcessEnv(name) => match self.host.process_env(name) {
                Some(value) => out.push_str(&value),
                None => self.problem(Problem::MissingProcessEnv(name.clone()))?,
            },
            Expr::Str(text) => out.push_str(text),
            Expr::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Expr::Null => {}
            Expr::Function { name, args } => {
                let mut rendered = Vec::with_capacity(args.len());
                for (arg, value) in args {
                    rendered.push((arg.clone(), self.arg(value)?));
                }
                match self.host.call(name, &rendered) {
                    Ok(value) => out.push_str(&value),
                    Err(message) => self.problem(Problem::Function {
                        name: name.clone(),
                        message,
                    })?,
                }
            }
        }
        Ok(())
    }

    fn arg(&mut self, expr: &Expr) -> Result<FunctionArg, RenderError> {
        Ok(match expr {
            Expr::Bool(value) => FunctionArg::Bool(*value),
            Expr::Null => FunctionArg::Null,
            other => {
                let mut text = String::new();
                self.eval_into(other, &mut text)?;
                FunctionArg::Text(text)
            }
        })
    }

    /// Renders variable `name`; the value of a secret is kept, so whatever
    /// shows the text later can mask it.
    fn variable_into(&mut self, name: &str, out: &mut String) -> Result<(), RenderError> {
        let secret = self.variables.get(name).is_some_and(|var| var.secret);
        let start = out.len();
        let result = self.value_into(name, out);
        if secret && out.len() > start {
            self.secrets.push(Secret::new(&out[start..]));
        }
        result
    }

    fn value_into(&mut self, name: &str, out: &mut String) -> Result<(), RenderError> {
        let Some(var) = self.variables.get(name) else {
            return self.problem(Problem::MissingVariable(name.to_owned()));
        };
        if var.secret {
            self.contains_secret = true;
        }
        let Some(value) = &var.value else {
            return self.problem(if var.secret {
                Problem::SecretUnavailable(name.to_owned())
            } else {
                Problem::MissingVariable(name.to_owned())
            });
        };
        if !value.data.contains("{{") {
            out.push_str(&value.data);
            return Ok(());
        }
        if self.stack.iter().any(|n| n == name) {
            let mut chain = self.stack.clone();
            chain.push(name.to_owned());
            return self.problem(Problem::Cycle(chain));
        }
        if self.stack.len() >= MAX_DEPTH {
            return self.problem(Problem::TooDeep(name.to_owned()));
        }
        self.stack.push(name.to_owned());
        let result = self.render_into(&value.data, out);
        self.stack.pop();
        result
    }
}
