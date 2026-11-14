/// Cuerpo de una request HTTP. Los textos son plantillas sin renderizar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Body {
    /// Sin cuerpo.
    #[default]
    None,
    /// JSON.
    Json(String),
    /// Texto plano.
    Text(String),
    /// XML.
    Xml(String),
    /// Consulta SPARQL.
    Sparql(String),
    /// `application/x-www-form-urlencoded`.
    FormUrlEncoded(Vec<FormField>),
    /// `multipart/form-data`.
    Multipart(Vec<MultipartPart>),
    /// Contenido de un archivo.
    File {
        /// Ruta relativa a la colección.
        path: String,
        /// `Content-Type` que se envía, si se indica.
        content_type: Option<String>,
    },
}

/// Campo de un formulario `form-urlencoded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    /// Nombre.
    pub name: String,
    /// Valor (plantilla).
    pub value: String,
    /// Si es `false`, no se envía.
    pub enabled: bool,
}

/// Parte de un formulario `multipart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartPart {
    /// Nombre de la parte.
    pub name: String,
    /// Contenido.
    pub value: MultipartValue,
    /// `Content-Type` de la parte, si se indica.
    pub content_type: Option<String>,
    /// Si es `false`, no se envía.
    pub enabled: bool,
}

/// Contenido de una parte `multipart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartValue {
    /// Texto (plantilla).
    Text(String),
    /// Archivo, por su ruta relativa a la colección.
    File(String),
}
