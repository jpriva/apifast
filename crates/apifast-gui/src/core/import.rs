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

    /// Reads `collection` (a Postman collection or an OpenAPI spec) and
    /// the Postman `environments`, and makes the plan, for the user to
    /// review.
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
        let source = (self.snapshot.import.as_ref())
            .and_then(|view| view.files.first())
            .map_or_else(String::new, |file| file.display().to_string());
        match write(target, &imported, &source) {
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
    // A folder is a Bruno collection in `.bru`.
    if collection.is_dir() {
        let mut imported = apifast_import::import_bruno(collection).map_err(|e| e.to_string())?;
        if !environments.is_empty() {
            imported.warnings.push(apifast_app::ImportWarning::new(
                "",
                "the Postman environments were not read: a Bruno collection has its own",
            ));
        }
        return Ok(imported);
    }
    let text = read(collection)?;
    // A Postman collection, or else an OpenAPI 3 spec, in JSON or YAML.
    let mut imported = if let Ok(imported) = apifast_import::import_postman(&text) {
        imported
    } else {
        let spec = apifast_storage_fs::yaml_to_json(&text).map_err(|_| not_known(collection))?;
        if apifast_import::is_insomnia(&spec) {
            return apifast_import::import_insomnia(&spec)
                .map_err(|e| format!("`{}`: {e}", collection.display()));
        }
        apifast_import::import_openapi(&spec).map_err(|e| match e {
            apifast_import::OpenApiError::Swagger2 => format!("`{}`: {e}", collection.display()),
            _ => not_known(collection),
        })?
    };
    for path in environments {
        let (environment, warnings) = apifast_import::import_postman_environment(&read(path)?)
            .map_err(|e| format!("`{}`: {e}", path.display()))?;
        imported.environments.push(environment);
        imported.warnings.extend(warnings);
    }
    Ok(imported)
}

fn not_known(file: &Path) -> String {
    format!(
        "`{}` is not a Postman collection (v2.0 or v2.1), an Insomnia export nor an OpenAPI 3 spec",
        file.display()
    )
}

fn write(target: &Path, imported: &ImportedCollection, source: &str) -> Result<(), String> {
    let repo = FsCollection::create(target, &Collection::new(imported.name.clone()))
        .map_err(|e| format!("cannot import into `{}`: {e}", target.display()))?;
    apply_import((&repo, &repo), imported, source).map_err(|e| e.to_string())?;
    Ok(())
}
