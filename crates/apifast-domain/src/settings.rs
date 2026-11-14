use crate::{Resolved, Setting};

/// Inheritable send settings (collection → folder → request).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// Timeout in milliseconds; 0 means no limit.
    pub timeout_ms: Setting<u64>,
    /// Whether redirects are followed.
    pub follow_redirects: Setting<bool>,
    /// Maximum number of redirects followed.
    pub max_redirects: Setting<u32>,
    /// Whether the URL is encoded before sending.
    pub encode_url: Setting<bool>,
}

impl Settings {
    /// Default timeout: 30 seconds.
    pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
    /// Redirects are followed by default.
    pub const DEFAULT_FOLLOW_REDIRECTS: bool = true;
    /// Default maximum number of redirects.
    pub const DEFAULT_MAX_REDIRECTS: u32 = 5;
    /// The URL is encoded by default.
    pub const DEFAULT_ENCODE_URL: bool = true;
}

/// Resolved settings, each with its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSettings {
    /// Timeout in milliseconds; 0 means no limit.
    pub timeout_ms: Resolved<u64>,
    /// Whether redirects are followed.
    pub follow_redirects: Resolved<bool>,
    /// Maximum number of redirects followed.
    pub max_redirects: Resolved<u32>,
    /// Whether the URL is encoded before sending.
    pub encode_url: Resolved<bool>,
}
