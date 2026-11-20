//! Importing a source again into the collection it made (ADR 0010):
//! requests are matched by their key in the source, and compared three
//! ways (as last imported, as they are now, as the source has them), so
//! the user's edits are never overwritten without asking.
//!
//! | Source since last import | File since last import | Action        | Default |
//! |---|---|---|---|
//! | new                      | —                      | `Create`      | on      |
//! | the same                 | any                    | `Unchanged`   | —       |
//! | changed                  | the same               | `Update`      | on      |
//! | changed                  | changed                | `Conflict`    | off     |
//! | has it                   | deleted                | `Deleted`     | off     |
//! | no longer has it         | any                    | `Removed`     | off     |

use std::collections::BTreeSet;

use apifast_domain::{Folder, HttpRequest};

use crate::import::{ImportReport, ImportedItem, PlannedEntry, walk_planned};
use crate::{
    AppError, CollectionRepository, EnvironmentRepository, ImportWarning, ImportedCollection,
    RepositoryError, RequestPath,
};

/// Keeps what was last imported into a collection, to import it again.
pub trait ImportRecords {
    /// The record of the last import, if the collection was imported.
    ///
    /// # Errors
    ///
    /// If it cannot be read.
    fn import_record(&self) -> Result<Option<ImportRecord>, RepositoryError>;

    /// Keeps `record` in place of the last one.
    ///
    /// # Errors
    ///
    /// If it cannot be written.
    fn save_import_record(&self, record: &ImportRecord) -> Result<(), RepositoryError>;
}

/// What the last import wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportRecord {
    /// Where it came from, as the user would name it: `openapi pets.yaml`.
    pub source: String,
    /// Each request it wrote, by its key in the source.
    pub requests: Vec<RecordedRequest>,
}

/// A request an import wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// Its key in the source.
    pub key: String,
    /// Its file in the collection.
    pub path: String,
    /// A fingerprint of the request as the source had it.
    pub hash: String,
}

/// The fingerprint of a request, without its position: two requests with
/// the same one are the same. It depends on the domain's shape, so a
/// version of apifast that adds a field sees every request as edited
/// once, and asks instead of overwriting.
pub fn request_hash(request: &HttpRequest) -> String {
    let unpositioned = HttpRequest {
        seq: None,
        ..request.clone()
    };
    let text = format!("{unpositioned:?}");
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// What importing again does with one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReimportAction {
    /// New in the source: created.
    Create,
    /// Changed in the source, not here: updated.
    Update,
    /// The same in the source: left as it is, edited or not.
    Unchanged,
    /// Changed in the source and here: kept unless chosen.
    Conflict,
    /// Deleted here, still in the source: created again if chosen.
    Deleted,
    /// No longer in the source: deleted here if chosen.
    Removed,
}

/// A request of a reimport, what would happen to it, and whether it is
/// chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReimportEntry {
    /// Its key in the source.
    pub key: String,
    /// Its name.
    pub name: String,
    /// Its file in the collection.
    pub path: String,
    /// What happens to it.
    pub action: ReimportAction,
    /// Whether it happens. `Unchanged` ones are never written.
    pub selected: bool,
    /// Its fingerprint in the last import's record, kept when it is not
    /// written, so it is offered again next time.
    pub previous_hash: Option<String>,
}

/// What importing again would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReimportPlan {
    /// Where the last import came from.
    pub previous_source: String,
    /// Each request of the source and of the last import, in the source's
    /// order, then the removed ones.
    pub entries: Vec<ReimportEntry>,
    /// What could not be kept as it was.
    pub warnings: Vec<ImportWarning>,
}

impl ReimportPlan {
    /// How many entries have `action`.
    pub fn count(&self, action: ReimportAction) -> usize {
        self.entries.iter().filter(|e| e.action == action).count()
    }
}

