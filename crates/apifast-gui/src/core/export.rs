//! Exporting a collection from the interface: the collection as a Postman
//! file where the user chose, and each environment next to it.

use crate::messages::Say as _;
use crate::strings::{Strings, fill};
use std::path::{Path, PathBuf};

use apifast_app::{ImportWarning, collection_contents};

use super::State;

/// What the last export did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// Increases with every export, so each is told once.
    pub id: u64,
    /// The collection's file.
    pub file: PathBuf,
    /// How many environments went next to it.
    pub environments: usize,
    /// What was not kept, or why nothing was written.
    pub result: Result<Vec<ImportWarning>, String>,
}

impl State {
    /// Writes the collection at `root` to `file` as Postman v2.1, and its
    /// environments next to it as `<name>.postman_environment.json`.
    pub(super) fn export_postman(&mut self, root: &Path, file: &Path) {
        let id = self.next_revision();
        let written = self.open.get(root).map_or_else(
            || {
                Err(fill(
                    self.strings.errors.not_open,
                    &[("path", &root.display())],
                ))
            },
            |open| write_postman(&open.repo, file, self.strings),
        );
        let (environments, result) = match written {
            Ok((environments, warnings)) => (environments, Ok(warnings)),
            Err(why) => (0, Err(why)),
        };
        self.snapshot.exported = Some(Exported {
            id,
            file: file.to_path_buf(),
            environments,
            result,
        });
    }
}

fn write_postman(
    repo: &apifast_storage_fs::FsCollection,
    file: &Path,
    strings: &Strings,
) -> Result<(usize, Vec<ImportWarning>), String> {
    let contents = collection_contents(repo, repo).map_err(|e| e.say(strings))?;
    let all: Vec<String> = contents
        .environments
        .iter()
        .map(|e| e.name.clone())
        .collect();
    let files = apifast_import::write_postman(&contents, file, &all).map_err(|e| e.say(strings))?;
    Ok((files.environments.len(), files.warnings))
}
