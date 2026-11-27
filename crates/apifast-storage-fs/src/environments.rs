//! The environments in a folder's `environments/`: a collection's, or a
//! workspace's, which Bruno calls global. Both are written the same way,
//! keeping what each file has that the model does not.

use std::path::Path;

use apifast_app::RepositoryError;
use apifast_domain::Environment;

use crate::{fs, io_error, is_yaml, parse, read, read_text_in, save_document_in, write};

/// The folder that holds them, inside a collection or a workspace.
pub(crate) const ENVIRONMENTS_DIR: &str = "environments";

/// The environments of the folder `root` (its `environments/`).
#[derive(Debug, Clone)]
pub(crate) struct FsEnvironments<'a> {
    root: &'a Path,
}

impl<'a> FsEnvironments<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self { root }
    }

    /// Environment files with their parsed content, sorted by file name.
    fn files(&self) -> Result<Vec<(String, Environment)>, RepositoryError> {
        let dir = self.root.join(ENVIRONMENTS_DIR);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(ENVIRONMENTS_DIR, &e)),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| is_yaml(n))
            .collect();
        names.sort();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let relative = format!("{ENVIRONMENTS_DIR}/{name}");
            let Some(text) = read_text_in(self.root, &relative)? else {
                continue;
            };
            let doc = parse(&relative, &text)?;
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
            out.push((relative.clone(), read::environment(&doc, stem)));
        }
        Ok(out)
    }

    /// Writes `environment` to the file at `relative`, keeping what the
    /// file has that the model does not.
    fn save_at(&self, relative: &str, environment: &Environment) -> Result<(), RepositoryError> {
        let stem = relative
            .trim_start_matches("environments/")
            .rsplit_once('.')
            .map_or(relative, |(s, _)| s)
            .to_owned();
        save_document_in(
            self.root,
            relative,
            environment,
            (
                |doc: &oneyaml::Value| Ok(read::environment(doc, &stem)),
                write::update_environment,
                write::new_environment,
            ),
        )
    }

    /// All of them.
    pub(crate) fn all(&self) -> Result<Vec<Environment>, RepositoryError> {
        Ok(self.files()?.into_iter().map(|(_, env)| env).collect())
    }

    /// Saves `environment`, matched by name, creating its file if needed.
    pub(crate) fn save(&self, environment: &Environment) -> Result<(), RepositoryError> {
        let files = self.files()?;
        let relative = match files.iter().find(|(_, e)| e.name == environment.name) {
            Some((relative, _)) => relative.clone(),
            None => free_file(&files, &environment.name),
        };
        self.save_at(&relative, environment)
    }

    /// Renames `from` to `to`, and its file after it, as Bruno does.
    pub(crate) fn rename(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        let files = self.files()?;
        if files.iter().any(|(_, e)| e.name == to) {
            return Err(RepositoryError::Invalid {
                path: to.to_owned(),
                message: "an environment with that name already exists".into(),
            });
        }
        let (relative, environment) = files
            .iter()
            .find(|(_, e)| e.name == from)
            .ok_or_else(|| RepositoryError::NotFound(from.to_owned()))?;
        let others: Vec<_> = files
            .iter()
            .filter(|(r, _)| r != relative)
            .cloned()
            .collect();
        let target = free_file(&others, to);
        if target != *relative {
            std::fs::rename(self.root.join(relative), self.root.join(&target))
                .map_err(|e| io_error(relative, &e))?;
        }
        let renamed = Environment {
            name: to.to_owned(),
            ..environment.clone()
        };
        self.save_at(&target, &renamed)
    }

    /// Removes the environment `name` and its file.
    pub(crate) fn delete(&self, name: &str) -> Result<(), RepositoryError> {
        let files = self.files()?;
        let (relative, _) = files
            .iter()
            .find(|(_, e)| e.name == name)
            .ok_or_else(|| RepositoryError::NotFound(name.to_owned()))?;
        std::fs::remove_file(self.root.join(relative)).map_err(|e| io_error(relative, &e))
    }
}

/// A path in `environments/` for an environment named `name`, after the
/// name and not taken by any of `files`.
fn free_file(files: &[(String, Environment)], name: &str) -> String {
    let base = fs::file_name_for(name);
    let taken = |candidate: &str| files.iter().any(|(r, _)| r == candidate);
    let mut candidate = format!("{ENVIRONMENTS_DIR}/{base}.yml");
    let mut n = 2;
    while taken(&candidate) {
        candidate = format!("{ENVIRONMENTS_DIR}/{base}-{n}.yml");
        n += 1;
    }
    candidate
}
