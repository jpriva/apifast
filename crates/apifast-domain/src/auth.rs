use crate::Setting;

/// Authentication of a collection, folder or request.
///
/// `Setting::Inherit` is the file format's `inherit` auth: the closest parent's
/// auth is used. If nobody sets one, it resolves to [`AuthScheme::None`].
pub type Auth = Setting<AuthScheme>;

/// A concrete authentication scheme.
///
/// Sensitive fields never hold the secret itself, only a template that
/// references it, such as `{{access_token}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthScheme {
    /// No authentication.
    None,
    /// HTTP Basic.
    Basic(BasicAuth),
    /// Bearer token in the `Authorization` header.
    Bearer(BearerAuth),
    /// Key sent in a header or in the query.
    ApiKey(ApiKeyAuth),
}

/// HTTP Basic credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicAuth {
    /// User name (template).
    pub username: String,
    /// Password (template).
    pub password: String,
}

/// Bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerAuth {
    /// Token (template).
    pub token: String,
}

/// API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyAuth {
    /// Header or parameter name.
    pub key: String,
    /// Value (template).
    pub value: String,
    /// Where it is sent.
    pub placement: ApiKeyPlacement,
}

/// Where an API key is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiKeyPlacement {
    /// In a header.
    #[default]
    Header,
    /// In a query parameter.
    Query,
}
