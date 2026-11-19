//! A collection as exporters read it: the same shape importers make
//! ([`ImportedCollection`]), with its folders and requests loaded, in the
//! tree's order. What cannot be loaded is listed as a warning.

use crate::import::{ImportWarning, ImportedCollection, ImportedItem};
use crate::{AppError, CollectionRepository, EnvironmentRepository, TreeItem};

/// The whole collection of `collections`, with its environments.
///
/// # Errors
///
/// If the collection, its tree or its environments cannot be read. A
/// request that cannot be read is left out with a warning.
pub fn collection_contents(
    collections: &impl CollectionRepository,
    environments: &impl EnvironmentRepository,
) -> Result<ImportedCollection, AppError> {
    let tree = collections.tree()?;
    let mut warnings = Vec::new();
    let items = contents(collections, &tree.items, &mut warnings);
    Ok(ImportedCollection {
        name: tree.collection.name,
        defaults: tree.collection.defaults,
        items,
        environments: environments.environments()?,
        warnings,
    })
}

fn contents(
    collections: &impl CollectionRepository,
    items: &[TreeItem],
    warnings: &mut Vec<ImportWarning>,
) -> Vec<ImportedItem> {
    items
        .iter()
        .filter_map(|item| match item {
            TreeItem::Folder {
                folder, children, ..
            } => Some(ImportedItem::Folder {
                folder: folder.clone(),
                items: contents(collections, children, warnings),
            }),
            TreeItem::Request { path, .. } => match collections.request(path) {
                Ok(loaded) => Some(ImportedItem::Request {
                    request: Box::new(loaded.request),
                    key: Some(path.as_str().to_owned()),
                }),
                Err(e) => {
                    warnings.push(ImportWarning::new(
                        path.as_str(),
                        format!("cannot be read: {e}"),
                    ));
                    None
                }
            },
            TreeItem::Other { path, kind, .. } => {
                warnings.push(ImportWarning::new(
                    path.as_str(),
                    format!("a `{kind}` request cannot be exported yet: left out"),
                ));
                None
            }
            TreeItem::Invalid { path, message } => {
                warnings.push(ImportWarning::new(
                    path.as_str(),
                    format!("cannot be read: {message}"),
                ));
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Collection, Environment, Folder, HttpRequest, Method};

    use super::*;
    use crate::memory::{MemoryCollections, MemoryEnvironments};
    use crate::{RequestPath, apply_import};

    #[test]
    fn a_collection_is_read_back_in_its_order() {
        let collections = MemoryCollections::new(Collection::new("Shop"));
        let environments = MemoryEnvironments::default();
        let request = |name: &str| ImportedItem::Request {
            request: Box::new(HttpRequest::new(name, Method::Get, "/")),
            key: None,
        };
        let source = ImportedCollection {
            name: "Shop".into(),
            items: vec![
                ImportedItem::Folder {
                    folder: Folder::new("Users"),
                    items: vec![request("Zeta"), request("Alpha")],
                },
                request("Health"),
            ],
            environments: vec![Environment::new("local", Vec::new())],
            ..ImportedCollection::default()
        };
        apply_import((&collections, &environments), &source, "").unwrap();
        let read = collection_contents(&collections, &environments).unwrap();
        assert_eq!(read.name, "Shop");
        let ImportedItem::Folder { folder, items } = &read.items[0] else {
            panic!("{:?}", read.items[0]);
        };
        assert_eq!(folder.name, "Users");
        let names: Vec<&str> = (items.iter())
            .map(|i| match i {
                ImportedItem::Request { request, .. } => request.name.as_str(),
                ImportedItem::Folder { .. } => "",
            })
            .collect();
        assert_eq!(names, ["Zeta", "Alpha"], "the tree's order");
        assert_eq!(read.environments.len(), 1);
        assert!(collections.request(&RequestPath::new("health.yml")).is_ok());
    }
}
