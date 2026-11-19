//! Importing a collection from another tool (docs/12, F6): an importer
//! reads its file into an [`ImportedCollection`]; [`plan_import`] says what
//! would be created, and what was not kept, for the user to review;
//! [`apply_import`] writes it. The plan and the writing walk the tree the
//! same way, so what is written is what the plan showed.

use std::collections::BTreeSet;

use apifast_domain::{Environment, Folder, HttpRequest, RequestDefaults};

use crate::organize::slug;
use crate::{AppError, CollectionRepository, EnvironmentRepository, RequestPath};

/// A collection read from another tool's file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedCollection {
    /// Its name.
    pub name: String,
    /// What every request inherits: headers, auth, variables.
    pub defaults: RequestDefaults,
    /// Its folders and requests, in order.
    pub items: Vec<ImportedItem>,
    /// Its environments.
    pub environments: Vec<Environment>,
    /// What could not be kept as it was.
    pub warnings: Vec<ImportWarning>,
}

/// A folder or a request of an imported collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedItem {
    /// A folder and what it holds, in order.
    Folder {
        /// The folder: name and what its requests inherit.
        folder: Folder,
        /// Its folders and requests.
        items: Vec<ImportedItem>,
    },
    /// A request, and what identifies it in its source across imports:
    /// an id, or its operation, or its place. `None` when the source has
    /// nothing stable, as a cURL command.
    Request {
        /// The request.
        request: Box<HttpRequest>,
        /// Its key in the source.
        key: Option<String>,
    },
}

/// Something an importer could not keep as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    /// Where: the folders and the request, as `Users / Get user`; empty for
    /// the collection.
    pub at: String,
    /// What happened to it.
    pub message: String,
}

impl ImportWarning {
    /// A warning about `at`.
    pub fn new(at: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            at: at.into(),
            message: message.into(),
        }
    }
}

/// What importing would create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    /// The collection's name.
    pub collection: String,
    /// Each folder and request, in the order they are written.
    pub entries: Vec<PlannedEntry>,
    /// The environments, by name.
    pub environments: Vec<String>,
    /// What could not be kept as it was.
    pub warnings: Vec<ImportWarning>,
}

impl ImportPlan {
    /// How many folders and requests it creates.
    pub fn counts(&self) -> (usize, usize) {
        let folders = self.entries.iter().filter(|e| e.is_folder).count();
        (folders, self.entries.len() - folders)
    }
}

/// A folder or request the import writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEntry {
    /// Its path in the collection: `users` or `users/get-user.yml`.
    pub path: String,
    /// Its name.
    pub name: String,
    /// Whether it is a folder.
    pub is_folder: bool,
}

/// What would be written for `imported`.
pub fn plan_import(imported: &ImportedCollection) -> ImportPlan {
    let mut entries = Vec::new();
    walk_planned(&imported.items, "", &mut |entry, _| entries.push(entry));
    ImportPlan {
        collection: imported.name.clone(),
        entries,
        environments: imported
            .environments
            .iter()
            .map(|e| e.name.clone())
            .collect(),
        warnings: imported.warnings.clone(),
    }
}

/// What [`apply_import`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    /// Folders created.
    pub folders: usize,
    /// Requests created.
    pub requests: usize,
    /// Environments created.
    pub environments: usize,
}

/// Writes `imported` into the collection of `collections`, which should be
/// new: its name and defaults, then each folder and request where
/// [`plan_import`] put them, then the environments. Records what it wrote
/// as coming from `source`, to import it again later
/// ([`crate::plan_reimport`]).
///
/// # Errors
///
/// On the first file that cannot be written; what was written before
/// stays.
pub fn apply_import(
    (collections, environments): (
        &(impl CollectionRepository + crate::ImportRecords),
        &impl EnvironmentRepository,
    ),
    imported: &ImportedCollection,
    source: &str,
) -> Result<ImportReport, AppError> {
    let mut collection = collections.collection()?;
    collection.name.clone_from(&imported.name);
    collection.defaults = imported.defaults.clone();
    collections.save_collection(&collection)?;

    let mut report = ImportReport {
        folders: 0,
        requests: 0,
        environments: 0,
    };
    let mut failure = None;
    let mut record = crate::ImportRecord {
        source: source.to_owned(),
        requests: Vec::new(),
    };
    walk_planned(&imported.items, "", &mut |entry, item| {
        if failure.is_some() {
            return;
        }
        let written = match item {
            ImportedItem::Folder { folder, .. } => {
                report.folders += 1;
                collections.save_folder(&entry.path, folder)
            }
            ImportedItem::Request { request, key } => {
                report.requests += 1;
                record.requests.push(crate::RecordedRequest {
                    key: key.clone().unwrap_or_else(|| entry.path.clone()),
                    path: entry.path.clone(),
                    hash: crate::reimport::request_hash(request),
                });
                collections.save_request(&RequestPath::new(entry.path.clone()), request)
            }
        };
        failure = written.err();
    });
    if let Some(error) = failure {
        return Err(error.into());
    }
    for environment in &imported.environments {
        environments.save_environment(environment)?;
        report.environments += 1;
    }
    collections.save_import_record(&record)?;
    Ok(report)
}

