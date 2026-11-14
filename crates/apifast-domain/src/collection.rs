use crate::{Auth, Header, Settings, Variable};

/// Identificador estable de una colección, guardado en `.apifast/collection.yml`.
///
/// No cambia al mover la carpeta. Lo usa la bóveda para asociar claves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(String);

impl CollectionId {
    /// Envuelve un identificador ya existente.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// El identificador como texto.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Valores que una colección o carpeta define para todas sus requests
/// (el bloque `request` de `opencollection.yml` y `folder.yml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestDefaults {
    /// Headers que se añaden a cada request.
    pub headers: Vec<Header>,
    /// Autenticación por defecto.
    pub auth: Auth,
    /// Variables de este ámbito.
    pub variables: Vec<Variable>,
    /// Ajustes de envío.
    pub settings: Settings,
}

/// Raíz de una colección (`opencollection.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// Identificador de `.apifast/`; `None` en una colección de Bruno sin esa carpeta.
    pub id: Option<CollectionId>,
    /// Nombre visible.
    pub name: String,
    /// Valores heredados por todas las requests.
    pub defaults: RequestDefaults,
}

impl Collection {
    /// Colección sin identificador ni valores por defecto.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            defaults: RequestDefaults::default(),
        }
    }
}

/// Carpeta de una colección (`folder.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Nombre visible.
    pub name: String,
    /// Posición entre sus hermanos.
    pub seq: Option<u32>,
    /// Valores heredados por las requests de la carpeta.
    pub defaults: RequestDefaults,
}

impl Folder {
    /// Carpeta sin posición ni valores por defecto.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            seq: None,
            defaults: RequestDefaults::default(),
        }
    }
}
