use crate::{Auth, Header, Settings, Variable};

/// Stable identifier of a collection, stored in `.apifast/collection.yml`.
///
/// It does not change when the folder moves. The vault uses it to match keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(String);

impl CollectionId {
    /// Wraps an existing identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Values a collection or folder defines for all of its requests
/// (the `request` block of `opencollection.yml` and `folder.yml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestDefaults {
    /// Headers added to every request.
    pub headers: Vec<Header>,
    /// Default authentication.
    pub auth: Auth,
    /// Variables of this scope.
    pub variables: Vec<Variable>,
    /// Send settings.
    pub settings: Settings,
}

/// Root of a collection (`opencollection.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// Identifier from `.apifast/`; `None` for a Bruno collection without that folder.
    pub id: Option<CollectionId>,
    /// Display name.
    pub name: String,
    /// Values inherited by every request.
    pub defaults: RequestDefaults,
    /// Its proxy.
    pub proxy: crate::CollectionProxy,
    /// The client certificates it presents, by host.
    pub client_certificates: Vec<crate::ClientCertificate>,
    /// Hosts sent to fixed addresses, from `.apifast/collection.yml`.
    pub dns_overrides: Vec<crate::DnsOverride>,
}

impl Collection {
    /// Collection with no identifier and no defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            defaults: RequestDefaults::default(),
            proxy: crate::CollectionProxy::default(),
            client_certificates: Vec::new(),
            dns_overrides: Vec::new(),
        }
    }
}

/// Folder of a collection (`folder.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Display name.
    pub name: String,
    /// Position among its siblings.
    pub seq: Option<u32>,
    /// Values inherited by the folder's requests.
    pub defaults: RequestDefaults,
}

impl Folder {
    /// Folder with no position and no defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            seq: None,
            defaults: RequestDefaults::default(),
        }
    }
}
