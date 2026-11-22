//! Postman's `auth` as apifast's, as Bruno maps it
//! (`bruno-converters/src/postman/postman-to-bruno.js`). v2.1 keeps each
//! field as `{key, value}` in a list; v2.0 as an object.

use apifast_app::ImportNote;
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth,
    CredentialsPlacement, DigestAuth, NtlmAuth, OAuth2Auth, OAuth2Flow, OAuth2Parameter,
    OAuth2Request, ParameterPlacement, Setting, TokenPlacement,
};
use serde_json::Value;

/// The fields of one auth type, whatever the version.
struct Fields<'a>(Option<&'a Value>);

impl Fields<'_> {
    fn get(&self, key: &str) -> Option<&Value> {
        match self.0? {
            Value::Array(items) => items
                .iter()
                .find(|item| item.get("key").and_then(Value::as_str) == Some(key))
                .and_then(|item| item.get("value")),
            object => object.get(key),
        }
    }

    fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    }
}

/// The auth `value` describes, or why it is left out. `None` (no `auth`)
/// inherits, as in Postman.
pub(crate) fn auth(value: Option<&Value>) -> Result<Auth, ImportNote> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Setting::Inherit);
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("noauth");
    let fields = Fields(value.get(kind));
    let scheme = match kind {
        "noauth" => AuthScheme::None,
        "inherit" => return Ok(Setting::Inherit),
        "basic" => AuthScheme::Basic(BasicAuth {
            username: fields.text("username"),
            password: fields.text("password"),
        }),
        "bearer" => AuthScheme::Bearer(BearerAuth {
            token: fields.text("token"),
        }),
        "digest" => AuthScheme::Digest(DigestAuth {
            username: fields.text("username"),
            password: fields.text("password"),
        }),
        "ntlm" => AuthScheme::Ntlm(NtlmAuth {
            username: fields.text("username"),
            password: fields.text("password"),
            domain: fields.text("domain"),
        }),
        "apikey" => AuthScheme::ApiKey(ApiKeyAuth {
            key: fields.text("key"),
            value: fields.text("value"),
            placement: if fields.text("in") == "query" {
                ApiKeyPlacement::Query
            } else {
                ApiKeyPlacement::Header
            },
        }),
        "awsv4" => AuthScheme::AwsV4(Box::new(AwsV4Auth {
            access_key_id: fields.text("accessKey"),
            secret_access_key: fields.text("secretKey"),
            session_token: fields.text("sessionToken"),
            service: fields.text("service"),
            region: fields.text("region"),
            profile_name: String::new(),
        })),
        "oauth2" => AuthScheme::OAuth2(Box::new(oauth2(&fields)?)),
        other => {
            return Err(ImportNote::AuthUnsupported(other.to_owned()));
        }
    };
    Ok(Setting::Set(scheme))
}

fn oauth2(fields: &Fields<'_>) -> Result<OAuth2Auth, ImportNote> {
    let grant = fields.text("grant_type");
    let flow = match grant.as_str() {
        "authorization_code" | "authorization_code_with_pkce" => OAuth2Flow::AuthorizationCode,
        "password_credentials" => OAuth2Flow::Password,
        "implicit" => OAuth2Flow::Implicit,
        "" | "client_credentials" => OAuth2Flow::ClientCredentials,
        other => return Err(ImportNote::GrantUnsupported(other.to_owned())),
    };
    let token_placement = if fields.text("addTokenTo") == "queryParams" {
        TokenPlacement::Query("access_token".into())
    } else {
        let prefix = match fields.get("headerPrefix") {
            Some(Value::String(prefix)) => prefix.clone(),
            _ => "Bearer".into(),
        };
        TokenPlacement::Header(prefix)
    };
    let mut parameters = Vec::new();
    for (key, request) in [
        ("authRequestParams", OAuth2Request::Authorization),
        ("tokenRequestParams", OAuth2Request::Token),
        ("refreshRequestParams", OAuth2Request::Refresh),
    ] {
        let list = fields
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for param in list
            .iter()
            .filter(|p| p.get("enabled") != Some(&Value::Bool(false)))
        {
            let text = |k: &str| param.get(k).and_then(Value::as_str).unwrap_or_default();
            parameters.push(OAuth2Parameter {
                request,
                name: text("key").to_owned(),
                value: text("value").to_owned(),
                placement: match text("send_as") {
                    "request_url" => ParameterPlacement::Query,
                    "request_body" => ParameterPlacement::Body,
                    _ => ParameterPlacement::Header,
                },
            });
        }
    }
    let token_id = fields.text("tokenName");
    Ok(OAuth2Auth {
        flow,
        authorization_url: fields.text("authUrl"),
        access_token_url: fields.text("accessTokenUrl"),
        refresh_token_url: fields.text("refreshTokenUrl"),
        callback_url: fields.text("redirect_uri"),
        client_id: fields.text("clientId"),
        client_secret: fields.text("clientSecret"),
        credentials_placement: if fields.text("client_authentication") == "body" {
            CredentialsPlacement::Body
        } else {
            CredentialsPlacement::BasicAuthHeader
        },
        username: fields.text("username"),
        password: fields.text("password"),
        scope: fields.text("scope"),
        state: fields.text("state"),
        pkce: grant == "authorization_code_with_pkce",
        token_placement,
        parameters,
        ..OAuth2Auth {
            token_id: if token_id.is_empty() {
                OAuth2Auth::default().token_id
            } else {
                token_id
            },
            ..OAuth2Auth::default()
        }
    })
}
