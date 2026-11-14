//! Clock, randomness and prompts from the operating system.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use apifast_app::{Clock, Prompter, Random};

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// Random numbers for dynamic variables such as `{{$randomUUID}}`. Not for
/// cryptography: they come from the standard library's randomly seeded
/// hasher, which is enough for test data.
pub struct SystemRandom {
    state: RandomState,
    counter: AtomicU64,
}

impl SystemRandom {
    pub fn new() -> Self {
        Self {
            state: RandomState::new(),
            counter: AtomicU64::new(0),
        }
    }
}

impl Random for SystemRandom {
    fn next_u64(&self) -> u64 {
        let mut hasher = self.state.build_hasher();
        hasher.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        hasher.finish()
    }
}

/// Asks `{{?Label}}` values on the terminal. Without a terminal (in a pipe
/// or in CI) it gives no value, so the prompt counts as missing.
pub struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn prompt(&self, label: &str) -> Option<String> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return None;
        }
        let mut stderr = std::io::stderr();
        write!(stderr, "{label}: ").ok()?;
        stderr.flush().ok()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line).ok()?;
        let value = line.trim_end_matches(['\r', '\n']);
        (!value.is_empty()).then(|| value.to_owned())
    }
}
