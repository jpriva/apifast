use crate::{Resolved, Setting};

/// Ajustes de envío heredables (colección → carpeta → request).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// Tiempo máximo en milisegundos; 0 es sin límite.
    pub timeout_ms: Setting<u64>,
    /// Si se siguen las redirecciones.
    pub follow_redirects: Setting<bool>,
    /// Máximo de redirecciones seguidas.
    pub max_redirects: Setting<u32>,
    /// Si se codifica la URL antes de enviarla.
    pub encode_url: Setting<bool>,
}

impl Settings {
    /// Tiempo máximo por defecto: 30 segundos.
    pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
    /// Se siguen las redirecciones por defecto.
    pub const DEFAULT_FOLLOW_REDIRECTS: bool = true;
    /// Máximo de redirecciones por defecto.
    pub const DEFAULT_MAX_REDIRECTS: u32 = 5;
    /// La URL se codifica por defecto.
    pub const DEFAULT_ENCODE_URL: bool = true;
}

/// Ajustes resueltos, cada uno con su procedencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSettings {
    /// Tiempo máximo en milisegundos; 0 es sin límite.
    pub timeout_ms: Resolved<u64>,
    /// Si se siguen las redirecciones.
    pub follow_redirects: Resolved<bool>,
    /// Máximo de redirecciones seguidas.
    pub max_redirects: Resolved<u32>,
    /// Si se codifica la URL antes de enviarla.
    pub encode_url: Resolved<bool>,
}
