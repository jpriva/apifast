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
    /// AWS Signature Version 4: the request is signed with an access key.
    AwsV4(Box<AwsV4Auth>),
    /// A scheme of the file format that apifast cannot send yet, such as
    /// `digest` or `oauth2`, by its `type`. It is kept so that reading and
    /// saving a collection never loses it; sending a request with it fails.
    Unsupported(String),
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

/// AWS Signature Version 4 credentials and scope. Every field is a
/// template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsV4Auth {
    /// Access key id (`AKIA…`).
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
    /// Session token of temporary credentials; empty for none.
    pub session_token: String,
    /// Service name in the signature, such as `s3` or `execute-api`.
    pub service: String,
    /// Region, such as `us-east-1`.
    pub region: String,
    /// A profile of the AWS configuration files. Kept from Bruno's files;
    /// apifast does not read those files.
    pub profile_name: String,
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
