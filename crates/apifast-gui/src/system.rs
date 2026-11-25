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

/// The operating system, for a bug report: its name and version (from
/// `/etc/os-release` on Linux), the processor architecture, and on Linux the
/// desktop and its session type.
pub fn description() -> String {
    let arch = std::env::consts::ARCH;
    let name = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_name(&text))
        .unwrap_or_else(|| std::env::consts::OS.to_owned());
    let desktop = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE"]
        .iter()
        .filter_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .collect::<Vec<_>>();
    if desktop.is_empty() {
        format!("{name} ({arch})")
    } else {
        format!("{name} ({arch}), {}", desktop.join(" on "))
    }
}

/// `PRETTY_NAME` of an `os-release` file, without its quotes.
pub fn os_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.strip_prefix("PRETTY_NAME=")?;
        Some(value.trim().trim_matches('"').to_owned())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_name_comes_from_os_release() {
        let text = "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 45 (Workstation Edition)\"\n";
        assert_eq!(
            super::os_name(text).as_deref(),
            Some("Fedora Linux 45 (Workstation Edition)")
        );
        assert_eq!(super::os_name("NAME=x"), None);
        assert_ne!(super::description(), "");
    }
}
