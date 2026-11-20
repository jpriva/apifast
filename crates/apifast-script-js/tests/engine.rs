//! The engine against real scripts: what they write, how they fail, and
//! their limits.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::time::{Duration, Instant};

use apifast_app::{
    CancelToken, ConsoleLevel, ConsoleLine, Script, ScriptEngine, ScriptError, ScriptLimits,
    ScriptOutcome,
};
use apifast_script_js::QuickJs;

const LIMITS: ScriptLimits = ScriptLimits {
    time_ms: 300,
    memory_bytes: 16 << 20,
};

async fn run(code: &str) -> ScriptOutcome {
    let script = Script {
        name: "request",
        code,
    };
    QuickJs::new(LIMITS).run(script, &CancelToken::new()).await
}

fn log(text: &str) -> ConsoleLine {
    ConsoleLine {
        level: ConsoleLevel::Log,
        text: text.to_owned(),
    }
}

#[tokio::test]
async fn the_console_keeps_what_the_script_writes() {
    let outcome = run(r#"
        console.log("user", 42, {id: 1}, [1, "a"], null, undefined);
        console.warn(new TypeError("bad"));
        console.error("x"); console.info("i"); console.debug("d");
    "#)
    .await;
    assert_eq!(outcome.error, None);
    let levels: Vec<ConsoleLevel> = outcome.console.iter().map(|l| l.level).collect();
    assert_eq!(
        levels,
        [
            ConsoleLevel::Log,
            ConsoleLevel::Warn,
            ConsoleLevel::Error,
            ConsoleLevel::Info,
            ConsoleLevel::Debug
        ]
    );
    assert_eq!(
        outcome.console[0].text,
        r#"user 42 {"id":1} [1,"a"] null undefined"#
    );
    assert_eq!(outcome.console[1].text, "TypeError: bad");
}

#[tokio::test]
async fn a_script_may_await_and_return() {
    let outcome = run(r"
        const value = await Promise.resolve(5);
        console.log(value);
        if (value === 5) return;
        console.log('not reached');
    ")
    .await;
    assert_eq!(
        outcome,
        ScriptOutcome {
            console: vec![log("5")],
            error: None
        }
    );
}

#[tokio::test]
async fn sloppy_code_runs_as_in_bruno() {
    let outcome = run("undeclared = 1; console.log(undeclared)").await;
    assert_eq!(outcome.error, None);
}

#[tokio::test]
async fn an_exception_says_its_type_and_line() {
    let outcome = run("console.log('before');\nconst user = null;\nuser.name").await;
    assert_eq!(outcome.console, [log("before")]);
    assert_eq!(
        outcome.error,
        Some(ScriptError::Thrown {
            message: "TypeError: cannot read property 'name' of null".into(),
            line: Some(3),
        })
    );
}

#[tokio::test]
async fn a_rejection_and_a_thrown_value_are_errors_too() {
    let rejected = run("\nawait Promise.reject(new Error('no'))").await;
    assert_eq!(
        rejected.error,
        Some(ScriptError::Thrown {
            message: "Error: no".into(),
            line: Some(2)
        })
    );
    let thrown = run("throw {code: 7}").await;
    assert_eq!(
        thrown.error,
        Some(ScriptError::Thrown {
            message: r#"{"code":7}"#.into(),
            line: None
        })
    );
    let text = run("throw 'plain'").await;
    assert_eq!(
        text.error,
        Some(ScriptError::Thrown {
            message: "plain".into(),
            line: None
        })
    );
}

#[tokio::test]
async fn a_syntax_error_is_found_before_anything_runs() {
    let outcome = run("console.log('never');\nlet x = ;").await;
    assert_eq!(outcome.console, []);
    let Some(ScriptError::Syntax { line, .. }) = outcome.error else {
        panic!("not a syntax error: {:?}", outcome.error);
    };
    assert_eq!(line, Some(2));
}

#[tokio::test]
async fn a_script_that_keeps_running_is_stopped() {
    let started = Instant::now();
    let outcome = run("console.log('start'); while (true) {}").await;
    assert_eq!(outcome.error, Some(ScriptError::TimedOut { ms: 300 }));
    assert_eq!(outcome.console, [log("start")]);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_script_that_keeps_waiting_is_stopped() {
    let started = Instant::now();
    let outcome = run("await new Promise(() => {})").await;
    assert_eq!(outcome.error, Some(ScriptError::TimedOut { ms: 300 }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_script_cannot_catch_its_time_limit() {
    let outcome = run("try { while (true) {} } catch (e) { console.log('caught') }").await;
    assert_eq!(outcome.error, Some(ScriptError::TimedOut { ms: 300 }));
    assert_eq!(outcome.console, []);
}

#[tokio::test]
async fn a_script_that_uses_too_much_memory_is_stopped() {
    let outcome = run("const all = []; while (true) all.push('x'.repeat(4096));").await;
    assert_eq!(
        outcome.error,
        Some(ScriptError::OutOfMemory { bytes: 16 << 20 })
    );
}

#[tokio::test]
async fn deep_recursion_is_an_error_not_a_crash() {
    let outcome = run("function down(n) { return down(n + 1) + 1 }\ndown(0)").await;
    let Some(ScriptError::Thrown { message, line }) = outcome.error else {
        panic!("not thrown: {:?}", outcome.error);
    };
    assert!(message.contains("stack"), "{message}");
    assert_eq!(line, Some(1));
}

#[tokio::test]
async fn cancelling_stops_a_running_script() {
    let cancel = CancelToken::new();
    let engine = QuickJs::new(ScriptLimits {
        time_ms: 10_000,
        ..LIMITS
    });
    let canceller = cancel.clone();
    // From another thread, as the interface cancels: the spinning script
    // holds this one.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        canceller.cancel();
    });
    let started = Instant::now();
    let spin = Script {
        name: "request",
        code: "while (true) {}",
    };
    let outcome = engine.run(spin, &cancel).await;
    assert_eq!(outcome.error, Some(ScriptError::Cancelled));
    let waiting = Script {
        name: "request",
        code: "await new Promise(() => {})",
    };
    let outcome = engine.run(waiting, &cancel).await;
    assert_eq!(outcome.error, Some(ScriptError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_script_reaches_neither_files_nor_modules() {
    let outcome = run(r"
        console.log(typeof require, typeof std, typeof os, typeof fetch, typeof process);
    ")
    .await;
    assert_eq!(
        outcome.console,
        [log("undefined undefined undefined undefined undefined")]
    );
    let import = run("await import('os')").await;
    assert!(import.error.is_some());
}

#[tokio::test]
async fn nothing_is_kept_between_scripts() {
    let engine = QuickJs::new(LIMITS);
    let cancel = CancelToken::new();
    let first = Script {
        name: "a",
        code: "globalThis.kept = 1",
    };
    engine.run(first, &cancel).await;
    let second = Script {
        name: "b",
        code: "console.log(typeof kept)",
    };
    let outcome = engine.run(second, &cancel).await;
    assert_eq!(outcome.console, [log("undefined")]);
}

#[tokio::test]
async fn a_script_runs_on_any_thread() {
    let handle = tokio::spawn(async { run("console.log('moved')").await });
    assert_eq!(handle.await.unwrap().console, [log("moved")]);
}

#[tokio::test]
async fn the_console_keeps_at_most_a_megabyte() {
    let outcome = run("for (let i = 0; i < 300; i++) console.log('x'.repeat(8192))").await;
    assert_eq!(outcome.error, None);
    let kept: usize = outcome.console.iter().map(|l| l.text.len()).sum();
    assert!(kept <= (1 << 20) + 100, "{kept}");
    assert!(outcome.console.last().unwrap().text.contains("dropped"));
}
