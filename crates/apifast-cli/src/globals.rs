//! The workspace's global environment (Bruno's `environments/` next to
//! `workspace.yml`), for `send`, `run` and `describe`: the scope under
//! every other.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use apifast_app::EnvironmentRepository as _;
use apifast_domain::Variable;
use apifast_storage_fs::{FsWorkspace, WORKSPACE_FILE};

/// Which global environment to use, from the command line.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct GlobalArgs {
    /// The workspace's global environment, by name.
    #[arg(long = "global-env", value_name = "NAME")]
    global_env: Option<String>,
    /// The workspace folder, with its workspace.yml; by default, the
    /// nearest one above the collection.
    #[arg(long, value_name = "DIR", requires = "global_env")]
    workspace: Option<PathBuf>,
}

impl GlobalArgs {
    /// The variables of the chosen global environment, with what it
    /// extends; none when none is chosen.
    ///
    /// # Errors
    ///
    /// If there is no workspace, or it has no such environment.
    pub fn variables(&self, collection: &Path) -> Result<Vec<Variable>> {
        let Some(name) = &self.global_env else {
            return Ok(Vec::new());
        };
        let folder = match &self.workspace {
            Some(folder) => folder.clone(),
            None => workspace_above(collection).with_context(|| {
                format!(
                    "--global-env needs a workspace: no {WORKSPACE_FILE} above `{}`; pass --workspace",
                    collection.display()
                )
            })?,
        };
        let workspace = FsWorkspace::open(&folder)
            .with_context(|| format!("cannot open the workspace `{}`", folder.display()))?;
        let environments = workspace.environments()?;
        if !environments.iter().any(|e| &e.name == name) {
            let names: Vec<_> = environments.iter().map(|e| e.name.as_str()).collect();
            bail!(
                "the workspace has no global environment `{name}`; it has: {}",
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            );
        }
        Ok(apifast_app::global_variables(&environments, Some(name))?)
    }
}

/// The nearest folder holding a workspace.yml, from `collection` up.
fn workspace_above(collection: &Path) -> Option<PathBuf> {
    collection
        .ancestors()
        .find(|dir| dir.join(WORKSPACE_FILE).is_file())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(name: &str, workspace: Option<&Path>) -> GlobalArgs {
        GlobalArgs {
            global_env: Some(name.to_owned()),
            workspace: workspace.map(Path::to_path_buf),
        }
    }

    /// A workspace with a global environment and a collection inside.
    fn team() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        };
        write(
            "workspace.yml",
            "opencollection: 1.0.0\ninfo:\n  name: Team\n  type: workspace\ncollections:\n",
        );
        write(
            "environments/shared.yml",
            "name: shared\nvariables:\n  - name: region\n    value: eu\n",
        );
        write(
            "collections/shop/opencollection.yml",
            "opencollection: 1.0.0\n",
        );
        dir
    }

    #[test]
    fn the_workspace_above_the_collection_gives_the_values() {
        let dir = team();
        let shop = dir.path().join("collections/shop");
        let variables = args("shared", None).variables(&shop).unwrap();
        assert_eq!(variables, [Variable::text("region", "eu")]);
        assert_eq!(GlobalArgs::default().variables(&shop).unwrap(), []);
    }

    #[test]
    fn a_missing_workspace_or_environment_says_what_to_do() {
        let dir = team();
        let shop = dir.path().join("collections/shop");
        let error = args("nope", None).variables(&shop).unwrap_err();
        assert!(error.to_string().contains("it has: shared"), "{error}");

        let elsewhere = tempfile::tempdir().unwrap();
        let error = args("shared", None)
            .variables(elsewhere.path())
            .unwrap_err();
        assert!(error.to_string().contains("--workspace"), "{error}");
        let given = args("shared", Some(dir.path()));
        assert_eq!(given.variables(elsewhere.path()).unwrap().len(), 1);
    }
}
