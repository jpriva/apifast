//! Clock and randomness from the operating system. The CLI has its own copy:
//! drivers cannot depend on each other.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use apifast_app::{Clock, Random};

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// Random numbers for dynamic variables. Not for cryptography: enough for
/// test data.
#[derive(Debug, Default)]
pub struct SystemRandom {
    state: RandomState,
    counter: AtomicU64,
}

impl Random for SystemRandom {
    fn next_u64(&self) -> u64 {
        let mut hasher = self.state.build_hasher();
        hasher.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        hasher.finish()
    }
}
