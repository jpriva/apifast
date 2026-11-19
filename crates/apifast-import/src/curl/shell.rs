//! A command line split into words as a POSIX shell does: single quotes,
//! double quotes, backslashes, `$'…'` (ANSI-C quoting) and line
//! continuations. Nothing is expanded: `$HOST` stays as written.

/// Why a command cannot be split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellError {
    /// A quote opened and not closed.
    Unclosed(char),
}

/// The words of `command`.
pub(crate) fn words(command: &str) -> Result<Vec<String>, ShellError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\\' => match chars.next() {
                // A line continuation joins the lines; a last `\` is nothing.
                Some('\n') | None => {}
                Some('\r') => {
                    chars.next_if_eq(&'\n');
                }
                Some(next) => {
                    word.push(next);
                    in_word = true;
                }
            },
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(ShellError::Unclosed('\'')),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        // Inside double quotes only these are escaped.
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => word.push(e),
                            Some('\n') => {}
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err(ShellError::Unclosed('"')),
                        },
                        Some(c) => word.push(c),
                        None => return Err(ShellError::Unclosed('"')),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                ansi_c(&mut chars, &mut word)?;
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// The rest of a `$'…'` word, with its escapes decoded.
fn ansi_c(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> Result<(), ShellError> {
    loop {
        match chars.next() {
            Some('\'') => return Ok(()),
            Some('\\') => {
                let decoded = match chars.next() {
                    Some('n') => '\n',
                    Some('r') => '\r',
                    Some('t') => '\t',
                    Some('0') => '\0',
                    Some('x') => hex(chars, 2).unwrap_or('x'),
                    Some('u') => hex(chars, 4).unwrap_or('u'),
                    Some(other) => other,
                    None => return Err(ShellError::Unclosed('\'')),
                };
                word.push(decoded);
            }
            Some(c) => word.push(c),
            None => return Err(ShellError::Unclosed('\'')),
        }
    }
}

/// The character of the next `max` (or fewer) hex digits.
fn hex(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, max: usize) -> Option<char> {
    let mut digits = String::new();
    while digits.len() < max
        && let Some(c) = chars.next_if(char::is_ascii_hexdigit)
    {
        digits.push(c);
    }
    u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(command: &str) -> Vec<String> {
        words(command).unwrap()
    }

    #[test]
    fn quotes_and_escapes_make_words() {
        assert_eq!(
            split(r#"curl 'a b' "c \"d\" \$e" f\ g h'i'"j""#),
            ["curl", "a b", r#"c "d" $e"#, "f g", "hij"]
        );
        assert_eq!(split(r#""\n is kept""#), [r"\n is kept"]);
        assert_eq!(split("''"), [""]);
    }

    #[test]
    fn line_continuations_join_lines() {
        assert_eq!(
            split("curl \\\n  -H 'a: b' \\\r\n  https://x"),
            ["curl", "-H", "a: b", "https://x"]
        );
    }

    #[test]
    fn ansi_c_quoting_is_decoded() {
        assert_eq!(split(r"$'a\nb\t\x41é\''"), ["a\nb\tAé'"]);
    }

    #[test]
    fn an_open_quote_is_an_error() {
        assert_eq!(words("curl 'oops"), Err(ShellError::Unclosed('\'')));
        assert_eq!(words("curl \"oops"), Err(ShellError::Unclosed('"')));
    }
}
