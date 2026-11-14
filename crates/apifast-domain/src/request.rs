use crate::{Auth, Body, Settings, Variable};

/// Request de una colección, según su protocolo.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// Request HTTP.
    Http(HttpRequest),
}

/// Request HTTP. Los textos son plantillas sin renderizar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Nombre visible (`info.name`).
    pub name: String,
    /// Posición entre sus hermanos.
    pub seq: Option<u32>,
    /// Etiquetas.
    pub tags: Vec<String>,
    /// Método.
    pub method: Method,
    /// URL.
    pub url: String,
    /// Parámetros de query y de ruta.
    pub params: Vec<Param>,
    /// Headers propios.
    pub headers: Vec<Header>,
    /// Cuerpo.
    pub body: Body,
    /// Autenticación.
    pub auth: Auth,
    /// Variables de la request (`runtime.variables`).
    pub variables: Vec<Variable>,
    /// Ajustes de envío.
    pub settings: Settings,
    /// Documentación en Markdown.
    pub docs: Option<String>,
}

impl HttpRequest {
    /// Request sin headers, body ni ajustes propios, con auth heredada.
    pub fn new(name: impl Into<String>, method: Method, url: impl Into<String>) -> Self {
        Self {
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
            settings: Settings::default(),
            docs: None,
        }
    }
}

/// Método HTTP.
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
    /// Método propio; se envía tal cual.
    Custom(String),
}

impl Method {
    /// Interpreta un método. Los estándar se reconocen sin distinguir mayúsculas;
    /// cualquier otro se conserva tal cual, porque los métodos propios distinguen
    /// mayúsculas.
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

    /// El método tal como se envía.
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

/// Header HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Nombre.
    pub name: String,
    /// Valor (plantilla).
    pub value: String,
    /// Si es `false`, no se envía.
    pub enabled: bool,
}

impl Header {
    /// Header habilitado.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }
}

/// Parámetro de query o de ruta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Nombre.
    pub name: String,
    /// Valor (plantilla).
    pub value: String,
    /// Query (`?a=1`) o ruta (`/:id`).
    pub kind: ParamKind,
    /// Si es `false`, no se envía.
    pub enabled: bool,
}

/// Tipo de parámetro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Parámetro de la query.
    Query,
    /// Segmento `:nombre` de la ruta.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_metodos_estandar_se_normalizan_y_los_propios_se_conservan() {
        assert_eq!(Method::parse("get"), Method::Get);
        assert_eq!(Method::parse("Post").as_str(), "POST");
        assert_eq!(Method::parse("purge"), Method::Custom("purge".into()));
        assert_eq!(Method::parse("purge").as_str(), "purge");
    }
}
