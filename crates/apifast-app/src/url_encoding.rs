//! The `encodeUrl` setting: how Bruno encodes a URL before sending it
//! (`encodeUrl` in `@usebruno/common`), so the same file sends the same bytes
//! in both tools.
//!
//! - Scheme and authority are kept as written.
//! - Each path segment is decoded and encoded again, so a segment already
//!   encoded is not encoded twice.
//! - Each query name and value is encoded as it is, so `%20` becomes `%2520`:
//!   Bruno does it on purpose, for values the server decodes once more (such
//!   as a redirect URL).
//! - `#` is data, not a fragment: it becomes `%23`.
//!
//! With the setting off the URL goes out as written, and only the transport's
//! URL parser normalises it, as axios does in Bruno.

/// `url` encoded as Bruno does with `encodeUrl: true`.
pub(crate) fn encode_url(url: &str) -> String {
    let (before_query, query) = match url.split_once('?') {
        Some((before, query)) => (before, Some(query)),
        None => (url, None),
    };
    let authority_end = authority_end(before_query);
    let mut out = String::with_capacity(url.len() + url.len() / 2);
    out.push_str(&before_query[..authority_end]);
    for (i, segment) in before_query[authority_end..].split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        encode_component(&decode_component(segment), &mut out);
    }
    if let Some(query) = query {
        out.push('?');
        encode_query(query, &mut out);
    }
    out
}

/// Where the authority of `url` ends. A URL without a scheme starts with its
/// authority, as the transport sends it over `http://`.
fn authority_end(url: &str) -> usize {
    let authority_start = scheme_len(url).map_or(0, |len| len + 3);
    url[authority_start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |end| authority_start + end)
}

/// The length of `url`'s scheme, if it has one followed by `://`: a letter,
/// then letters, digits, `+`, `-` or `.` (the WHATWG scheme state).
fn scheme_len(url: &str) -> Option<usize> {
    let len = url.find("://")?;
    let mut chars = url[..len].chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(len)
}

/// Each `name[=value]` pair of `query` encoded; a pair without a name is
/// dropped, as Bruno's `parseQueryParams` does.
fn encode_query(query: &str, out: &mut String) {
    let mut first = true;
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (pair, None),
        };
        if name.is_empty() {
            continue;
        }
        if !first {
            out.push('&');
        }
        first = false;
        encode_component(name, out);
        if let Some(value) = value {
            out.push('=');
            encode_component(value, out);
        }
    }
}

/// JavaScript's `encodeURIComponent`: every byte but `A-Z a-z 0-9` and
/// `- _ . ! ~ * ' ( )` as `%XX`.
fn encode_component(text: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

/// Bruno's `safeDecodeURIComponent`: every `%XX` decoded if the whole text
/// decodes to UTF-8; otherwise only the ones below `%80`, and the rest kept
/// as written.
fn decode_component(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let all = decode_escapes(text, |_| true);
    if all.len() == text.len() - 2 * text.matches('%').count()
        && let Ok(decoded) = String::from_utf8(all)
    {
        return decoded;
    }
    let ascii = decode_escapes(text, |byte| byte < 0x80);
    // Only ASCII bytes were decoded into valid UTF-8 text.
    String::from_utf8(ascii).unwrap_or_else(|_| text.to_owned())
}

/// The bytes of `text` with each well-formed `%XX` whose value `decode`
/// accepts replaced by that byte.
fn decode_escapes(text: &str, decode: impl Fn(u8) -> bool) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok())
            .filter(|&byte| decode(byte));
        if let Some(byte) = escaped {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_and_query_parts_are_encoded() {
        assert_eq!(
            encode_url("https://api.test/users/a b/ñ?q=a,b&name=J Doe&flag"),
            "https://api.test/users/a%20b/%C3%B1?q=a%2Cb&name=J%20Doe&flag"
        );
    }

    #[test]
    fn the_path_is_encoded_once_and_the_query_as_written() {
        assert_eq!(
            encode_url("https://api.test/a%20b/c%2Fd?r=%2Fhome&s=a%20b"),
            "https://api.test/a%20b/c%2Fd?r=%252Fhome&s=a%2520b"
        );
    }

    #[test]
    fn scheme_authority_and_unreserved_marks_are_kept() {
        assert_eq!(
            encode_url("https://user:pw@[::1]:8443/x!~*'()_.-?k=!~*'()"),
            "https://user:pw@[::1]:8443/x!~*'()_.-?k=!~*'()"
        );
    }

    #[test]
    fn a_url_without_scheme_keeps_its_port() {
        assert_eq!(
            encode_url("localhost:6000/a:b?x=1:2"),
            "localhost:6000/a%3Ab?x=1%3A2"
        );
        assert_eq!(encode_url("localhost:6000"), "localhost:6000");
    }

    #[test]
    fn a_hash_is_data() {
        assert_eq!(
            encode_url("https://api.test/a#b?token=abc#def"),
            "https://api.test/a%23b?token=abc%23def"
        );
        assert_eq!(
            encode_url("https://api.test#top"),
            "https://api.test%23top",
            "the authority ends before #, which is then part of the path"
        );
        assert_eq!(encode_url("https://api.test"), "https://api.test");
    }

    #[test]
    fn query_pairs_follow_bruno_parse_query_params() {
        assert_eq!(
            encode_url("http://h/?a=1=2&&=x&b=&c"),
            "http://h/?a=1%3D2&b=&c"
        );
        assert_eq!(encode_url("http://h/p?"), "http://h/p?");
    }

    #[test]
    fn malformed_escapes_in_the_path_are_encoded() {
        // Not UTF-8 as a whole: only the ASCII escape is decoded.
        assert_eq!(encode_url("http://h/%41%C3%zz%"), "http://h/A%25C3%25zz%25");
        assert_eq!(encode_url("http://h/100%"), "http://h/100%25");
    }

    #[test]
    fn scheme_rules_match_whatwg() {
        assert_eq!(scheme_len("https://h"), Some(5));
        assert_eq!(scheme_len("web+x.1://h"), Some(7));
        assert_eq!(scheme_len("1http://h"), None);
        assert_eq!(scheme_len("://h"), None);
        assert_eq!(scheme_len("h/a?u=http://x"), None);
    }
}