/// What importing `imported` again into `collections`, last imported as
/// `record`, would do.
///
/// # Errors
///
/// If a request file exists but cannot be read.
pub fn plan_reimport(
    collections: &impl CollectionRepository,
    record: &ImportRecord,
    imported: &ImportedCollection,
) -> Result<ReimportPlan, AppError> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut taken = BTreeSet::new();
    let mut failure = None;
    walk_planned(&imported.items, "", &mut |entry, item| {
        let ImportedItem::Request { request, key } = item else {
            return;
        };
        if failure.is_some() {
            return;
        }
        let key = key.clone().unwrap_or_else(|| entry.path.clone());
        seen.insert(key.clone());
        let hash = request_hash(request);
        let recorded = record.requests.iter().find(|r| r.key == key);
        let planned = match recorded {
            None => Ok((
                free_path(collections, &entry, &mut taken),
                ReimportAction::Create,
            )),
            Some(recorded) => {
                compare(collections, recorded, &hash).map(|a| (recorded.path.clone(), a))
            }
        };
        match planned {
            Ok((path, action)) => entries.push(ReimportEntry {
                key,
                name: entry.name,
                path,
                action,
                selected: matches!(action, ReimportAction::Create | ReimportAction::Update),
                previous_hash: recorded.map(|r| r.hash.clone()),
            }),
            Err(e) => failure = Some(e),
        }
    });
    if let Some(error) = failure {
        return Err(error.into());
    }
    for recorded in record.requests.iter().filter(|r| !seen.contains(&r.key)) {
        if collections.exists(&recorded.path)? {
            entries.push(ReimportEntry {
                key: recorded.key.clone(),
                name: recorded
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                path: recorded.path.clone(),
                action: ReimportAction::Removed,
                selected: false,
                previous_hash: Some(recorded.hash.clone()),
            });
        }
    }
    Ok(ReimportPlan {
        previous_source: record.source.clone(),
        entries,
        warnings: imported.warnings.clone(),
    })
}

/// What changed since `recorded`: in the source (`hash`) and in the file.
fn compare(
    collections: &impl CollectionRepository,
    recorded: &RecordedRequest,
    hash: &str,
) -> Result<ReimportAction, RepositoryError> {
    let current = match collections.request(&RequestPath::new(recorded.path.clone())) {
        Ok(loaded) => loaded.request,
        Err(RepositoryError::NotFound(_)) => return Ok(ReimportAction::Deleted),
        Err(e) => return Err(e),
    };
    Ok(if hash == recorded.hash {
        ReimportAction::Unchanged
    } else if request_hash(&current) == recorded.hash {
        ReimportAction::Update
    } else {
        ReimportAction::Conflict
    })
}

/// `entry`'s path, or the next free one (`name-2.yml`) if a file of the
/// collection, or another new request, has it.
fn free_path(
    collections: &impl CollectionRepository,
    entry: &PlannedEntry,
    taken: &mut BTreeSet<String>,
) -> String {
    let (stem, extension) = entry.path.rsplit_once('.').unwrap_or((&entry.path, "yml"));
    let mut path = entry.path.clone();
    let mut n = 2;
    while taken.contains(&path) || collections.exists(&path).unwrap_or(false) {
        path = format!("{stem}-{n}.{extension}");
        n += 1;
    }
    taken.insert(path.clone());
    path
}

