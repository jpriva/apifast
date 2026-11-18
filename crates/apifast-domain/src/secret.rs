//! Secret values: kept away from printing, wiped from memory when dropped,
//! and masked wherever a text that held them is shown or stored
//! (docs/06: "Protección en memoria y en la interfaz").

use std::borrow::Cow;
use std::fmt;

use zeroize::Zeroizing;

/// What a secret value is replaced with when shown.
pub const MASK: &str = "••••••";

/// A secret value. Printing it shows [`MASK`]; its memory is wiped when it
/// is dropped. Only [`Secret::expose`] gives the value, to use it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wraps `value`.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The value, to use it. Never to show it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({MASK})")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

/// `text` with every secret value replaced by [`MASK`], also where a URL
/// carries it encoded: each byte as written or as `%XX` (any case), in any
/// mix, since encoders differ on which characters they leave as written.
/// Longer values go first, so one that contains another is masked whole.
/// Empty values are ignored.
pub fn mask<'a>(text: &'a str, secrets: &[Secret]) -> Cow<'a, str> {
    let mut values: Vec<&str> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .collect();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    let mut out = Cow::Borrowed(text);
    for value in values {
        if let Some(masked) = mask_value(&out, value.as_bytes()) {
            out = Cow::Owned(masked);
        }
    }
    out
}

/// `text` with each occurrence of `value` replaced by [`MASK`], or `None`
/// if there is none.
fn mask_value(text: &str, value: &[u8]) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match match_len(&bytes[i..], value) {
            Some(len) => {
                let masked = out.get_or_insert_with(|| String::with_capacity(text.len()));
                // `i` and `copied` are at character boundaries: a match starts
                // with a whole character or `%`, and ends after one or `%XX`.
                masked.push_str(&text[copied..i]);
                masked.push_str(MASK);
                i += len;
                copied = i;
            }
            None => i += 1,
        }
    }
    let mut masked = out?;
    masked.push_str(&text[copied..]);
    Some(masked)
}

/// How many bytes at the start of `text` spell `value`, each byte as written
/// or as `%XX`.
fn match_len(text: &[u8], value: &[u8]) -> Option<usize> {
    let mut at = 0;
    for (i, &byte) in value.iter().enumerate() {
        let literal = text.get(at) == Some(&byte);
        let escaped = text.get(at) == Some(&b'%')
            && text.get(at + 1..at + 3).and_then(hex_value) == Some(byte);
        match (literal, escaped) {
            // Only a `%` in the value can be both: as is, or as `%25`. The
            // recursion is as deep as the value has `%`.
            (true, true) => {
                let rest = &value[i + 1..];
                return match_len(&text[at + 1..], rest)
                    .map(|len| at + 1 + len)
                    .or_else(|| match_len(&text[at + 3..], rest).map(|len| at + 3 + len));
            }
            (true, false) => at += 1,
            (false, true) => at += 3,
            (false, false) => return None,
        }
    }
    Some(at)
}

/// The byte two hex digits spell.
fn hex_value(hex: &[u8]) -> Option<u8> {
    let digit = |c: u8| char::from(c).to_digit(16);
    let value = digit(hex[0])? * 16 + digit(hex[1])?;
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints_its_value() {
        let secret = Secret::new("s3cr3t");
        assert_eq!(format!("{secret}"), MASK);
        assert_eq!(format!("{secret:?}"), format!("Secret({MASK})"));
        assert_eq!(secret.expose(), "s3cr3t");
    }

    #[test]
    fn values_are_masked_plain_and_percent_encoded() {
        let secrets = [Secret::new("a b&c"), Secret::new("t0k")];
        assert_eq!(
            mask("https://h/t0k/x?k=a%20b%26c&raw=a b&c", &secrets),
            format!("https://h/{MASK}/x?k={MASK}&raw={MASK}")
        );
    }

    #[test]
    fn values_are_masked_in_any_mix_of_encodings() {
        let secrets = [Secret::new("a b!'(ñ")];
        // encodeURIComponent, then WHATWG on top of it, then a hand-made mix.
        for carried in ["a%20b!'(%C3%B1", "a%20b!%27(%c3%b1", "a b%21%27%28%C3%B1"] {
            assert_eq!(mask(carried, &secrets), MASK, "{carried}");
        }
        let pct = [Secret::new("p%41ss")];
        assert_eq!(
            mask("?k=p%2541ss&j=p%41ss", &pct),
            format!("?k={MASK}&j={MASK}")
        );
        assert_eq!(mask("ñ%2", &[Secret::new("x")]), "ñ%2");
    }

    #[test]
    fn the_longer_value_wins_and_text_without_secrets_is_not_copied() {
        let secrets = [Secret::new("abc"), Secret::new("abcdef"), Secret::new("")];
        assert_eq!(mask("xabcdefx", &secrets), format!("x{MASK}x"));
        assert!(matches!(mask("nothing here", &secrets), Cow::Borrowed(_)));
    }
}
