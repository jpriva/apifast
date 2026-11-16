//! Secret values: kept away from printing, wiped from memory when dropped,
//! and masked wherever a text that held them is shown or stored
//! (docs/06: "Protección en memoria y en la interfaz").

use std::borrow::Cow;
use std::fmt::{self, Write as _};

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

/// `value` percent-encoded as a URL would carry it: every byte but the
/// unreserved ones (`A-Z a-z 0-9 - . _ ~`) as `%XX`.
fn percent_encoded(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `text` with every secret value, and its percent-encoded form, replaced
/// by [`MASK`]. Longer values go first, so one that contains another is
/// masked whole. Empty values are ignored.
pub fn mask<'a>(text: &'a str, secrets: &[Secret]) -> Cow<'a, str> {
    let mut forms: Vec<String> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .flat_map(|value| [value.to_owned(), percent_encoded(value)])
        .collect();
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms.dedup();
    let mut out = Cow::Borrowed(text);
    for form in &forms {
        if out.contains(form.as_str()) {
            out = Cow::Owned(out.replace(form.as_str(), MASK));
        }
    }
    out
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
    fn the_longer_value_wins_and_text_without_secrets_is_not_copied() {
        let secrets = [Secret::new("abc"), Secret::new("abcdef"), Secret::new("")];
        assert_eq!(mask("xabcdefx", &secrets), format!("x{MASK}x"));
        assert!(matches!(mask("nothing here", &secrets), Cow::Borrowed(_)));
    }
}
