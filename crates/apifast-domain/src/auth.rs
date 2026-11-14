use crate::Setting;

/// Autenticación de una colección, carpeta o request.
///
/// `Setting::Inherit` es la auth `inherit` del formato: se toma la del padre más
/// cercano. Si nadie la define, se resuelve como [`AuthScheme::None`].
pub type Auth = Setting<AuthScheme>;

/// Esquema de autenticación concreto.
///
/// Los campos sensibles no guardan el secreto, sino una plantilla que lo
/// referencia, por ejemplo `{{access_token}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthScheme {
    /// Sin autenticación.
    None,
    /// HTTP Basic.
    Basic(BasicAuth),
    /// Token Bearer en el header `Authorization`.
    Bearer(BearerAuth),
    /// Clave en un header o en la query.
    ApiKey(ApiKeyAuth),
}

/// Credenciales de HTTP Basic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicAuth {
    /// Usuario (plantilla).
    pub username: String,
    /// Contraseña (plantilla).
    pub password: String,
}

/// Token Bearer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerAuth {
    /// Token (plantilla).
    pub token: String,
}

/// Clave de API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyAuth {
    /// Nombre del header o del parámetro.
    pub key: String,
    /// Valor (plantilla).
    pub value: String,
    /// Dónde se envía.
    pub placement: ApiKeyPlacement,
}

/// Dónde se envía una clave de API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiKeyPlacement {
    /// En un header.
    #[default]
    Header,
    /// En un parámetro de la query.
    Query,
}
