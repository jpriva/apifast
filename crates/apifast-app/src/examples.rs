//! A response saved as an example of its request (docs/09, "Panel de
//! respuesta"): the request as written, with its templates (never the
//! rendered one, which may hold secrets), and the response with its body.

use apifast_domain::{
    Example, ExampleBody, ExampleRequest, ExampleResponse, Header, HttpRequest, reason_phrase,
};

use crate::ResponseHead;

/// The largest body an example keeps, in bytes: it goes into the request's
/// file, and so to git.
pub const EXAMPLE_LIMIT: usize = 1024 * 1024;

/// Why a response cannot be an example.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExampleError {
    /// The body is larger than [`EXAMPLE_LIMIT`].
    #[error("the body is larger than {} KB, too large for an example", EXAMPLE_LIMIT / 1024)]
    TooLarge,
    /// The body is not text.
    #[error("the body is not text; binary bodies cannot be saved as an example yet")]
    Binary,
}

/// `request` and its response (`head` and the whole `body`) as an example
/// named `name`. `Set-Cookie` headers are left out: they are sessions, and
/// the example goes to git.
///
/// # Errors
///
/// If the body is too large or not text.
pub fn example_from(
    name: &str,
    request: &HttpRequest,
    head: &ResponseHead,
    body: &[u8],
) -> Result<Example, ExampleError> {
    if body.len() > EXAMPLE_LIMIT {
        return Err(ExampleError::TooLarge);
    }
    let data = std::str::from_utf8(body).map_err(|_| ExampleError::Binary)?;
    let content_type = (head.headers.iter())
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map_or("", |(_, v)| v.as_str());
    let headers = (head.headers.iter())
        .filter(|(n, _)| !n.eq_ignore_ascii_case("set-cookie"))
        .map(|(n, v)| Header::new(n.clone(), v.clone()))
        .collect();
    Ok(Example {
        name: name.trim().to_owned(),
        description: None,
        request: Some(ExampleRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.clone(),
            params: request.params.clone(),
            body: request.body.clone(),
        }),
        response: Some(ExampleResponse {
            status: Some(head.status),
            status_text: reason_phrase(head.status).map(str::to_owned),
            headers,
            body: (!data.is_empty()).then(|| ExampleBody {
                kind: ExampleBody::kind_for(content_type).to_owned(),
                data: data.to_owned(),
            }),
        }),
    })
}

/// A name for a new example of a response with `status`, not taken by any
/// of `examples`: `200 OK`, then `200 OK (2)`...
pub fn example_name(status: u16, examples: &[Example]) -> String {
    let base = reason_phrase(status)
        .map_or_else(|| status.to_string(), |phrase| format!("{status} {phrase}"));
    let taken = |name: &str| examples.iter().any(|e| e.name == name);
    let mut name = base.clone();
    let mut n = 2;
    while taken(&name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use apifast_domain::Method;

    use super::*;

    fn head(status: u16, headers: &[(&str, &str)]) -> ResponseHead {
        ResponseHead {
            status,
            version: "HTTP/1.1".into(),
            url: "http://api.test/users/1?token=s3cret".into(),
            redirects: 0,
            headers: (headers.iter())
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            elapsed_ms: 1,
            timeline: Vec::new(),
        }
    }

    #[test]
    fn the_request_as_written_and_the_response_make_the_example() {
        let mut request = HttpRequest::new("Get", Method::Get, "{{host}}/users/1?token={{token}}");
        request
            .headers
            .push(Header::new("Authorization", "Bearer {{token}}"));
        let head = head(
            200,
            &[
                ("Content-Type", "application/json"),
                ("Set-Cookie", "sid=abc"),
                ("X-Id", "7"),
            ],
        );
        let example = example_from(" Found ", &request, &head, br#"{"id":1}"#).unwrap();
        assert_eq!(example.name, "Found");
        let sent = example.request.unwrap();
        assert_eq!(sent.url, "{{host}}/users/1?token={{token}}");
        assert_eq!(sent.headers[0].value, "Bearer {{token}}");
        let response = example.response.unwrap();
        assert_eq!(response.status, Some(200));
        assert_eq!(response.status_text.as_deref(), Some("OK"));
        let names: Vec<&str> = response.headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["Content-Type", "X-Id"]);
        let body = response.body.unwrap();
        assert_eq!(
            (body.kind.as_str(), body.data.as_str()),
            ("json", r#"{"id":1}"#)
        );
    }

    #[test]
    fn binary_or_large_bodies_are_refused_and_an_empty_one_is_none() {
        let request = HttpRequest::new("Get", Method::Get, "/");
        let head = head(204, &[]);
        assert_eq!(
            example_from("x", &request, &head, &[0xff, 0xfe]),
            Err(ExampleError::Binary)
        );
        let large = vec![b'a'; EXAMPLE_LIMIT + 1];
        assert_eq!(
            example_from("x", &request, &head, &large),
            Err(ExampleError::TooLarge)
        );
        let empty = example_from("x", &request, &head, b"").unwrap();
        assert_eq!(empty.response.unwrap().body, None);
    }

    #[test]
    fn new_names_follow_the_status_and_are_never_taken() {
        let named = |name: &str| Example {
            name: name.into(),
            description: None,
            request: None,
            response: None,
        };
        assert_eq!(example_name(200, &[]), "200 OK");
        assert_eq!(
            example_name(200, &[named("200 OK"), named("200 OK (2)")]),
            "200 OK (3)"
        );
        assert_eq!(example_name(299, &[]), "299");
    }
}
