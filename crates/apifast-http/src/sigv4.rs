//! AWS Signature Version 4: signs a request as it goes out, after the URL
//! is encoded and the body is final.
//! <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv.html>

use std::fmt::Write as _;

use apifast_app::AwsV4Credentials;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::{Digest, Sha256};

use apifast_app::TransportError;

/// The payload hash of a body whose bytes are not at hand when signing
/// (files and multipart, which are streamed). S3 accepts it; other services
/// may not.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// SHA-256 of `bytes`, in lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    // HMAC takes keys of any length.
    #[expect(clippy::expect_used, reason = "HMAC accepts keys of any length")]
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Percent-encodes as SigV4 asks: everything but `A-Z a-z 0-9 - _ . ~`.
fn encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_slash && byte == b'/')
        {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Decodes `%XX` escapes; anything else stays as it is.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The path as SigV4 wants it: each segment encoded once for S3, twice for
/// the other services.
fn canonical_path(url: &Url, service: &str) -> String {
    let path = url.path();
    if path.is_empty() {
        return "/".into();
    }
    let once = encode(&decode(path), true);
    if service == "s3" {
        once
    } else {
        encode(&once, true)
    }
}

fn canonical_query(url: &Url) -> String {
    let Some(query) = url.query().filter(|q| !q.is_empty()) else {
        return String::new();
    };
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            // `+` in a form-encoded query is a space.
            let plain = |t: &str| decode(&t.replace('+', " "));
            (encode(&plain(key), false), encode(&plain(value), false))
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// The `Host` header value: the host, with the port when it is not the
/// scheme's default.
fn host(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// Signs: adds `X-Amz-Date`, the session token and the content hash when
/// needed, and `Authorization`. `amz_date` is `YYYYMMDDTHHMMSSZ` in UTC.
/// Signs `host`, `content-type` and every `x-amz-*` header.
///
/// # Errors
///
/// If a header value cannot be sent.
pub fn sign(
    method: &str,
    url: &Url,
    headers: &mut HeaderMap,
    payload_hash: &str,
    credentials: &AwsV4Credentials,
    amz_date: &str,
) -> Result<(), TransportError> {
    let invalid = |_| TransportError::InvalidRequest("invalid AWS signature header".into());
    let mut set = |name: &'static str, value: &str| -> Result<(), TransportError> {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).map_err(invalid)?,
        );
        Ok(())
    };
    set("x-amz-date", amz_date)?;
    if !credentials.session_token.is_empty() {
        set("x-amz-security-token", &credentials.session_token)?;
    }
    if credentials.service == "s3" {
        set("x-amz-content-sha256", payload_hash)?;
    }

    let mut signed: Vec<(String, String)> = vec![("host".into(), host(url))];
    for (name, value) in headers.iter() {
        let name = name.as_str();
        if name == "content-type" || name.starts_with("x-amz-") {
            let value = value.to_str().unwrap_or_default();
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            signed.push((name.to_owned(), value));
        }
    }
    signed.sort();
    let mut canonical_headers = String::new();
    for (name, value) in &signed {
        let _ = writeln!(canonical_headers, "{name}:{value}");
    }
    let signed_headers = signed
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{method}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        canonical_path(url, &credentials.service),
        canonical_query(url),
    );
    let date = amz_date.get(..8).unwrap_or_default();
    let scope = format!(
        "{date}/{}/{}/aws4_request",
        credentials.region, credentials.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let key = format!("AWS4{}", credentials.secret_access_key);
    let key = hmac(key.as_bytes(), date);
    let key = hmac(&key, &credentials.region);
    let key = hmac(&key, &credentials.service);
    let key = hmac(&key, "aws4_request");
    let signature = hex(&hmac(&key, &string_to_sign));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    );
    headers.insert(
        reqwest::header::AUTHORIZATION,
        HeaderValue::from_str(&authorization).map_err(invalid)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> AwsV4Credentials {
        AwsV4Credentials {
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: String::new(),
            service: "iam".into(),
            region: "us-east-1".into(),
        }
    }

    /// The example of AWS's documentation ("Create a signed AWS API
    /// request"): `GET` of IAM `ListUsers`.
    #[test]
    fn signs_the_documented_iam_example() {
        let url =
            Url::parse("https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=utf-8"),
        );
        sign(
            "GET",
            &url,
            &mut headers,
            &sha256_hex(b""),
            &example(),
            "20150830T123600Z",
        )
        .unwrap();
        assert_eq!(
            headers["authorization"],
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
        assert_eq!(headers["x-amz-date"], "20150830T123600Z");
    }

    #[test]
    fn queries_are_sorted_and_encoded() {
        let url = Url::parse("https://h/?b=2&a=x%20y&a=1&c").unwrap();
        assert_eq!(canonical_query(&url), "a=1&a=x%20y&b=2&c=");
        let url = Url::parse("https://h/?q=a+b").unwrap();
        assert_eq!(canonical_query(&url), "q=a%20b");
    }

    #[test]
    fn paths_are_encoded_once_for_s3_and_twice_otherwise() {
        let url = Url::parse("https://h/my%20file/a+b").unwrap();
        assert_eq!(canonical_path(&url, "s3"), "/my%20file/a%2Bb");
        assert_eq!(canonical_path(&url, "execute-api"), "/my%2520file/a%252Bb");
        let url = Url::parse("https://h").unwrap();
        assert_eq!(canonical_path(&url, "s3"), "/");
    }

    #[test]
    fn session_tokens_and_s3_add_their_headers() {
        let url = Url::parse("https://bucket.s3.amazonaws.com/key").unwrap();
        let mut credentials = example();
        credentials.service = "s3".into();
        credentials.session_token = "token".into();
        let mut headers = HeaderMap::new();
        sign(
            "PUT",
            &url,
            &mut headers,
            UNSIGNED_PAYLOAD,
            &credentials,
            "20150830T123600Z",
        )
        .unwrap();
        assert_eq!(headers["x-amz-security-token"], "token");
        assert_eq!(headers["x-amz-content-sha256"], UNSIGNED_PAYLOAD);
        let authorization = headers["authorization"].to_str().unwrap();
        assert!(
            authorization.contains(
                "SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token,"
            ),
            "{authorization}"
        );
    }
}
