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
    /// HTTP Digest: the credentials answer the server's challenge.
    Digest(DigestAuth),
    /// NTLM (version 2): a handshake on the connection, for Windows servers.
    Ntlm(NtlmAuth),
    /// AWS Signature Version 4: the request is signed with an access key.
    AwsV4(Box<AwsV4Auth>),
    /// OAuth 2.0: a token obtained from an authorization server.
    OAuth2(Box<OAuth2Auth>),
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

/// HTTP Digest credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DigestAuth {
    /// User name (template).
    pub username: String,
    /// Password (template).
    pub password: String,
}

/// NTLM credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NtlmAuth {
    /// User name (template); `DOMAIN\user` also gives the domain.
    pub username: String,
    /// Password (template).
    pub password: String,
    /// Windows domain (template); may be empty.
    pub domain: String,
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

/// OAuth 2.0, as Bruno's files describe it. Texts are templates; empty
/// means not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth2Auth {
    /// How the token is obtained.
    pub flow: OAuth2Flow,
    /// Where the user authorizes (authorization code and implicit flows).
    pub authorization_url: String,
    /// Where tokens are requested.
    pub access_token_url: String,
    /// Where tokens are refreshed; the token URL when empty.
    pub refresh_token_url: String,
    /// Where the authorization server sends the user back.
    pub callback_url: String,
    /// Client id.
    pub client_id: String,
    /// Client secret.
    pub client_secret: String,
    /// How the client credentials are sent.
    pub credentials_placement: CredentialsPlacement,
    /// Resource owner's user name (password flow).
    pub username: String,
    /// Resource owner's password (password flow).
    pub password: String,
    /// Scopes, separated by spaces.
    pub scope: String,
    /// The `state` of the authorization request.
    pub state: String,
    /// Proof Key for Code Exchange, in the authorization code flow.
    pub pkce: bool,
    /// Names the token in the cache, so several auths can share one.
    pub token_id: String,
    /// Where the token goes in the request.
    pub token_placement: TokenPlacement,
    /// Which token of the response is sent: `access_token` or `id_token`.
    pub token_source: String,
    /// Get a token before sending when there is none.
    pub auto_fetch: bool,
    /// Refresh an expired token before sending.
    pub auto_refresh: bool,
    /// Extra parameters of the authorization, token and refresh requests.
    pub parameters: Vec<OAuth2Parameter>,
}

impl Default for OAuth2Auth {
    fn default() -> Self {
        Self {
            flow: OAuth2Flow::ClientCredentials,
            authorization_url: String::new(),
            access_token_url: String::new(),
            refresh_token_url: String::new(),
            callback_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            credentials_placement: CredentialsPlacement::Body,
            username: String::new(),
            password: String::new(),
            scope: String::new(),
            state: String::new(),
            pkce: false,
            token_id: "credentials".into(),
            token_placement: TokenPlacement::Header("Bearer".into()),
            token_source: "access_token".into(),
            auto_fetch: true,
            auto_refresh: false,
            parameters: Vec::new(),
        }
    }
}

/// An OAuth 2.0 grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuth2Flow {
    /// `client_credentials`: the client authenticates as itself.
    ClientCredentials,
    /// `resource_owner_password_credentials`: the user's name and password.
    Password,
    /// `authorization_code`: the user authorizes in a browser.
    AuthorizationCode,
    /// `implicit`: kept from the files; deprecated and not sent.
    Implicit,
}

impl OAuth2Flow {
    /// Its name in the files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientCredentials => "client_credentials",
            Self::Password => "resource_owner_password_credentials",
            Self::AuthorizationCode => "authorization_code",
            Self::Implicit => "implicit",
        }
    }

    /// The flow named `name` in a file; client credentials when unknown.
    pub fn parse(name: &str) -> Self {
        match name {
            "resource_owner_password_credentials" | "password" => Self::Password,
            "authorization_code" => Self::AuthorizationCode,
            "implicit" => Self::Implicit,
            _ => Self::ClientCredentials,
        }
    }
}

/// How the client id and secret travel to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CredentialsPlacement {
    /// As form fields of the body.
    #[default]
    Body,
    /// As HTTP Basic credentials.
    BasicAuthHeader,
}

/// Where the token goes in the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenPlacement {
    /// `Authorization: <prefix> <token>`; just the token with no prefix.
    Header(String),
    /// A query parameter with this name.
    Query(String),
}

/// Which request an extra parameter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuth2Request {
    /// The authorization request, in the browser.
    Authorization,
    /// The token request.
    Token,
    /// The refresh request.
    Refresh,
}

/// Where an extra parameter goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterPlacement {
    /// A header.
    Header,
    /// A query parameter.
    Query,
    /// A form field of the body.
    Body,
}

/// An extra parameter of an OAuth 2.0 request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth2Parameter {
    /// The request it goes in.
    pub request: OAuth2Request,
    /// Name.
    pub name: String,
    /// Value (template).
    pub value: String,
    /// Where it goes.
    pub placement: ParameterPlacement,
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
