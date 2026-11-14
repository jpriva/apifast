//! Shared pieces for the UI spike: test data, a JSON + `{{var}}` tokenizer,
//! process metrics and the scripted scenario both prototypes run.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Test data
// ---------------------------------------------------------------------------

pub const TREE_ROWS: usize = 5000;
pub const ROW_HEIGHT: f32 = 22.0;

pub struct TreeRow {
    pub depth: u8,
    pub method: &'static str,
    pub label: String,
}

pub fn tree_rows() -> Vec<TreeRow> {
    const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    (0..TREE_ROWS)
        .map(|i| {
            if i % 25 == 0 {
                TreeRow { depth: 0, method: "", label: format!("folder-{}", i / 25) }
            } else {
                TreeRow { depth: 1, method: METHODS[i % 5], label: format!("request number {i}") }
            }
        })
        .collect()
}

/// Pretty-printed JSON with `{{variables}}` in string values.
/// Size in KB from `SPIKE_JSON_KB` (default 1000, about 1 MB).
pub fn big_json() -> String {
    let target = std::env::var("SPIKE_JSON_KB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1000) * 1000;
    let mut s = String::from("[\n");
    let mut i = 0;
    while s.len() < target {
        let _ = write!(
            s,
            "  {{\n    \"id\": {i},\n    \"name\": \"user {i}\",\n    \"email\": \"{{{{user_email}}}}\",\n    \"active\": true,\n    \"score\": 12.5,\n    \"tags\": [\"a\", \"b\", null]\n  }},\n"
        );
        i += 1;
    }
    s.push_str("  {}\n]\n");
    s
}

// ---------------------------------------------------------------------------
// Tokenizer: JSON + {{var}}
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Key,
    Str,
    Num,
    Keyword,
    Punct,
    Var,
}

/// Tokenizes one line (or any text) into (byte range, kind) spans covering it fully.
pub fn tokenize(line: &str, out: &mut Vec<(std::ops::Range<usize>, Tok)>) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let c = b[i];
        if c == b'{' && b.get(i + 1) == Some(&b'{') {
            let end = line[i..].find("}}").map(|e| i + e + 2).unwrap_or(b.len());
            out.push((start..end, Tok::Var));
            i = end;
        } else if c == b'"' {
            i += 1;
            let mut segment = i - 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                } else if b[i] == b'{' && b.get(i + 1) == Some(&b'{') {
                    if i > segment {
                        out.push((segment..i, Tok::Str));
                    }
                    let end = line[i..].find("}}").map(|e| i + e + 2).unwrap_or(b.len());
                    out.push((i..end, Tok::Var));
                    i = end;
                    segment = i;
                    continue;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            let is_key = line[i..].trim_start().starts_with(':');
            if i > segment {
                out.push((segment..i, if is_key { Tok::Key } else { Tok::Str }));
            }
        } else if c.is_ascii_digit() || c == b'-' {
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'.' | b'-' | b'e' | b'E' | b'+')) {
                i += 1;
            }
            out.push((start..i, Tok::Num));
        } else if c.is_ascii_alphabetic() {
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push((start..i, Tok::Keyword));
        } else if matches!(c, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            i += 1;
            out.push((start..i, Tok::Punct));
        } else {
            while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            if i == start {
                i += line[i..].chars().next().map_or(1, char::len_utf8);
            }
            out.push((start..i, Tok::Plain));
        }
    }
}

/// RGB color per token kind (same palette in both prototypes).
pub fn color(t: Tok) -> [u8; 3] {
    match t {
        Tok::Plain | Tok::Punct => [200, 200, 200],
        Tok::Key => [120, 180, 255],
        Tok::Str => [160, 220, 140],
        Tok::Num => [240, 180, 100],
        Tok::Keyword => [220, 130, 220],
        Tok::Var => [255, 210, 60],
    }
}

// ---------------------------------------------------------------------------
// Process metrics (Linux /proc)
// ---------------------------------------------------------------------------

fn status_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

pub fn rss_kb() -> u64 {
    status_kb("VmRSS:")
}

pub fn hwm_kb() -> u64 {
    status_kb("VmHWM:")
}

/// User + system CPU time of the whole process (all threads).
pub fn cpu_time() -> Duration {
    let ticks = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            let rest = &s[s.rfind(')')? + 2..];
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0);
    Duration::from_millis(ticks * 10) // CLK_TCK = 100 on Linux
}

// ---------------------------------------------------------------------------
// Scripted scenario
// ---------------------------------------------------------------------------

pub const PHASE_FRAMES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Nothing to do this frame, keep requesting frames.
    Wait,
    /// Insert one character in the middle of the editor text.
    Type,
    /// Scroll the tree by this many pixels.
    ScrollTree(f32),
    /// Scroll the editor by this many pixels.
    ScrollEditor(f32),
    /// Stop requesting frames (idle measurement).
    Idle,
    /// Write the report and exit.
    Exit,
}

#[derive(Default)]
struct Phase {
    intervals: Vec<f64>,
    cpu_start: Duration,
    cpu_end: Duration,
}

