//! What a script's exceptions become.

use apifast_app::{ScriptError, ScriptLimits};
use rquickjs::{CaughtError, Ctx, Exception, Value};

use crate::FILE;

/// `caught`, raised by the script or by `QuickJS`, as a [`ScriptError`].
pub(crate) fn from_caught<'js>(
    ctx: &Ctx<'js>,
    caught: CaughtError<'js>,
    limits: ScriptLimits,
) -> ScriptError {
    match caught {
        CaughtError::Exception(exception) => from_exception(&exception, limits),
        // QuickJS throws `null` when it cannot even allocate the error for
        // running out of memory. A script could throw `null` itself, but
        // no script means to.
        CaughtError::Value(value) if value.is_null() => ScriptError::OutOfMemory {
            bytes: limits.memory_bytes,
        },
        CaughtError::Value(value) => ScriptError::Thrown {
            message: shown(ctx, value),
            line: None,
        },
        CaughtError::Error(rquickjs::Error::Allocation) => ScriptError::OutOfMemory {
            bytes: limits.memory_bytes,
        },
        CaughtError::Error(error) => ScriptError::Engine(error.to_string()),
    }
}

fn from_exception(exception: &Exception<'_>, limits: ScriptLimits) -> ScriptError {
    let name: Option<String> = exception.get("name").ok();
    let message = exception.message().unwrap_or_default();
    let line = exception.stack().as_deref().and_then(script_line);
    match name.as_deref() {
        Some("SyntaxError") => ScriptError::Syntax { message, line },
        Some("InternalError") if message == "out of memory" => ScriptError::OutOfMemory {
            bytes: limits.memory_bytes,
        },
        Some(name) if !name.is_empty() => ScriptError::Thrown {
            message: format!("{name}: {message}"),
            line,
        },
        _ => ScriptError::Thrown { message, line },
    }
}

/// A value thrown that is not an `Error`, as text.
fn shown<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> String {
    if let Some(text) = value.as_string().and_then(|s| s.to_string().ok()) {
        return text;
    }
    match ctx.json_stringify(value) {
        Ok(Some(json)) => json.to_string().unwrap_or_default(),
        _ => "a value that is not an Error".to_owned(),
    }
}

/// The script's line in `stack`: its first frame in [`FILE`], as
/// `at f (script:3:7)` or, for a syntax error, `at script:3:7`.
fn script_line(stack: &str) -> Option<u32> {
    stack.lines().find_map(|frame| {
        let (_, rest) = frame.split_once(&format!("{FILE}:"))?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_is_the_first_frame_in_the_script() {
        let stack = "    at f (script:3:7)\n    at <anonymous> (script:9:1)\n";
        assert_eq!(script_line(stack), Some(3));
        assert_eq!(script_line("    at script:12:4\n"), Some(12));
        assert_eq!(script_line("    at native\n"), None);
    }
}
