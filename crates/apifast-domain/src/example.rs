//! Response examples saved with a request (`examples` in its file, as
//! Bruno writes them): what was sent and what came back, kept to read
//! later or to answer from a mock.

use crate::{Body, Header, Method, Param};

/// A saved example of a request and its response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    /// Its name.
    pub name: String,
    /// What it shows, if said.
    pub description: Option<String>,
    /// The request that was sent.
    pub request: Option<ExampleRequest>,
    /// The response that came back.
    pub response: Option<ExampleResponse>,
}

/// The request of an example, as written (templates unrendered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleRequest {
    /// Method.
    pub method: Method,
    /// URL.
    pub url: String,
    /// Headers.
    pub headers: Vec<Header>,
    /// Query and path parameters.
    pub params: Vec<Param>,
    /// Body.
    pub body: Body,
}

/// The response of an example.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExampleResponse {
    /// Status code, if any.
    pub status: Option<u16>,
    /// Status text, such as `OK`, if any.
    pub status_text: Option<String>,
    /// Headers.
    pub headers: Vec<Header>,
    /// Body, if any.
    pub body: Option<ExampleBody>,
}

/// The body of an example's response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleBody {
    /// Its type, as Bruno writes it: `json`, `text`, `xml`, `html` or
    /// `binary`. Kept as written, so an unknown one survives a save.
    pub kind: String,
    /// Its contents.
    pub data: String,
}

impl ExampleBody {
    /// The type for a response of `content_type`: `json`, `xml`, `html`
    /// or `text`.
    pub fn kind_for(content_type: &str) -> &'static str {
        let media = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if media.ends_with("json") || media.ends_with("+json") {
            "json"
        } else if media.ends_with("html") {
            "html"
        } else if media.ends_with("xml") {
            "xml"
        } else {
            "text"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_type_follows_the_content_type() {
        assert_eq!(
            ExampleBody::kind_for("application/json; charset=utf-8"),
            "json"
        );
        assert_eq!(ExampleBody::kind_for("application/problem+json"), "json");
        assert_eq!(ExampleBody::kind_for("text/html"), "html");
        assert_eq!(ExampleBody::kind_for("application/xhtml+xml"), "xml");
        assert_eq!(ExampleBody::kind_for("text/xml"), "xml");
        assert_eq!(ExampleBody::kind_for("text/plain"), "text");
        assert_eq!(ExampleBody::kind_for(""), "text");
    }
}