/// Calls `visit` on each folder (before what it holds) and request, with
/// its path: the slug of its name, made unique among its siblings, and its
/// position (`seq`) as the order in the source.
pub(crate) fn walk_planned<'a>(
    items: &'a [ImportedItem],
    parent: &str,
    visit: &mut dyn FnMut(PlannedEntry, &ImportedItem),
) {
    let mut taken = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let seq = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let (name, base, extension) = match item {
            ImportedItem::Folder { folder, .. } => (&folder.name, slug(&folder.name, "folder"), ""),
            ImportedItem::Request { request, .. } => {
                (&request.name, slug(&request.name, "request"), ".yml")
            }
        };
        let mut file = format!("{base}{extension}");
        let mut n = 2;
        while !taken.insert(file.clone()) {
            file = format!("{base}-{n}{extension}");
            n += 1;
        }
        let path = if parent.is_empty() {
            file
        } else {
            format!("{parent}/{file}")
        };
        let entry = PlannedEntry {
            path: path.clone(),
            name: name.clone(),
            is_folder: matches!(item, ImportedItem::Folder { .. }),
        };
        match item {
            ImportedItem::Folder { folder, items } => {
                let positioned = ImportedItem::Folder {
                    folder: Folder {
                        seq: Some(seq),
                        ..folder.clone()
                    },
                    items: Vec::new(),
                };
                visit(entry, &positioned);
                walk_planned(items, &path, visit);
            }
            ImportedItem::Request { request, key } => {
                let positioned = ImportedItem::Request {
                    request: Box::new(HttpRequest {
                        seq: Some(seq),
                        ..request.as_ref().clone()
                    }),
                    key: key.clone(),
                };
                visit(entry, &positioned);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Collection, Method};

    use super::*;
    use crate::memory::{MemoryCollections, MemoryEnvironments};

    fn request(name: &str) -> ImportedItem {
        ImportedItem::Request {
            request: Box::new(HttpRequest::new(name, Method::Get, "/")),
            key: Some(name.to_owned()),
        }
    }

    fn sample() -> ImportedCollection {
        ImportedCollection {
            name: "Shop".into(),
            items: vec![
                ImportedItem::Folder {
                    folder: Folder::new("Users"),
                    items: vec![request("Get user"), request("Get user"), request("¿?")],
                },
                request("Health"),
                ImportedItem::Folder {
                    folder: Folder::new("users"),
                    items: Vec::new(),
                },
            ],
            environments: vec![Environment::new("local", Vec::new())],
            warnings: vec![ImportWarning::new(
                "Users / Get user",
                "scripts are not imported",
            )],
            ..ImportedCollection::default()
        }
    }

    #[test]
    fn the_plan_lists_unique_paths_in_order() {
        let plan = plan_import(&sample());
        let paths: Vec<&str> = plan.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "users",
                "users/get-user.yml",
                "users/get-user-2.yml",
                "users/request.yml",
                "health.yml",
                "users-2",
            ]
        );
        assert_eq!(plan.counts(), (2, 4));
        assert_eq!(plan.environments, ["local"]);
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn applying_writes_what_the_plan_showed_with_positions() {
        let collections = MemoryCollections::new(Collection::new("new"));
        let environments = MemoryEnvironments::default();
        let report = apply_import((&collections, &environments), &sample(), "test").unwrap();
        assert_eq!(
            report,
            ImportReport {
                folders: 2,
                requests: 4,
                environments: 1,
            }
        );
        assert_eq!(collections.collection().unwrap().name, "Shop");
        let second = collections.get("users/get-user-2.yml").unwrap();
        assert_eq!((second.name.as_str(), second.seq), ("Get user", Some(2)));
        assert_eq!(collections.get("health.yml").unwrap().seq, Some(2));
        assert_eq!(environments.environments().unwrap().len(), 1);
    }
}
