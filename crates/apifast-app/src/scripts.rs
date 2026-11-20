//! JavaScript scripts (docs/08, F9): the port of the engine that runs them.
//!
//! A script runs alone, with limits on its time and memory, and without
//! access to the file system or the network: what it can read and change
//! is only what the engine is given. What it writes to the console is kept,
//! also when it fails.

use std::future::Future;

use crate::CancelToken;

/// How long a script may run and how much memory it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Wall-clock time, waits included, in milliseconds.
    pub time_ms: u64,
    /// Memory of the script's heap, in bytes.
    pub memory_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            time_ms: 10_000,
            memory_bytes: 64 << 20,
        }
    }
}

/// A script to run.
#[derive(Debug, Clone, Copy)]
pub struct Script<'s> {
    /// Where it comes from, for messages: `collection`, `users/folder.yml`.
    pub name: &'s str,
    /// Its source. It runs as the body of an `async` function, as in
    /// Bruno: it may `await`, and `return` ends it.
    pub code: &'s str,
}

/// The console method a line was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// `console.log`.
    Log,
    /// `console.info`.
    Info,
    /// `console.warn`.
    Warn,
    /// `console.error`.
    Error,
    /// `console.debug`.
    Debug,
}

/// A line a script wrote to the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// How it was written.
    pub level: ConsoleLevel,
    /// Its arguments, joined by spaces: strings as they are, other values
    /// as JSON.
    pub text: String,
}

/// Why a script did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    /// It does not parse.
    #[error("syntax error{}: {message}", at(*line))]
    Syntax {
        /// The parser's message.
        message: String,
        /// The line of the script, from 1, when known.
        line: Option<u32>,
    },
    /// It threw an exception, or a promise it awaited was rejected.
    #[error("{message}{}", at(*line))]
    Thrown {
        /// The error's name and message (`TypeError: …`), or the value
        /// thrown.
        message: String,
        /// The line of the script, from 1, when known.
        line: Option<u32>,
    },
    /// It ran longer than its limit.
    #[error("the script ran longer than {ms} ms")]
    TimedOut {
        /// The limit.
        ms: u64,
    },
    /// It used more memory than its limit.
    #[error("the script used more than {} MB of memory", bytes >> 20)]
    OutOfMemory {
        /// The limit.
        bytes: usize,
    },
    /// It was cancelled.
    #[error("the script was cancelled")]
    Cancelled,
    /// The engine could not run it.
    #[error("the script engine failed: {0}")]
    Engine(String),
}

fn at(line: Option<u32>) -> String {
    line.map(|line| format!(" at line {line}"))
        .unwrap_or_default()
}

/// What running a script gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptOutcome {
    /// What it wrote to the console, in order.
    pub console: Vec<ConsoleLine>,
    /// Why it did not finish; `None` if it did.
    pub error: Option<ScriptError>,
}

/// Runs JavaScript scripts.
pub trait ScriptEngine {
    /// Runs `script` until it ends, fails, exceeds its limits or `cancel`
    /// asks it to stop.
    fn run(
        &self,
        script: Script<'_>,
        cancel: &CancelToken,
    ) -> impl Future<Output = ScriptOutcome> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_where_they_happened() {
        let thrown = ScriptError::Thrown {
            message: "Error: boom".into(),
            line: Some(3),
        };
        assert_eq!(thrown.to_string(), "Error: boom at line 3");
        let syntax = ScriptError::Syntax {
            message: "unexpected token".into(),
            line: None,
        };
        assert_eq!(syntax.to_string(), "syntax error: unexpected token");
        let memory = ScriptError::OutOfMemory { bytes: 64 << 20 };
        assert_eq!(
            memory.to_string(),
            "the script used more than 64 MB of memory"
        );
    }
}
