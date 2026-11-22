//! OpenAPI security as apifast auth, as Bruno maps it: the credentials are
//! variables to fill in (`{{token}}`, `{{apiKey}}`…), never values.

use apifast_app::ImportNote;
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, BasicAuth, BearerAuth, DigestAuth, OAuth2Auth,
    OAuth2Flow,
};
use serde_json::Value;

use super::schema::Spec;

/// The auth of a security requirement list (`security`): the first
/// scheme of its first requirement. An empty list is no auth. Why a
/// scheme is left out, if it is.
pub(crate) fn auth(spec: &Spec<'_>, requirements: &Value) -> Result<AuthScheme, ImportNote> {
    let Some(first) = requirements.as_array().and_then(|r| r.first()) else {
        return Ok(AuthScheme::None);
    };
    let Some((name, scopes)) = first.as_object().and_then(|o| o.iter().next()) else {
        // `security: [{}]` makes auth optional.
        return Ok(AuthScheme::None);
    };
    let scheme = spec
        .root
        .pointer(&format!("/components/securitySchemes/{}", escape(name)))
        .and_then(|s| spec.resolve(s))
        .ok_or_else(|| ImportNote::SecuritySchemeUndefined(name.to_owned()))?;
    let text = |key: &str| scheme.get(key).and_then(Value::as_str).unwrap_or_default();
    match text("type") {
        "http" => match text("scheme").to_ascii_lowercase().as_str() {
            "basic" => Ok(AuthScheme::Basic(BasicAuth {
                username: "{{username}}".into(),
                password: "{{password}}".into(),
            })),
            "bearer" => Ok(AuthScheme::Bearer(BearerAuth {
                token: "{{token}}".into(),
            })),
            "digest" => Ok(AuthScheme::Digest(DigestAuth {
                username: "{{username}}".into(),
                password: "{{password}}".into(),
            })),
            other => Err(ImportNote::HttpSecurityUnsupported(other.to_owned())),
        },
        "apiKey" => {
            let placement = match text("in") {
                "query" => ApiKeyPlacement::Query,
                "header" => ApiKeyPlacement::Header,
                other => return Err(ImportNote::ApiKeyInUnsupported(other.to_owned())),
            };
            Ok(AuthScheme::ApiKey(ApiKeyAuth {
                key: text("name").to_owned(),
                value: "{{apiKey}}".into(),
                placement,
            }))
        }
        "oauth2" => oauth2(scheme, scopes).map(|o| AuthScheme::OAuth2(Box::new(o))),
        other => Err(ImportNote::SecurityUnsupported(other.to_owned())),
    }
}

/// The first flow of an OAuth 2.0 scheme, with the scopes required.
fn oauth2(scheme: &Value, scopes: &Value) -> Result<OAuth2Auth, ImportNote> {
    let flows = scheme.get("flows").and_then(Value::as_object);
    let (name, flow) =
        (flows.and_then(|f| f.iter().next())).ok_or(ImportNote::OAuth2WithoutFlows)?;
    let text = |key: &str| {
        flow.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let scope: Vec<&str> = scopes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    Ok(OAuth2Auth {
        flow: match name.as_str() {
            "clientCredentials" => OAuth2Flow::ClientCredentials,
            "password" => OAuth2Flow::Password,
            "authorizationCode" => OAuth2Flow::AuthorizationCode,
            "implicit" => OAuth2Flow::Implicit,
            other => return Err(ImportNote::FlowUnsupported(other.to_owned())),
        },
        authorization_url: text("authorizationUrl"),
        access_token_url: text("tokenUrl"),
        refresh_token_url: text("refreshUrl"),
        client_id: "{{clientId}}".into(),
        client_secret: "{{clientSecret}}".into(),
        scope: scope.join(" "),
        // Authorization code is used with PKCE today.
        pkce: name == "authorizationCode",
        ..OAuth2Auth::default()
    })
}

/// A JSON pointer token: `~` and `/` escaped.
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
