//! Clock, randomness and the file manager, from the operating system. The
//! CLI has its own copy of the clock and randomness: drivers cannot depend
//! on each other.

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

/// Opens `folder` in the system's file manager, without waiting for it.
///
/// # Errors
///
/// If the file manager cannot be started.
pub fn open_folder(folder: &std::path::Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}
