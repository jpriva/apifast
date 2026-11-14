use std::collections::BTreeMap;

use apifast_template::Host;

use crate::{Clock, Prompter, Random};

/// The template [`Host`] the use cases pass to the renderer, built from the ports.
pub struct AppHost<'a> {
    clock: &'a dyn Clock,
    random: &'a dyn Random,
    prompter: &'a dyn Prompter,
    process_env: BTreeMap<String, String>,
    /// Answers already given, so a prompt used twice is asked once.
    answers: BTreeMap<String, Option<String>>,
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
        }
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

    fn prompt(&mut self, label: &str) -> Option<String> {
        if let Some(answer) = self.answers.get(label) {
            return answer.clone();
        }
        let answer = self.prompter.prompt(label);
        self.answers.insert(label.to_owned(), answer.clone());
        answer
    }
}
