//! The `console` global: what a script writes is kept, up to a size.

use std::sync::{Arc, Mutex, PoisonError};

use apifast_app::{ConsoleLevel, ConsoleLine};
use rquickjs::{Ctx, Function};

/// The most text a script's console keeps; past it, a last line says the
/// rest was dropped.
const MAX_BYTES: usize = 1 << 20;

/// Builds `console` from the native `write(level, text)`, which is then
/// removed from the globals. Strings are written as they are, other values
/// as JSON when they can be.
const PRELUDE: &str = r"
(() => {
    const write = globalThis.__apifast_console;
    delete globalThis.__apifast_console;
    const show = (value) => {
        if (typeof value === 'string') return value;
        if (value instanceof Error) return `${value.name}: ${value.message}`;
        if (typeof value === 'function' || typeof value === 'symbol') return String(value);
        try {
            const json = JSON.stringify(value);
            return json === undefined ? String(value) : json;
        } catch {
            return String(value);
        }
    };
    const method = (level) => (...values) => write(level, values.map(show).join(' '));
    globalThis.console = Object.freeze({
        log: method('log'),
        info: method('info'),
        warn: method('warn'),
        error: method('error'),
        debug: method('debug'),
    });
})();
";

/// What a script wrote so far.
#[derive(Debug, Default)]
pub(crate) struct Console {
    lines: Vec<ConsoleLine>,
    bytes: usize,
    full: bool,
}

impl Console {
    fn write(&mut self, level: ConsoleLevel, text: String) {
        if self.full {
            return;
        }
        if self.bytes + text.len() > MAX_BYTES {
            self.full = true;
            self.lines.push(ConsoleLine {
                level: ConsoleLevel::Warn,
                text: format!("(console output past {} MB dropped)", MAX_BYTES >> 20),
            });
            return;
        }
        self.bytes += text.len();
        self.lines.push(ConsoleLine { level, text });
    }
}

fn level(name: &str) -> ConsoleLevel {
    match name {
        "info" => ConsoleLevel::Info,
        "warn" => ConsoleLevel::Warn,
        "error" => ConsoleLevel::Error,
        "debug" => ConsoleLevel::Debug,
        _ => ConsoleLevel::Log,
    }
}

/// Gives the script of `ctx` a `console` that writes to `console`.
pub(crate) fn install(ctx: &Ctx<'_>, console: Arc<Mutex<Console>>) -> rquickjs::Result<()> {
    let write = Function::new(ctx.clone(), move |name: String, text: String| {
        let mut console = console.lock().unwrap_or_else(PoisonError::into_inner);
        console.write(level(&name), text);
    })?;
    ctx.globals().set("__apifast_console", write)?;
    ctx.eval::<(), _>(PRELUDE)
}

/// The lines written to `console`.
pub(crate) fn take(console: &Mutex<Console>) -> Vec<ConsoleLine> {
    let mut console = console.lock().unwrap_or_else(PoisonError::into_inner);
    std::mem::take(&mut console.lines)
}
