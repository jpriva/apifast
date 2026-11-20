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

mod api;
mod console;
mod errors;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use apifast_app::{
    CancelToken, Script, ScriptEngine, ScriptError, ScriptFuture, ScriptInput, ScriptLimits,
    ScriptOutcome,
};
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
    fn run<'a>(
        &'a self,
        script: Script<'a>,
        input: &'a ScriptInput,
        cancel: &'a CancelToken,
    ) -> ScriptFuture<'a> {
        let (limits, cancel) = (self.limits, cancel.clone());
        let (code, input) = (script.code.to_owned(), input.clone());
        Box::pin(async move {
            let console = Arc::new(Mutex::new(Console::default()));
            let mut outcome = run(&code, &input, limits, &cancel, &console)
                .await
                .unwrap_or_else(|error| ScriptOutcome {
                    error: Some(error),
                    ..ScriptOutcome::default()
                });
            outcome.console = console::take(&console);
            outcome
        })
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

/// Runs `code`: what it changed, and why it failed; an error only when it
/// could not start, or was stopped by its limits.
async fn run(
    code: &str,
    input: &ScriptInput,
    limits: ScriptLimits,
    cancel: &CancelToken,
    console: &Arc<Mutex<Console>>,
) -> Result<ScriptOutcome, ScriptError> {
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
                .and_then(|()| api::install(&ctx, input))
                .map_err(|e| stop.reason().unwrap_or_else(|| engine(e)))?;
            let mut outcome = ScriptOutcome {
                error: evaluate(&ctx, code, &stop).await.err(),
                ..ScriptOutcome::default()
            };
            if let Some(stopped) = outcome.error.as_ref().filter(|e| stopped(e)) {
                return Err(stopped.clone());
            }
            // Its tests may still be waiting.
            let settled = api::settle(&ctx).catch(&ctx).map_err(|_| ());
            if let Ok(settled) = settled {
                wait(&ctx, settled, &stop).await?;
            }
            api::read_output(&ctx, &mut outcome)
                .map_err(|e| stop.reason().unwrap_or_else(|| engine(e)))?;
            Ok(outcome)
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
    wait(ctx, promise, stop).await
}

/// Waits for `promise`, until the deadline or a cancel.
async fn wait(ctx: &Ctx<'_>, promise: Promise<'_>, stop: &Stop) -> Result<(), ScriptError> {
    let failed = |caught| {
        stop.reason()
            .unwrap_or_else(|| errors::from_caught(ctx, caught, stop.limits))
    };
    let finished = promise.into_future::<Value>();
    let deadline = tokio::time::Instant::from_std(stop.deadline);
    tokio::select! {
        result = finished => result.catch(ctx).map(drop).map_err(failed),
        () = tokio::time::sleep_until(deadline) => Err(ScriptError::TimedOut { ms: stop.limits.time_ms }),
        () = cancelled(&stop.cancel) => Err(ScriptError::Cancelled),
    }
}

/// Whether `error` stopped the script from outside: then what it changed
/// is not kept.
fn stopped(error: &ScriptError) -> bool {
    matches!(
        error,
        ScriptError::TimedOut { .. } | ScriptError::Cancelled | ScriptError::OutOfMemory { .. }
    )
}

async fn cancelled(cancel: &CancelToken) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}
