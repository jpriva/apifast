//! HTTP Digest authentication (RFC 7616, and RFC 2617's MD5): the answer to
//! a server's `WWW-Authenticate: Digest` challenge.

use std::fmt::Write as _;

use md5::Md5;
use sha2::{Digest as _, Sha256};

/// A `Digest` challenge, as the server sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// `MD5`, `MD5-sess`, `SHA-256` or `SHA-256-sess`.
    pub algorithm: String,
    /// The `qop` values offered, such as `auth`.
    pub qop: Vec<String>,
}

impl Challenge {
    /// The `Digest` challenge among the `WWW-Authenticate` values, if any.
    pub fn find<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        values.into_iter().find_map(Self::parse)
    }

    fn parse(value: &str) -> Option<Self> {
        let value = value.trim_start();
        let scheme = value.get(..6)?;
        if !scheme.eq_ignore_ascii_case("digest") {
            return None;
        }
        let params = parameters(&value[6..]);
        let get = |name: &str| {
            params
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        Some(Self {
            realm: get("realm").unwrap_or_default(),
            nonce: get("nonce")?,
            opaque: get("opaque"),
            algorithm: get("algorithm").unwrap_or_else(|| "MD5".into()),
            qop: get("qop")
                .map(|q| q.split(',').map(|v| v.trim().to_owned()).collect())
                .unwrap_or_default(),
        })
    }
}

/// `name=value` pairs separated by commas; values may be quoted, with `\`
/// escapes.
fn parameters(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let name: String = chars
            .by_ref()
            .take_while(|c| *c != '=')
            .collect::<String>()
            .trim()
            .to_owned();
        if name.is_empty() {
            return out;
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next()),
                    '"' => break,
                    c => value.push(c),
                }
            }
        } else {
            let mut raw = String::new();
            while let Some(c) = chars.peek().copied().filter(|c| *c != ',') {
                raw.push(c);
                chars.next();
            }
            value.push_str(raw.trim());
        }
        out.push((name, value));
    }
}

/// What an answer needs besides the challenge.
pub struct Answer<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub method: &'a str,
    /// The path and query, as in the request line.
    pub uri: &'a str,
    /// The client nonce.
    pub cnonce: &'a str,
}

fn hash(algorithm: &str, text: &str) -> Option<String> {
    let bytes: Vec<u8> = if algorithm.to_ascii_uppercase().starts_with("SHA-256") {
        Sha256::digest(text.as_bytes()).to_vec()
    } else if algorithm.to_ascii_uppercase().starts_with("MD5") {
        Md5::digest(text.as_bytes()).to_vec()
    } else {
        return None;
    };
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    Some(out)
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The `Authorization` header that answers `challenge`. Uses `qop=auth`
/// when offered (or no `qop` for an RFC 2069 server); `auth-int` alone is
/// not supported.
///
/// # Errors
///
/// If the algorithm or the `qop` offered is not supported.
pub fn authorization(challenge: &Challenge, answer: &Answer<'_>) -> Result<String, String> {
    let algorithm = challenge.algorithm.as_str();
    let unsupported = || format!("Digest algorithm `{algorithm}` is not supported");
    let qop = if challenge.qop.is_empty() {
        None
    } else if challenge.qop.iter().any(|q| q.eq_ignore_ascii_case("auth")) {
        Some("auth")
    } else {
        return Err(format!(
            "the server asks Digest with qop `{}`, which apifast does not support",
            challenge.qop.join(", ")
        ));
    };
    let h = |text: &str| hash(algorithm, text).ok_or_else(unsupported);
    let nc = "00000001";
    let mut ha1 = h(&format!(
        "{}:{}:{}",
        answer.username, challenge.realm, answer.password
    ))?;
    if algorithm.to_ascii_lowercase().ends_with("-sess") {
        ha1 = h(&format!("{ha1}:{}:{}", challenge.nonce, answer.cnonce))?;
    }
    let ha2 = h(&format!("{}:{}", answer.method, answer.uri))?;
    let response = match qop {
        Some(qop) => h(&format!(
            "{ha1}:{}:{nc}:{}:{qop}:{ha2}",
            challenge.nonce, answer.cnonce
        ))?,
        None => h(&format!("{ha1}:{}:{ha2}", challenge.nonce))?,
    };
    let mut header = format!(
        "Digest username={}, realm={}, nonce={}, uri={}, algorithm={algorithm}, response={}",
        quoted(answer.username),
        quoted(&challenge.realm),
        quoted(&challenge.nonce),
        quoted(answer.uri),
        quoted(&response),
    );
    if let Some(qop) = qop {
        let _ = write!(
            header,
            ", qop={qop}, nc={nc}, cnonce={}",
            quoted(answer.cnonce)
        );
    }
    if let Some(opaque) = &challenge.opaque {
        let _ = write!(header, ", opaque={}", quoted(opaque));
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_of(header: &str) -> String {
        let params = parameters(header.trim_start_matches("Digest"));
        params
            .into_iter()
            .find(|(n, _)| n == "response")
            .map(|(_, v)| v)
            .unwrap_or_default()
    }

    /// RFC 2617, section 3.5.
    #[test]
    fn answers_the_rfc_2617_example() {
        let challenge = Challenge::find([
            "Basic realm=\"x\"",
            "Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", \
             nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", \
             opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"",
        ])
        .unwrap();
        assert_eq!(challenge.qop, ["auth", "auth-int"]);
        let header = authorization(
            &challenge,
            &Answer {
                username: "Mufasa",
                password: "Circle Of Life",
                method: "GET",
                uri: "/dir/index.html",
                cnonce: "0a4f113b",
            },
        )
        .unwrap();
        assert_eq!(response_of(&header), "6629fae49393a05397450978507c4ef1");
        assert!(header.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
        assert!(header.contains("qop=auth, nc=00000001, cnonce=\"0a4f113b\""));
    }

    /// RFC 7616, section 3.9.1, with SHA-256 and with MD5.
    #[test]
    fn answers_the_rfc_7616_examples() {
        let answer = Answer {
            username: "Mufasa",
            password: "Circle of Life",
            method: "GET",
            uri: "/dir/index.html",
            cnonce: "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ",
        };
        let challenge = |algorithm: &str| Challenge {
            realm: "http-auth@example.org".into(),
            nonce: "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v".into(),
            opaque: Some("FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS".into()),
            algorithm: algorithm.into(),
            qop: vec!["auth".into(), "auth-int".into()],
        };
        let sha = authorization(&challenge("SHA-256"), &answer).unwrap();
        assert_eq!(
            response_of(&sha),
            "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"
        );
        let md5 = authorization(&challenge("MD5"), &answer).unwrap();
        assert_eq!(response_of(&md5), "8ca523f5e9506fed4657c9700eebdbec");
    }

    #[test]
    fn unsupported_challenges_say_why() {
        let mut challenge = Challenge::find(["Digest nonce=\"n\", qop=\"auth-int\""]).unwrap();
        let answer = Answer {
            username: "u",
            password: "p",
            method: "GET",
            uri: "/",
            cnonce: "c",
        };
        assert!(
            authorization(&challenge, &answer)
                .unwrap_err()
                .contains("auth-int")
        );
        challenge.qop.clear();
        challenge.algorithm = "SHA-512-256".into();
        assert!(
            authorization(&challenge, &answer)
                .unwrap_err()
                .contains("SHA-512-256")
        );
        assert_eq!(Challenge::find(["Basic realm=\"x\""]), None);
        assert_eq!(Challenge::find(["Digest realm=\"no nonce\""]), None);
    }
}
