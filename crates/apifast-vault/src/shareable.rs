//! The collection's key as people pass it to each other (docs/06):
//!
//! ```text
//! AFK-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXXXXX
//! ```
//!
//! `AFK`, then 54 characters of Crockford base32 in groups of 6: 52 for the
//! 32 bytes of the key and 2 of a check, so a typo is caught before the key
//! is tried. Reading ignores case, dashes and spaces, and takes `I` and `L`
//! for `1` and `O` for `0`, as Crockford intends.

use crate::{KEY_LEN, Key};

const PREFIX: &str = "AFK";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters for the key: 256 bits in 5-bit symbols.
const KEY_CHARS: usize = 52;
/// Characters of the check: 10 bits of a CRC-16 of the key.
const CHECK_CHARS: usize = 2;
const GROUP: usize = 6;

/// Why a shareable key cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ShareableKeyError {
    /// Not the shape of a key: wrong length or characters.
    #[error("this is not an apifast collection key (`AFK-` and 54 characters)")]
    Format,
    /// The shape is right but the check does not match: a typo.
    #[error("the key has a typo: its check does not match")]
    Check,
}

/// CRC-16/CCITT-FALSE of `bytes`.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

/// The check symbols of `key`: the top 10 bits of its CRC-16.
fn check(key: &[u8; KEY_LEN]) -> [u8; CHECK_CHARS] {
    let bits = crc16(key) >> 6;
    [
        ALPHABET[usize::from(bits >> 5)],
        ALPHABET[usize::from(bits & 0x1f)],
    ]
}

/// `key` as people pass it.
pub fn shareable_key(key: &Key) -> String {
    let bytes = key.as_bytes();
    let mut symbols = Vec::with_capacity(KEY_CHARS + CHECK_CHARS);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            symbols.push(ALPHABET[((buffer >> bits) & 0x1f) as usize]);
        }
    }
    if bits > 0 {
        symbols.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize]);
    }
    symbols.extend_from_slice(&check(bytes));
    let groups: Vec<&str> = symbols
        .chunks(GROUP)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect();
    format!("{PREFIX}-{}", groups.join("-"))
}

/// The value of a symbol, read leniently.
fn value(symbol: u8) -> Option<u32> {
    let symbol = match symbol.to_ascii_uppercase() {
        b'I' | b'L' => b'1',
        b'O' => b'0',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&c| c == symbol)
        .and_then(|i| u32::try_from(i).ok())
}

/// The key in `text`, as people pass it.
///
/// # Errors
///
/// [`ShareableKeyError::Format`] if it is not shaped like a key;
/// [`ShareableKeyError::Check`] if its check does not match.
pub fn parse_shareable_key(text: &str) -> Result<Key, ShareableKeyError> {
    let compact: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'-')
        .collect();
    let body = compact
        .strip_prefix(PREFIX.as_bytes())
        .or_else(|| compact.strip_prefix(b"afk"))
        .unwrap_or(&compact);
    if body.len() != KEY_CHARS + CHECK_CHARS {
        return Err(ShareableKeyError::Format);
    }
    let (key_part, check_part) = body.split_at(KEY_CHARS);
    let mut bytes = [0u8; KEY_LEN];
    let (mut buffer, mut bits, mut at) = (0u32, 0u32, 0);
    for &symbol in key_part {
        buffer = (buffer << 5) | value(symbol).ok_or(ShareableKeyError::Format)?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            let byte =
                u8::try_from((buffer >> bits) & 0xff).map_err(|_| ShareableKeyError::Format)?;
            *bytes.get_mut(at).ok_or(ShareableKeyError::Format)? = byte;
            at += 1;
        }
    }
    // The 4 bits left over are padding: always zero when written by apifast.
    if at != KEY_LEN || buffer & ((1 << bits) - 1) != 0 {
        return Err(ShareableKeyError::Format);
    }
    let expected = check(&bytes);
    let given: Vec<u8> = check_part
        .iter()
        .map(|&s| value(s).map(|v| ALPHABET[v as usize]))
        .collect::<Option<_>>()
        .ok_or(ShareableKeyError::Format)?;
    if given != expected {
        return Err(ShareableKeyError::Check);
    }
    Ok(Key::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalogued check value of CRC-16/CCITT-FALSE.
    #[test]
    fn the_check_uses_the_standard_crc() {
        assert_eq!(crc16(b"123456789"), 0x29b1);
    }
}
