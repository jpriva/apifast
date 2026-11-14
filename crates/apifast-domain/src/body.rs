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
    /// Contents of a file.
    File {
        /// Path relative to the collection.
        path: String,
        /// `Content-Type` to send, if given.
        content_type: Option<String>,
    },
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
