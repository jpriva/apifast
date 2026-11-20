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
    /// Its scripts (`request.scripts`).
    pub scripts: Vec<crate::ScriptBlock>,
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
    /// Tags its requests carry, besides their own.
    pub tags: Vec<String>,
    /// Values inherited by the folder's requests.
    pub defaults: RequestDefaults,
}

impl Folder {
    /// Folder with no position, no tags and no defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            seq: None,
            tags: Vec::new(),
            defaults: RequestDefaults::default(),
        }
    }
}

/// The tags of a request in `folders` (from the outermost): its own first,
/// then those of its folders from the outermost, each once (in any case),
/// as in Bruno.
pub fn effective_tags(own: &[String], folders: &[Folder]) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in own.iter().chain(folders.iter().flat_map(|f| &f.tags)) {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            tags.push(tag.clone());
        }
    }
    tags
}

#[cfg(test)]
mod tag_tests {
    use super::*;

    #[test]
    fn a_request_carries_its_own_tags_then_those_of_its_folders_once() {
        let folder = |tags: &[&str]| Folder {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..Folder::new("f")
        };
        let folders = [
            folder(&["req-api"]),
            folder(&["nested"]),
            folder(&["deep", "Own-Tag"]),
        ];
        let own = ["nested".to_owned(), "own-tag".to_owned()];
        assert_eq!(
            effective_tags(&own, &folders),
            ["nested", "own-tag", "req-api", "deep"]
        );
        assert_eq!(effective_tags(&[], &folders[..1]), ["req-api"]);
    }
}
