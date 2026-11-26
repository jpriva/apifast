//! Budgets of docs/10 that need the real window: what a 100 MB response
//! costs in memory, and the CPU of a keystroke in a 200 KB body. Linux only
//! (it reads `/proc`). See tools/bench/README.md.
//!
//! ```sh
//! cargo run --release -p apifast-gui --example bench -- response
//! cargo run --release -p apifast-gui --example bench -- typing
//! cargo run --release -p apifast-gui --example bench -- send
//! cargo run --release -p apifast-gui --example bench -- viewports
//! ```
//!
//! `response` opens the real application on a collection with one request
//! to a local server that answers 100 MB, sends it and shows it, and reports
//! apifast's own memory (PSS without the system's graphics stack) before,
//! while the response shows, and after closing its tab. `typing` types into
//! the code editor of a request body holding 200 KB of JSON, one key per
//! frame, and reports the process's CPU per keystroke. `send`, without a
//! window, sends a request to a local server that answers one line, again
//! and again through the application's core, and reports the time from the
//! command to the response shown: the overhead of a send, without network.
//! `viewports` opens one and then two more windows of the system next to the
//! main one, each drawing a panel, and reports apifast's own memory with
//! each: the cost of taking a panel out to its own window.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use apifast_app::RequestPath;
use apifast_gui::app::App;
use apifast_gui::atoms::{CodeEditor, TemplateFieldTexts, VariableHints};
use apifast_gui::code::Language;
use apifast_gui::core::{Command, Core, ResponseState, TabKey};
use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::Backends;

/// The size of the response of `response`.
const RESPONSE_BYTES: usize = 100 << 20;
/// How many times `response` opens, sends, shows and closes it, unless
/// `APIFAST_BENCH_ROUNDS` says.
fn rounds() -> u32 {
    std::env::var("APIFAST_BENCH_ROUNDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(3)
}
/// The size of the body of `typing`.
const BODY_BYTES: usize = 200 << 10;
/// Keystrokes measured, after the ones that warm up.
const KEYSTROKES: u32 = 300;
const WARMUP: u32 = 30;

fn main() -> eframe::Result {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    // `response` also runs with the window out of sight: with vsync, a hidden
    // window on Wayland never gets its frame presented, and the scenario,
    // which moves on in each frame, would stop.
    let options = |present_mode| {
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = Backends::PRIMARY;
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
            wgpu_options: {
                let mut wgpu = WgpuConfiguration {
                    wgpu_setup: WgpuSetup::CreateNew(setup),
                    ..WgpuConfiguration::default()
                };
                wgpu.surface.present_mode = present_mode;
                wgpu
            },
            ..Default::default()
        }
    };
    match scenario.as_str() {
        "response" => eframe::run_native(
            "apifast bench",
            options(eframe::wgpu::PresentMode::AutoNoVsync),
            Box::new(|cc| Ok(Box::new(ResponseBench::new(&cc.egui_ctx)))),
        ),
        "typing" => eframe::run_native(
            "apifast bench",
            options(eframe::wgpu::PresentMode::AutoVsync),
            Box::new(|cc| {
                apifast_gui::theme::install(&cc.egui_ctx);
                Ok(Box::new(TypingBench::new()))
            }),
        ),
        "send" => {
            send_bench();
            Ok(())
        }
        "viewports" => eframe::run_native(
            "apifast bench",
            options(eframe::wgpu::PresentMode::AutoVsync),
            Box::new(|cc| {
                apifast_gui::theme::install(&cc.egui_ctx);
                Ok(Box::new(ViewportBench::new()))
            }),
        ),
        _ => {
            say("usage: bench response|typing|send|viewports");
            Ok(())
        }
    }
}

