/// Body of an HTTP request. Texts are unrendered templates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Body {
    /// No body.
    #[default]
    None,
    /// JSON.
    Json(String),
    /// Plain text.
    Text(String),
    /// XML.
    Xml(String),
    /// SPARQL query.
    Sparql(String),
    /// `application/x-www-form-urlencoded`.
    FormUrlEncoded(Vec<FormField>),
    /// `multipart/form-data`.
    Multipart(Vec<MultipartPart>),
    /// A GraphQL operation, sent as JSON (`{"query": …, "variables": …}`).
    /// A request with it is a `graphql` request in its file.
    GraphQl {
        /// The query, mutation or subscription (template).
        query: String,
        /// Its variables, as JSON text (template); may be empty.
        variables: String,
    },
    /// The messages of a WebSocket connection, ready to send once it is
    /// open. A request with it is a `websocket` request in its file: the
    /// URL, headers and auth are those of the handshake.
    WebSocket(Vec<WsMessage>),
    /// A gRPC call: the method and the messages ready to send. A request
    /// with it is a `grpc` request in its file: the URL is the server's,
    /// the headers are the call's metadata, and the auth goes in them.
    Grpc(GrpcCall),
    /// Contents of a file.
    File {
        /// Path relative to the collection.
        path: String,
        /// `Content-Type` to send, if given.
        content_type: Option<String>,
    },
}

impl Body {
    /// Whether the request is a WebSocket connection.
    pub fn is_websocket(&self) -> bool {
        matches!(self, Self::WebSocket(_))
    }

    /// Whether the request is a gRPC call.
    pub fn is_grpc(&self) -> bool {
        matches!(self, Self::Grpc(_))
    }
}

/// What a gRPC request calls, and the messages it has ready to send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrpcCall {
    /// The method, as `/package.Service/Method`; empty until one is picked.
    pub method: String,
    /// How the method streams, when known.
    pub method_type: Option<GrpcMethodType>,
    /// The `.proto` file that describes the service, relative to the
    /// collection's folder, as in Bruno; `None` to ask the server
    /// (reflection).
    pub proto_path: Option<String>,
    /// The messages, as JSON (templates).
    pub messages: Vec<GrpcMessage>,
}

/// A message a gRPC request has ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcMessage {
    /// Its name; empty for the only message of a request.
    pub name: String,
    /// The message as JSON (template).
    pub data: String,
}

impl GrpcMessage {
    /// A message named `name` with `data`.
    pub fn new(name: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data: data.into(),
        }
    }
}

/// How a gRPC method streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrpcMethodType {
    /// One message each way.
    Unary,
    /// Many messages from the client, one answer.
    ClientStreaming,
    /// One message, many answers.
    ServerStreaming,
    /// Many messages both ways.
    BidiStreaming,
}

impl GrpcMethodType {
    /// Its name in a file, as Bruno writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unary => "unary",
            Self::ClientStreaming => "client-streaming",
            Self::ServerStreaming => "server-streaming",
            Self::BidiStreaming => "bidi-streaming",
        }
    }

    /// The type named `name`; `None` for any other.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "unary" => Some(Self::Unary),
            "client-streaming" => Some(Self::ClientStreaming),
            "server-streaming" => Some(Self::ServerStreaming),
            "bidi-streaming" => Some(Self::BidiStreaming),
            _ => None,
        }
    }

    /// Whether the client sends more than one message.
    pub fn client_streams(self) -> bool {
        matches!(self, Self::ClientStreaming | Self::BidiStreaming)
    }

    /// Whether the server answers with more than one message.
    pub fn server_streams(self) -> bool {
        matches!(self, Self::ServerStreaming | Self::BidiStreaming)
    }
}

/// A message a WebSocket request has ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsMessage {
    /// Its name; empty for the only message of a request.
    pub name: String,
    /// How its text is edited and checked.
    pub kind: WsMessageKind,
    /// The text sent (template).
    pub data: String,
    /// Whether it is the one chosen to send.
    pub selected: bool,
}

impl WsMessage {
    /// A message named `name`, of `kind`, with `data`, not selected.
    pub fn new(name: impl Into<String>, kind: WsMessageKind, data: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind,
            data: data.into(),
            selected: false,
        }
    }
}

/// The kind of a WebSocket message, as Bruno names it in files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum WsMessageKind {
    /// JSON text.
    #[default]
    Json,
    /// XML text.
    Xml,
    /// Plain text.
    Text,
}

impl WsMessageKind {
    /// Its name in a file: `json`, `xml` or `text`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
            Self::Text => "text",
        }
    }

    /// The kind named `name`; text, as Bruno reads it, for any other.
    pub fn parse(name: &str) -> Self {
        match name {
            "json" => Self::Json,
            "xml" => Self::Xml,
            _ => Self::Text,
        }
    }
}

/// Field of a `form-urlencoded` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    /// Name.
    pub name: String,
    /// Value (template).
    pub value: String,
    /// When `false`, it is not sent.
    pub enabled: bool,
}

/// Part of a `multipart` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartPart {
    /// Part name.
    pub name: String,
    /// Contents.
    pub value: MultipartValue,
    /// `Content-Type` of the part, if given.
    pub content_type: Option<String>,
    /// When `false`, it is not sent.
    pub enabled: bool,
}

/// Contents of a `multipart` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartValue {
    /// Text (template).
    Text(String),
    /// One or more files, by their paths relative to the collection.
    Files(Vec<String>),
}
