//! Importing a collection from the interface: the files are read and the
//! plan made when the user asks to review it; the collection read is kept
//! until they confirm, so what is written is what they saw.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::{ImportPlan, ImportedCollection, apply_import, plan_import};
use apifast_domain::Collection;
use apifast_storage_fs::FsCollection;

use super::State;

/// What the interface shows of an import being reviewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportView {
    /// The files the plan was made from: the collection, then its
    /// environments.
    pub files: Vec<PathBuf>,
    /// What would be created, or why the files cannot be imported.
    pub plan: Result<Arc<ImportPlan>, String>,
}

impl State {
    /// The commands about an import.
    pub(super) fn handle_import(&mut self, command: super::Command) {
        match command {
            super::Command::PlanImport {
                collection,
                environments,
            } => self.plan_import(&collection, &environments),
            super::Command::ApplyImport(target) => self.apply_import(&target),
            super::Command::CloseImport => self.close_import(),
            _ => {}
        }
    }

    /// Reads the Postman `collection` and `environments` and makes the
    /// plan, for the user to review.
    fn plan_import(&mut self, collection: &Path, environments: &[PathBuf]) {
        let mut files = vec![collection.to_path_buf()];
        files.extend(environments.iter().cloned());
        let read = read_postman(collection, environments);
        let plan = read
            .as_ref()
            .map(|imported| Arc::new(plan_import(imported)))
            .map_err(Clone::clone);
        self.importing = read.ok();
        self.snapshot.import = Some(ImportView { files, plan });
    }

    /// Writes the reviewed import into the new folder `target`, then opens
    /// it in the workspace.
    fn apply_import(&mut self, target: &Path) {
        let Some(imported) = self.importing.take() else {
            return;
        };
        match write(target, &imported) {
            Ok(()) => {
                self.snapshot.import = None;
                self.open_or_report(target);
            }
            Err(message) => {
                // Kept with its plan, to try again in another folder.
                self.importing = Some(imported);
                self.snapshot.error = Some(message);
            }
        }
    }

    /// Forgets the import being reviewed.
    fn close_import(&mut self) {
        self.importing = None;
        self.snapshot.import = None;
    }
}

fn read_postman(collection: &Path, environments: &[PathBuf]) -> Result<ImportedCollection, String> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))
    };
    let mut imported = apifast_import::import_postman(&read(collection)?)
        .map_err(|e| format!("`{}`: {e}", collection.display()))?;
    for path in environments {
        let (environment, warnings) = apifast_import::import_postman_environment(&read(path)?)
            .map_err(|e| format!("`{}`: {e}", path.display()))?;
        imported.environments.push(environment);
        imported.warnings.extend(warnings);
    }
    Ok(imported)
}

fn write(target: &Path, imported: &ImportedCollection) -> Result<(), String> {
    let repo = FsCollection::create(target, &Collection::new(imported.name.clone()))
        .map_err(|e| format!("cannot import into `{}`: {e}", target.display()))?;
    apply_import(&repo, &repo, imported).map_err(|e| e.to_string())?;
    Ok(())
}