/// Does what `plan` chose, with the requests of `imported`, and records the
/// import as `source`. A changed request keeps the place (`seq`) it has
/// here. New folders are created as the source has them.
///
/// # Errors
///
/// On the first file that cannot be written or deleted; what was done
/// before stays, and the record is not updated.
pub fn apply_reimport(
    (collections, environments): (
        &(impl CollectionRepository + ImportRecords),
        &impl EnvironmentRepository,
    ),
    imported: &ImportedCollection,
    plan: &ReimportPlan,
    source: &str,
) -> Result<ImportReport, AppError> {
    let mut requests: Vec<(String, HttpRequest)> = Vec::new();
    let mut folders: Vec<(String, Folder)> = Vec::new();
    walk_planned(&imported.items, "", &mut |entry, item| match item {
        ImportedItem::Folder { folder, .. } => folders.push((entry.path, (**folder).clone())),
        ImportedItem::Request { request, key } => {
            requests.push((key.clone().unwrap_or(entry.path), request.as_ref().clone()));
        }
    });
    let mut report = ImportReport {
        folders: 0,
        requests: 0,
        environments: 0,
    };
    let mut record = ImportRecord {
        source: source.to_owned(),
        requests: Vec::new(),
    };
    for entry in &plan.entries {
        let new = requests
            .iter()
            .find(|(key, _)| *key == entry.key)
            .map(|(_, r)| r);
        if entry.selected {
            match (entry.action, new) {
                (ReimportAction::Removed, _) => {
                    collections.delete_path(&entry.path)?;
                    report.requests += 1;
                }
                (ReimportAction::Unchanged, _) | (_, None) => {}
                (_, Some(new)) => {
                    report.folders += create_folders(collections, &entry.path, &folders)?;
                    let path = RequestPath::new(entry.path.clone());
                    let mut request = new.clone();
                    // An update keeps the place the request has here.
                    if let Ok(current) = collections.request(&path) {
                        request.seq = current.request.seq;
                    }
                    collections.save_request(&path, &request)?;
                    report.requests += 1;
                }
            }
        }
        // What was written, or is the same, is recorded as the source has
        // it; what was left out keeps its last record, to be offered again.
        let written = entry.selected || entry.action == ReimportAction::Unchanged;
        let hash = match (written, entry.action, new) {
            (true, ReimportAction::Removed, _) => None,
            (true, _, Some(new)) => Some(request_hash(new)),
            _ => entry.previous_hash.clone(),
        };
        if let Some(hash) = hash {
            record.requests.push(RecordedRequest {
                key: entry.key.clone(),
                path: entry.path.clone(),
                hash,
            });
        }
    }
    // Environments are only added: the ones here may hold edits.
    let existing = environments.environments()?;
    for environment in &imported.environments {
        if !existing.iter().any(|e| e.name == environment.name) {
            environments.save_environment(environment)?;
            report.environments += 1;
        }
    }
    collections.save_import_record(&record)?;
    Ok(report)
}

