//! JavaScript engine of apifast (docs/08, F9), on `QuickJS` through rquickjs.
//!
//! Each script runs in a runtime of its own, made when it starts and
//! dropped when it ends: nothing is kept between scripts, and nothing of
//! the engine exists until a script runs. The runtime has no modules, so a
//! script reaches neither the file system nor the network; it only has the
//! language and the globals given here (`console`).
//!
//! Its limits: an interrupt handler stops code that keeps running past the
//! time limit (or once cancelled), a deadline stops one that keeps waiting,
//! and `QuickJS`'s own memory limit bounds its heap.

mod console;
mod errors;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use apifast_app::{CancelToken, Script, ScriptEngine, ScriptError, ScriptLimits, ScriptOutcome};
use rquickjs::context::EvalOptions;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Promise, Value};

use crate::console::Console;

/// The file name scripts are evaluated as, which their errors' stacks name.
const FILE: &str = "script";

/// How deep a script's calls may go, in bytes of the native stack. Far
/// below the stack of a tokio thread, so deep recursion is a script error.
const STACK_BYTES: usize = 256 << 10;

/// How often a waiting script checks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// Runs scripts with `QuickJS`, each within `limits`.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickJs {
    limits: ScriptLimits,
}

impl QuickJs {
    /// An engine that runs each script within `limits`.
    pub fn new(limits: ScriptLimits) -> Self {
        Self { limits }
    }
}

impl ScriptEngine for QuickJs {
    fn run(
        &self,
        script: Script<'_>,
        cancel: &CancelToken,
    ) -> impl Future<Output = ScriptOutcome> + Send {
        let (limits, cancel) = (self.limits, cancel.clone());
        let code = script.code.to_owned();
        async move {
            let console = Arc::new(Mutex::new(Console::default()));
            let error = run(&code, limits, &cancel, &console).await.err();
            let console = console::take(&console);
            ScriptOutcome { console, error }
        }
    }
}

/// Why a run stopped early, as the interrupt handler and the deadline see
/// it: errors raised then are this, whatever `QuickJS` says.
#[derive(Clone)]
struct Stop {
    cancel: CancelToken,
    deadline: Instant,
    limits: ScriptLimits,
}

impl Stop {
    fn reason(&self) -> Option<ScriptError> {
        if self.cancel.is_cancelled() {
            Some(ScriptError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(ScriptError::TimedOut {
                ms: self.limits.time_ms,
            })
        } else {
            None
        }
    }
}

async fn run(
    code: &str,
    limits: ScriptLimits,
    cancel: &CancelToken,
    console: &Arc<Mutex<Console>>,
) -> Result<(), ScriptError> {
    let engine = |e: rquickjs::Error| ScriptError::Engine(e.to_string());
    let stop = Stop {
        cancel: cancel.clone(),
        deadline: Instant::now() + Duration::from_millis(limits.time_ms),
        limits,
    };
    let runtime = AsyncRuntime::new().map_err(engine)?;
    runtime.set_memory_limit(limits.memory_bytes).await;
    runtime.set_max_stack_size(STACK_BYTES).await;
    let interrupt = stop.clone();
    runtime
        .set_interrupt_handler(Some(Box::new(move || interrupt.reason().is_some())))
        .await;
    let context = AsyncContext::full(&runtime).await.map_err(|e| {
        // A limit too small for even an empty context.
        if matches!(e, rquickjs::Error::Allocation) {
            ScriptError::OutOfMemory {
                bytes: limits.memory_bytes,
            }
        } else {
            engine(e)
        }
    })?;
    let console = console.clone();
    context
        .async_with(async |ctx| {
            // Interrupted here too, if cancelled before it started.
            console::install(&ctx, console)
                .map_err(|e| stop.reason().unwrap_or_else(|| engine(e)))?;
            evaluate(&ctx, code, &stop).await
        })
        .await
}

/// Evaluates `code` as the body of an `async` function and waits for it.
async fn evaluate(ctx: &Ctx<'_>, code: &str, stop: &Stop) -> Result<(), ScriptError> {
    // On the first line, so the script's lines keep their numbers.
    let source = format!("(async () => {{{code}\n}})()");
    let mut options = EvalOptions::default();
    options.filename = Some(FILE.to_owned());
    // Sloppy, as scripts are in Bruno.
    options.strict = false;
    let failed = |caught| {
        stop.reason()
            .unwrap_or_else(|| errors::from_caught(ctx, caught, stop.limits))
    };
    // The code runs until its first `await` here, bounded by the interrupt
    // handler.
    let promise: Promise = ctx
        .eval_with_options(source, options)
        .catch(ctx)
        .map_err(failed)?;
    let finished = promise.into_future::<Value>();
    let deadline = tokio::time::Instant::from_std(stop.deadline);
    tokio::select! {
        result = finished => result.catch(ctx).map(drop).map_err(failed),
        () = tokio::time::sleep_until(deadline) => Err(ScriptError::TimedOut { ms: stop.limits.time_ms }),
        () = cancelled(&stop.cancel) => Err(ScriptError::Cancelled),
    }
}

async fn cancelled(cancel: &CancelToken) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}
