use crate::{Assertion, Auth, Body, Capture, Settings, Variable};

/// A request of a collection, by protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// HTTP request.
    Http(HttpRequest),
}

/// What a request is, by its body: what its file calls it and how it goes
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RequestKind {
    /// An HTTP request.
    #[default]
    Http,
    /// HTTP with a GraphQL operation as the body.
    GraphQl,
    /// A WebSocket connection, with messages to send.
    WebSocket,
    /// A gRPC call, with messages to send.
    Grpc,
}

impl RequestKind {
    /// Its `info.type` in a file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::GraphQl => "graphql",
            Self::WebSocket => "websocket",
            Self::Grpc => "grpc",
        }
    }
}

/// HTTP request. Texts are unrendered templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Display name (`info.name`).
    pub name: String,
    /// Position among its siblings.
    pub seq: Option<u32>,
    /// Tags.
    pub tags: Vec<String>,
    /// Method.
    pub method: Method,
    /// URL.
    pub url: String,
    /// Query and path parameters.
    pub params: Vec<Param>,
    /// Own headers.
    pub headers: Vec<Header>,
    /// Body.
    pub body: Body,
    /// Authentication.
    pub auth: Auth,
    /// Request variables (`runtime.variables`).
    pub variables: Vec<Variable>,
    /// Checks on the response (`runtime.assertions`).
    pub assertions: Vec<Assertion>,
    /// Values taken from the response into variables (`runtime.actions`).
    pub captures: Vec<Capture>,
    /// Send settings.
    pub settings: Settings,
    /// Markdown documentation.
    pub docs: Option<String>,
    /// Saved examples of its responses.
    pub examples: Vec<crate::Example>,
    /// Its scripts (`runtime.scripts`).
    pub scripts: Vec<crate::ScriptBlock>,
}

impl HttpRequest {
    /// What it is, by its body.
    pub fn kind(&self) -> RequestKind {
        match self.body {
            crate::Body::GraphQl { .. } => RequestKind::GraphQl,
            crate::Body::WebSocket(_) => RequestKind::WebSocket,
            crate::Body::Grpc(_) => RequestKind::Grpc,
            _ => RequestKind::Http,
        }
    }

    /// Request with no own headers, body or settings, and inherited auth.
    pub fn new(name: impl Into<String>, method: Method, url: impl Into<String>) -> Self {
        Self {
            scripts: Vec::new(),
            name: name.into(),
            seq: None,
            tags: Vec::new(),
            method,
            url: url.into(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            assertions: Vec::new(),
            captures: Vec::new(),
            settings: Settings::default(),
            docs: None,
            examples: Vec::new(),
        }
    }
}

/// HTTP method.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET`
    Get,
    /// `POST`
    Post,
    /// `PUT`
    Put,
    /// `PATCH`
    Patch,
    /// `DELETE`
    Delete,
    /// `HEAD`
    Head,
    /// `OPTIONS`
    Options,
    /// `TRACE`
    Trace,
    /// `CONNECT`
    Connect,
    /// A custom method, sent as is.
    Custom(String),
}

impl Method {
    /// Parses a method. Standard methods are matched case-insensitively; any
    /// other is kept as is, because custom methods are case-sensitive.
    pub fn parse(method: &str) -> Self {
        match method.to_ascii_uppercase().as_str() {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            "TRACE" => Self::Trace,
            "CONNECT" => Self::Connect,
            _ => Self::Custom(method.to_owned()),
        }
    }

    /// The method as sent on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
            Self::Connect => "CONNECT",
            Self::Custom(method) => method,
        }
    }
}

/// HTTP header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Name.
    pub name: String,
    /// Value (template).
    pub value: String,
    /// When `false`, it is not sent.
    pub enabled: bool,
}

impl Header {
    /// Enabled header.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }
}

/// Query or path parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Name.
    pub name: String,
    /// Value (template).
    pub value: String,
    /// Query (`?a=1`) or path (`/:id`).
    pub kind: ParamKind,
    /// When `false`, it is not sent.
    pub enabled: bool,
}

/// Kind of parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Query parameter.
    Query,
    /// `:name` path segment.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_methods_are_normalized_and_custom_ones_kept() {
        assert_eq!(Method::parse("get"), Method::Get);
        assert_eq!(Method::parse("Post").as_str(), "POST");
        assert_eq!(Method::parse("purge"), Method::Custom("purge".into()));
        assert_eq!(Method::parse("purge").as_str(), "purge");
    }
}