/// Creates the folders of `path` that do not exist, as the source has
/// them. Returns how many.
fn create_folders(
    collections: &impl CollectionRepository,
    path: &str,
    folders: &[(String, Folder)],
) -> Result<usize, RepositoryError> {
    let mut created = 0;
    let mut prefix = String::new();
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        if !collections.exists(&prefix)? {
            let folder = (folders.iter())
                .find(|(p, _)| *p == prefix)
                .map_or_else(|| Folder::new(*part), |(_, f)| f.clone());
            collections.save_folder(&prefix, &folder)?;
            created += 1;
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Collection, Method};

    use super::*;
    use crate::memory::{MemoryCollections, MemoryEnvironments};
    use crate::{ImportRecords, apply_import};

    fn request(key: &str, url: &str) -> ImportedItem {
        ImportedItem::Request {
            request: Box::new(HttpRequest::new(key, Method::Get, url)),
            key: Some(key.to_owned()),
        }
    }

    fn source(items: Vec<ImportedItem>) -> ImportedCollection {
        ImportedCollection {
            name: "Shop".into(),
            items: vec![ImportedItem::Folder {
                folder: Box::new(Folder::new("Users")),
                items,
            }],
            ..ImportedCollection::default()
        }
    }

    fn action(plan: &ReimportPlan, key: &str) -> (ReimportAction, bool) {
        let entry = plan.entries.iter().find(|e| e.key == key).unwrap();
        (entry.action, entry.selected)
    }

    #[test]
    fn importing_again_updates_creates_and_asks_before_overwriting_edits() {
        let collections = MemoryCollections::new(Collection::new("new"));
        let environments = MemoryEnvironments::default();
        let first = source(vec![
            request("same", "/same"),
            request("changes", "/v1"),
            request("edited", "/v1"),
            request("deleted", "/x"),
            request("removed", "/old"),
        ]);
        apply_import((&collections, &environments), &first, "openapi pets.yaml").unwrap();
        let record = collections.import_record().unwrap().unwrap();
        assert_eq!(record.requests.len(), 5);
        assert_eq!(record.requests[1].path, "users/changes.yml");

        // Here: one request edited, one deleted.
        let path = RequestPath::new("users/edited.yml");
        let mut edited = collections.request(&path).unwrap().request;
        edited
            .headers
            .push(apifast_domain::Header::new("X-Mine", "1"));
        collections.save_request(&path, &edited).unwrap();
        collections.delete_path("users/deleted.yml").unwrap();
        // In the source: two changed, one new, one gone.
        let second = source(vec![
            request("same", "/same"),
            request("changes", "/v2"),
            request("edited", "/v2"),
            request("deleted", "/x"),
            request("new", "/new"),
        ]);
        let plan = plan_reimport(&collections, &record, &second).unwrap();
        assert_eq!(plan.previous_source, "openapi pets.yaml");
        assert_eq!(action(&plan, "same"), (ReimportAction::Unchanged, false));
        assert_eq!(action(&plan, "changes"), (ReimportAction::Update, true));
        assert_eq!(action(&plan, "edited"), (ReimportAction::Conflict, false));
        assert_eq!(action(&plan, "deleted"), (ReimportAction::Deleted, false));
        assert_eq!(action(&plan, "new"), (ReimportAction::Create, true));
        assert_eq!(action(&plan, "removed"), (ReimportAction::Removed, false));

        let report = apply_reimport(
            (&collections, &environments),
            &second,
            &plan,
            "openapi pets.yaml",
        )
        .unwrap();
        assert_eq!(report.requests, 2, "the update and the new one");
        let get = |p: &str| collections.request(&RequestPath::new(p)).map(|l| l.request);
        assert_eq!(get("users/changes.yml").unwrap().url, "/v2");
        assert_eq!(
            get("users/changes.yml").unwrap().seq,
            Some(2),
            "keeps its place"
        );
        assert_eq!(
            get("users/edited.yml").unwrap().url,
            "/v1",
            "the edit is kept"
        );
        assert_eq!(get("users/new.yml").unwrap().url, "/new");
        assert!(
            get("users/removed.yml").is_ok(),
            "not deleted unless chosen"
        );
        assert!(
            get("users/deleted.yml").is_err(),
            "not recreated unless chosen"
        );

        // The same source again: what was left out is offered again.
        let record = collections.import_record().unwrap().unwrap();
        let again = plan_reimport(&collections, &record, &second).unwrap();
        assert_eq!(
            action(&again, "changes"),
            (ReimportAction::Unchanged, false)
        );
        assert_eq!(action(&again, "new"), (ReimportAction::Unchanged, false));
        assert_eq!(action(&again, "edited"), (ReimportAction::Conflict, false));
        assert_eq!(action(&again, "deleted"), (ReimportAction::Deleted, false));
        assert_eq!(action(&again, "removed"), (ReimportAction::Removed, false));
    }

    #[test]
    fn chosen_conflicts_take_the_source_and_chosen_removals_delete() {
        let collections = MemoryCollections::new(Collection::new("new"));
        let environments = MemoryEnvironments::default();
        let first = source(vec![request("edited", "/v1"), request("removed", "/old")]);
        apply_import((&collections, &environments), &first, "s").unwrap();
        let path = RequestPath::new("users/edited.yml");
        let mut edited = collections.request(&path).unwrap().request;
        edited.url = "/mine".into();
        collections.save_request(&path, &edited).unwrap();
        // A file of the user's where a new request would go.
        collections
            .save_request(
                &RequestPath::new("users/new.yml"),
                &HttpRequest::new("mine", Method::Get, "/"),
            )
            .unwrap();

        let second = source(vec![request("edited", "/v2"), request("new", "/new")]);
        let record = collections.import_record().unwrap().unwrap();
        let mut plan = plan_reimport(&collections, &record, &second).unwrap();
        for entry in &mut plan.entries {
            entry.selected = true;
        }
        let new = plan.entries.iter().find(|e| e.key == "new").unwrap();
        assert_eq!(new.path, "users/new-2.yml", "the user's file is not taken");
        apply_reimport((&collections, &environments), &second, &plan, "s").unwrap();
        let get = |p: &str| collections.request(&RequestPath::new(p)).map(|l| l.request);
        assert_eq!(get("users/edited.yml").unwrap().url, "/v2");
        assert!(get("users/removed.yml").is_err());
        assert_eq!(get("users/new.yml").unwrap().name, "mine");
        assert_eq!(get("users/new-2.yml").unwrap().url, "/new");
    }

    #[test]
    fn fingerprints_ignore_the_position() {
        let mut a = HttpRequest::new("r", Method::Get, "/");
        let b = HttpRequest {
            seq: Some(9),
            ..a.clone()
        };
        assert_eq!(request_hash(&a), request_hash(&b));
        a.url = "/x".into();
        assert_ne!(request_hash(&a), request_hash(&b));
    }
}