impl Phase {
    fn summary(&self, name: &str) -> String {
        let mut v = self.intervals.clone();
        v.sort_by(f64::total_cmp);
        let p = |q: f64| v.get(((v.len() as f64 * q) as usize).min(v.len().saturating_sub(1))).copied().unwrap_or(0.0);
        let cpu = (self.cpu_end.saturating_sub(self.cpu_start)).as_secs_f64() * 1e3 / v.len().max(1) as f64;
        format!(
            "\"{name}\":{{\"frames\":{},\"p50_ms\":{:.2},\"p95_ms\":{:.2},\"max_ms\":{:.2},\"cpu_ms_per_frame\":{:.2}}}",
            v.len(),
            p(0.5),
            p(0.95),
            v.last().copied().unwrap_or(0.0),
            cpu
        )
    }
}

/// Drives the same timeline in both toolkits. Call `next()` once per frame.
pub struct Scenario {
    pub toolkit: &'static str,
    pub backend: String,
    pub mode: String,
    started: Instant,
    first_frame: Option<Duration>,
    step: usize,
    last_frame: Option<Instant>,
    rss_loaded: u64,
    phases: [Phase; 3],
    idle_from: Option<(Instant, Duration)>,
    idle_cpu_pct: f64,
    idle_frames: u64,
    pub enabled: bool,
    reported: bool,
}

const SETTLE: Duration = Duration::from_millis(1500);
const IDLE: Duration = Duration::from_secs(5);

impl Scenario {
    pub fn new(toolkit: &'static str, started: Instant) -> Self {
        Self {
            toolkit,
            backend: std::env::var("SPIKE_BACKEND").unwrap_or_else(|_| "default".into()),
            mode: std::env::var("SPIKE_MODE").unwrap_or_else(|_| "full".into()),
            started,
            first_frame: None,
            step: 0,
            last_frame: None,
            rss_loaded: 0,
            phases: Default::default(),
            idle_from: None,
            idle_cpu_pct: 0.0,
            idle_frames: 0,
            enabled: std::env::var("SPIKE_SCENARIO").is_ok(),
            reported: false,
        }
    }

    pub fn is_empty_mode(&self) -> bool {
        self.mode == "empty"
    }

    /// Called at the start of each rendered frame. Returns what to do.
    pub fn next(&mut self) -> Action {
        let now = Instant::now();
        if self.first_frame.is_none() {
            self.first_frame = Some(now - self.started);
        }
        if !self.enabled {
            return Action::Idle;
        }
        // Idle phase: count frames that still happen while we asked for none.
        if let Some((t, cpu)) = self.idle_from {
            self.idle_frames += 1;
            if now - t >= IDLE {
                self.idle_cpu_pct = (cpu_time().saturating_sub(cpu)).as_secs_f64() / (now - t).as_secs_f64() * 100.0;
                self.write_report();
                return Action::Exit;
            }
            return Action::Idle;
        }
        // Settle after startup before measuring.
        if now - self.started < SETTLE && self.step == 0 {
            return Action::Wait;
        }
        if self.step == 0 {
            self.rss_loaded = rss_kb();
        }
        let phase_idx = self.step / (PHASE_FRAMES + 1);
        let in_phase = self.step % (PHASE_FRAMES + 1);
        self.step += 1;
        if self.is_empty_mode() || phase_idx >= 3 {
            self.idle_from = Some((now, cpu_time()));
            return Action::Idle;
        }
        let ph = &mut self.phases[phase_idx];
        if in_phase == 0 {
            ph.cpu_start = cpu_time();
            self.last_frame = Some(now);
        } else {
            if let Some(l) = self.last_frame {
                ph.intervals.push((now - l).as_secs_f64() * 1e3);
            }
            self.last_frame = Some(now);
            ph.cpu_end = cpu_time();
        }
        match phase_idx {
            0 => Action::Type,
            1 => Action::ScrollTree(40.0),
            _ => Action::ScrollEditor(40.0),
        }
    }

    /// Wakes up the idle timer when the toolkit does not render while idle.
    pub fn idle_deadline_passed(&self) -> bool {
        self.idle_from.is_some_and(|(t, _)| t.elapsed() >= IDLE)
    }

    fn write_report(&mut self) {
        if self.reported {
            return;
        }
        self.reported = true;
        let line = format!(
            "{{\"toolkit\":\"{}\",\"backend\":\"{}\",\"mode\":\"{}\",\"json_kb\":{},\"first_frame_ms\":{:.0},\"rss_loaded_kb\":{},\"rss_end_kb\":{},\"hwm_kb\":{},\"idle_cpu_pct\":{:.2},\"idle_frames\":{},{},{},{}}}\n",
            self.toolkit,
            self.backend,
            self.mode,
            std::env::var("SPIKE_JSON_KB").unwrap_or_else(|_| "1000".into()),
            self.first_frame.unwrap_or_default().as_secs_f64() * 1e3,
            self.rss_loaded,
            rss_kb(),
            hwm_kb(),
            self.idle_cpu_pct,
            self.idle_frames,
            self.phases[0].summary("typing"),
            self.phases[1].summary("tree_scroll"),
            self.phases[2].summary("editor_scroll"),
        );
        if let Ok(path) = std::env::var("SPIKE_OUT") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
        print!("{line}");
    }

    /// Final report when idle ends without a frame (reactive toolkits).
    pub fn finish_idle(&mut self) {
        if let Some((t, cpu)) = self.idle_from {
            self.idle_cpu_pct = (cpu_time().saturating_sub(cpu)).as_secs_f64() / t.elapsed().as_secs_f64() * 100.0;
        }
        self.write_report();
    }
}