/// Writes a line of the results.
fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// The process's PSS, in KiB: all of it, and apifast's own (without the
/// system's graphics stack, as tools/bench/breakdown.py splits it).
fn pss() -> (u64, u64) {
    let graphics = |name: &str| {
        [
            "libLLVM",
            "libvulkan",
            "libSPIRV",
            "libgallium",
            "libEGL",
            "libGL",
            "libgbm",
            "libdrm",
            "libglapi",
            "libdisplay-info",
            "libelf",
            "libsensors",
            "libexpat",
            "libz3",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
            || name.contains("_dri.so")
            || name.contains("_icd")
    };
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let (mut total, mut drivers, mut name) = (0, 0, String::new());
    for line in smaps.lines() {
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or_default();
        if first.contains('-') && !first.ends_with(':') {
            let path = parts.nth(4).unwrap_or("[anon]");
            name = Path::new(path)
                .file_name()
                .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        } else if first == "Pss:" {
            let kb: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            total += kb;
            if graphics(&name) {
                drivers += kb;
            }
        }
    }
    (total, total - drivers)
}

/// The heap and the anonymous memory of the process, in KiB: where its own
/// allocations live (small ones on the heap, large ones in anonymous maps).
fn heap_and_anon() -> (u64, u64) {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let (mut heap, mut anon, mut kind) = (0, 0, "");
    for line in smaps.lines() {
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or_default();
        if first.contains('-') && !first.ends_with(':') {
            kind = match parts.nth(4) {
                Some("[heap]") => "heap",
                None => "anon",
                Some(_) => "",
            };
        } else if first == "Pss:" {
            let kb: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            match kind {
                "heap" => heap += kb,
                "anon" => anon += kb,
                _ => {}
            }
        }
    }
    (heap, anon)
}

/// A line of the memory now, against `before` (apifast's own).
fn memory_line(what: &str, before: u64) -> String {
    let (total, own) = pss();
    let (heap, anon) = heap_and_anon();
    format!(
        "{what}: own {} (+{}), heap {}, anon {}, total {}",
        mb(own),
        mb(own.saturating_sub(before)),
        mb(heap),
        mb(anon),
        mb(total)
    )
}

fn mb(kb: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a memory size to print")]
    let mb = kb as f64 / 1024.0;
    format!("{mb:.1} MB")
}

/// The CPU time this process used, in milliseconds.
fn cpu_ms() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // After the command name, which ends with `)`: utime and stime are the
    // 12th and 13th fields.
    let rest = stat.rsplit(')').next().unwrap_or_default();
    let fields: Vec<u64> = rest
        .split_whitespace()
        .skip(11)
        .take(2)
        .filter_map(|f| f.parse().ok())
        .collect();
    // Clock ticks are 100 a second on Linux.
    fields.iter().sum::<u64>() * 10
}

/// A local server that answers every request with `RESPONSE_BYTES` of text.
fn server() -> u16 {
    server_of(RESPONSE_BYTES)
}

/// A local server that answers every request with `bytes` of text, on a
/// connection of its own.
fn server_of(bytes: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("bind: {e}"));
    let port = listener.local_addr().map_or(0, |a| a.port());
    std::thread::spawn(move || {
        let line = "{\"id\": 1234567, \"name\": \"apifast\", \"ok\": true}\n".repeat(1024);
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut header = String::new();
            while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                header.clear();
            }
            let mut stream = &stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {bytes}\r\nConnection: close\r\n\r\n"
            );
            let mut sent = 0;
            while sent < bytes {
                let chunk = &line.as_bytes()[..line.len().min(bytes - sent)];
                if stream.write_all(chunk).is_err() {
                    break;
                }
                sent += chunk.len();
            }
        }
    });
    port
}

/// A collection with one request, `big.yml`, to the local server.
fn collection(port: u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("apifast-bench-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Bench\n",
    );
    let _ = std::fs::write(
        dir.join("big.yml"),
        format!(
            "info:\n  name: Big\n  type: http\nhttp:\n  method: GET\n  url: http://127.0.0.1:{port}/big\n"
        ),
    );
    dir
}

