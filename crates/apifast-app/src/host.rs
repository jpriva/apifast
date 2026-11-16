use std::collections::BTreeMap;

use apifast_template::{FunctionArg, Host};

use crate::functions::ResponseFunctions;
use crate::{Clock, Prompter, Random};

/// The template [`Host`] the use cases pass to the renderer, built from the ports.
pub struct AppHost<'a> {
    clock: &'a dyn Clock,
    random: &'a dyn Random,
    prompter: &'a dyn Prompter,
    process_env: BTreeMap<String, String>,
    /// Answers already given, so a prompt used twice is asked once.
    answers: BTreeMap<String, Option<String>>,
    /// `response` and `header`, when the host can reach other requests.
    functions: Option<ResponseFunctions<'a>>,
}

impl<'a> AppHost<'a> {
    /// Creates a host.
    pub fn new(
        clock: &'a dyn Clock,
        random: &'a dyn Random,
        prompter: &'a dyn Prompter,
        process_env: BTreeMap<String, String>,
    ) -> Self {
        Self {
            clock,
            random,
            prompter,
            process_env,
            answers: BTreeMap::new(),
            functions: None,
        }
    }

    /// With `response` and `header`.
    pub(crate) fn with_functions(mut self, functions: ResponseFunctions<'a>) -> Self {
        self.functions = Some(functions);
        self
    }

    /// The `response` and `header` functions, if it has them.
    pub(crate) fn functions(&mut self) -> Option<&mut ResponseFunctions<'a>> {
        self.functions.as_mut()
    }
}

impl Host for AppHost<'_> {
    fn unix_millis(&mut self) -> i64 {
        self.clock.unix_millis()
    }

    fn random_u64(&mut self) -> u64 {
        self.random.next_u64()
    }

    fn process_env(&self, name: &str) -> Option<String> {
        self.process_env.get(name).cloned()
    }

    fn call(&mut self, name: &str, args: &[(String, FunctionArg)]) -> Result<String, String> {
        match (name, &mut self.functions) {
            ("response" | "header", Some(functions)) => functions.call(name, args),
            ("response" | "header", None) => {
                Err(format!("`{name}` cannot reach other requests here"))
            }
            _ => Err(format!("unknown function `{name}`")),
        }
    }

    fn prompt(&mut self, label: &str) -> Option<String> {
        if let Some(answer) = self.answers.get(label) {
            return answer.clone();
        }
        let answer = self.prompter.prompt(label);
        self.answers.insert(label.to_owned(), answer.clone());
        answer
    }
}
