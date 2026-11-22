//! Importing a collection from the interface: the files are read and the
//! plan made when the user asks to review it; the collection read is kept
//! until they confirm, so what is written is what they saw.

use crate::messages::Say as _;
use crate::strings::{Strings, fill};
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
    /// The commands about an import, or a reimport.
    pub(super) fn handle_import(&mut self, command: super::Command) {
        match command {
            super::Command::PlanImport {
                collection,
                environments,
            } => self.plan_import(&collection, &environments),
            super::Command::ApplyImport(target) => self.apply_import(&target),
            super::Command::CloseImport => self.close_import(),
            other => self.handle_reimport(other),
        }
    }

    /// Reads `collection` (a Postman collection or an OpenAPI spec) and
    /// the Postman `environments`, and makes the plan, for the user to
    /// review.
    fn plan_import(&mut self, collection: &Path, environments: &[PathBuf]) {
        let mut files = vec![collection.to_path_buf()];
        files.extend(environments.iter().cloned());
        let read = read_source(collection, environments, self.strings);
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
            .cloned()
            .unwrap_or_default();
        match write(target, &imported, &source, self.strings) {
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

/// Reads `collection`, whatever its format (a Bruno folder, a Postman,
/// Insomnia or OpenAPI file), with the Postman `environments`.
pub(super) fn read_source(
    collection: &Path,
    environments: &[PathBuf],
    strings: &Strings,
) -> Result<ImportedCollection, String> {
    let e = &strings.errors;
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .map_err(|why| fill(e.cannot_read, &[("path", &path.display()), ("why", &why)]))
    };
    let at = |path: &Path, why: String| {
        fill(e.file_problem, &[("path", &path.display()), ("why", &why)])
    };
    // A folder is a Bruno collection in `.bru`.
    if collection.is_dir() {
        let mut imported =
            apifast_import::import_bruno(collection).map_err(|why| why.say(strings))?;
        if !environments.is_empty() {
            imported.warnings.push(apifast_app::ImportWarning::new(
                "",
                apifast_app::ImportNote::EnvironmentsIgnored,
            ));
        }
        return Ok(imported);
    }
    let text = read(collection)?;
    // A Postman collection, or else an OpenAPI 3 spec, in JSON or YAML.
    let mut imported = if let Ok(imported) = apifast_import::import_postman(&text) {
        imported
    } else {
        let spec =
            apifast_storage_fs::yaml_to_json(&text).map_err(|_| not_known(collection, strings))?;
        if apifast_import::is_insomnia(&spec) {
            return apifast_import::import_insomnia(&spec)
                .map_err(|why| at(collection, why.say(strings)));
        }
        apifast_import::import_openapi(&spec).map_err(|why| match why {
            apifast_import::OpenApiError::Swagger2 => at(collection, why.say(strings)),
            _ => not_known(collection, strings),
        })?
    };
    for path in environments {
        let (environment, warnings) = apifast_import::import_postman_environment(&read(path)?)
            .map_err(|why| at(path, why.say(strings)))?;
        imported.environments.push(environment);
        imported.warnings.extend(warnings);
    }
    Ok(imported)
}

fn not_known(file: &Path, strings: &Strings) -> String {
    fill(strings.errors.not_importable, &[("path", &file.display())])
}

/// Writes `imported`, read from `source`, as a new collection in `target`.
fn write(
    target: &Path,
    imported: &ImportedCollection,
    source: &Path,
    strings: &Strings,
) -> Result<(), String> {
    let repo =
        FsCollection::create(target, &Collection::new(imported.name.clone())).map_err(|e| {
            fill(
                strings.errors.import_into_failed,
                &[("path", &target.display()), ("why", &e.say(strings))],
            )
        })?;
    let source = apifast_storage_fs::import_source(target, source);
    apply_import((&repo, &repo), imported, &source).map_err(|e| e.say(strings))?;
    Ok(())
}