/// Sends `SENDS` times through the core and reports the median and the
/// slowest of the time to each response.
fn send_bench() {
    const SENDS: usize = 200;
    let (notify, notified) = std::sync::mpsc::channel::<()>();
    let data = std::env::temp_dir().join(format!("apifast-bench-send-{}", std::process::id()));
    let core = Core::start_with(data, apifast_vault::SystemKeyring::off(), move || {
        let _ = notify.send(());
    });
    let wait = |done: &dyn Fn(&apifast_gui::core::Snapshot) -> bool| loop {
        if done(&core.snapshot()) {
            return;
        }
        let _ = notified.recv_timeout(Duration::from_millis(50));
    };
    core.send(Command::OpenCollection(collection(server_of(64))));
    wait(&|s| s.collection.is_some());
    let root = core
        .snapshot()
        .collection
        .as_ref()
        .map(|c| c.root.clone())
        .unwrap_or_default();
    core.send(Command::OpenRequest(TabKey::new(
        root,
        RequestPath::new("big.yml"),
    )));
    wait(&|s| s.active_tab().is_some());
    let received = |s: &apifast_gui::core::Snapshot| {
        matches!(
            s.active_tab().map(|t| &t.response),
            Some(ResponseState::Received(_))
        )
    };
    let mut times = Vec::with_capacity(SENDS);
    for _ in 0..SENDS {
        let version = core.snapshot().version;
        let started = Instant::now();
        core.send(Command::Send);
        wait(&|s| {
            s.version > version
                && received(s)
                && !s.active_tab().is_some_and(|t| t.response.is_sending())
        });
        times.push(started.elapsed());
    }
    times.sort();
    // The first ones warm up connections and caches.
    let times = &times[SENDS / 10..];
    say(&format!(
        "send to a local server: median {:.2} ms, p95 {:.2} ms, slowest {:.2} ms ({} sends)",
        times[times.len() / 2].as_secs_f64() * 1000.0,
        times[times.len() * 95 / 100].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0,
        times.len(),
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Idle(Instant),
    Sending(Instant),
    Showing(Instant),
    Closed(Instant),
    Done,
}

struct ResponseBench {
    app: App,
    tab: Option<TabKey>,
    phase: Phase,
    before: u64,
    round: u32,
}

impl ResponseBench {
    fn new(ctx: &egui::Context) -> Self {
        apifast_gui::theme::install(ctx);
        let data = std::env::temp_dir().join(format!("apifast-bench-data-{}", std::process::id()));
        let repaint = ctx.clone();
        let core = Core::start_with(data, apifast_vault::SystemKeyring::off(), move || {
            repaint.request_repaint();
        });
        let app = App::new(core);
        app.core()
            .send(Command::OpenCollection(collection(server())));
        Self {
            app,
            tab: None,
            phase: Phase::Opening,
            before: 0,
            round: 1,
        }
    }

    fn step(&mut self, ctx: &egui::Context) {
        let snapshot = self.app.core().snapshot();
        let wait = Duration::from_secs(5);
        self.phase = match self.phase {
            Phase::Opening => match &snapshot.collection {
                Some(collection) => {
                    let tab = TabKey::new(collection.root.clone(), RequestPath::new("big.yml"));
                    self.app.core().send(Command::OpenRequest(tab.clone()));
                    self.tab = Some(tab);
                    Phase::Idle(Instant::now())
                }
                None => Phase::Opening,
            },
            Phase::Idle(since) if since.elapsed() > wait => {
                if self.round == 1 {
                    self.before = pss().1;
                }
                say(&memory_line(
                    &format!("round {} before", self.round),
                    self.before,
                ));
                self.app.core().send(Command::Send);
                Phase::Sending(Instant::now())
            }
            Phase::Sending(started) => {
                let tab = snapshot.active_tab();
                match tab.map(|tab| &tab.response) {
                    Some(ResponseState::Received(_)) => {
                        say(&format!(
                            "received {} MB in {:?}",
                            RESPONSE_BYTES >> 20,
                            started.elapsed()
                        ));
                        Phase::Showing(Instant::now())
                    }
                    Some(ResponseState::Failed(error)) => {
                        say(&format!("failed: {error}"));
                        Phase::Done
                    }
                    _ => Phase::Sending(started),
                }
            }
            Phase::Showing(since) if since.elapsed() > wait => {
                say(&memory_line(
                    &format!("round {} showing", self.round),
                    self.before,
                ));
                if let Some(tab) = self.tab.clone() {
                    self.app.core().send(Command::CloseTab(tab));
                }
                Phase::Closed(Instant::now())
            }
            Phase::Closed(since) if since.elapsed() > wait => {
                say(&memory_line(
                    &format!("round {} closed", self.round),
                    self.before,
                ));
                if self.round >= rounds() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    Phase::Done
                } else {
                    // Again: the same request, in a new tab.
                    self.round += 1;
                    if let Some(tab) = self.tab.clone() {
                        self.app.core().send(Command::OpenRequest(tab));
                    }
                    Phase::Idle(Instant::now())
                }
            }
            phase => phase,
        };
        // Waiting phases look again shortly, without input.
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl eframe::App for ResponseBench {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.step(&ctx);
        self.app.show(ui);
    }
}

struct TypingBench {
    code: String,
    /// The body's size before typing, to say how much was typed.
    initial: usize,
    hints: VariableHints,
    frames: u32,
    started: Option<(Instant, u64)>,
}

impl TypingBench {
    fn new() -> Self {
        let item =
            "  {\"id\": 1234567, \"name\": \"apifast\", \"tags\": [\"a\", \"b\"], \"ok\": true},\n";
        let mut code = String::from("[\n");
        while code.len() < BODY_BYTES {
            code.push_str(item);
        }
        code.push_str("  {}\n]\n");
        Self {
            initial: code.len(),
            code,
            hints: VariableHints::default(),
            frames: 0,
            started: None,
        }
    }
}

impl eframe::App for TypingBench {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // One key a frame, once the editor has focus.
        if self.frames > 2 {
            raw_input.events.push(egui::Event::Text("a".into()));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let id = egui::Id::unique("bench-body");
        let texts = TemplateFieldTexts {
            placeholder: "",
            missing: "",
            generated: "",
        };
        egui::CentralPanel::default().show(ui, |ui| {
            CodeEditor::new(&mut self.code, Language::Json, &self.hints, texts)
                .id(id)
                .desired_rows(40)
                .show(ui);
        });
        ui.ctx().memory_mut(|memory| memory.request_focus(id));
        self.frames += 1;
        let typed = self.frames.saturating_sub(3);
        if typed == WARMUP {
            self.started = Some((Instant::now(), cpu_ms()));
        }
        if typed == WARMUP + KEYSTROKES
            && let Some((started, cpu)) = self.started
        {
            let used = cpu_ms().saturating_sub(cpu);
            say(&format!(
                "typing in {} KB: {} keystrokes ({} characters typed), CPU {:.2} ms per keystroke, {:.2} ms of wall time each",
                self.initial >> 10,
                KEYSTROKES,
                self.code.len() - self.initial,
                f64::from(u32::try_from(used).unwrap_or(u32::MAX)) / f64::from(KEYSTROKES),
                started.elapsed().as_secs_f64() * 1000.0 / f64::from(KEYSTROKES),
            ));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

/// How long each step of `viewports` waits before measuring: the windows
/// open, draw and settle.
const VIEWPORT_SETTLE: Duration = Duration::from_secs(6);

/// Opens extra windows one by one and measures the memory with each.
struct ViewportBench {
    /// Extra windows open now.
    open: usize,
    /// When the current step started.
    since: Instant,
    /// apifast's own memory with the main window only, in KiB.
    before: Option<u64>,
    /// What each window shows: some text, as a panel would.
    text: String,
    /// Frames drawn and CPU used (ms) since the step started.
    frames: u32,
    cpu: u64,
}

impl ViewportBench {
    fn new() -> Self {
        Self {
            open: 0,
            since: Instant::now(),
            before: None,
            frames: 0,
            cpu: cpu_ms(),
            text: "{\n  \"name\": \"Ana\",\n  \"roles\": [\"admin\"]\n}\n".repeat(20),
        }
    }
}

impl eframe::App for ViewportBench {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.label(format!("main window; {} more open", self.open));
        ui.add(egui::TextEdit::multiline(&mut self.text).code_editor());
        for n in 0..self.open {
            let id = egui::ViewportId::from_hash_of(("bench-viewport", n));
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("panel {n}"))
                .with_inner_size([640.0, 480.0]);
            let text = &mut self.text;
            ctx.show_viewport_immediate(id, builder, |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label(format!("panel {n}"));
                    ui.add(egui::TextEdit::multiline(text).code_editor());
                });
            });
        }
        ctx.request_repaint();
        self.frames += 1;
        if self.since.elapsed() < VIEWPORT_SETTLE {
            return;
        }
        let own = pss().1;
        let before = *self.before.get_or_insert(own);
        let per_frame = (cpu_ms() - self.cpu) * 1000 / u64::from(self.frames.max(1));
        say(&format!(
            "{}, CPU {per_frame} µs per frame ({} frames)",
            memory_line(&format!("{} extra window(s)", self.open), before),
            self.frames
        ));
        self.since = Instant::now();
        self.frames = 0;
        self.cpu = cpu_ms();
        if self.open == 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.open += 1;
        }
    }
}
