//! Finding the collection a request file belongs to.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use apifast_app::RequestPath;

/// The collection root and the request's path inside it.
#[derive(Debug, PartialEq, Eq)]
pub struct Located {
    pub root: PathBuf,
    pub request: RequestPath,
}

/// Finds the collection of `file`: `collection` if given, otherwise the
/// nearest folder above `file` that holds `opencollection.yml`.
pub fn locate(file: &Path, collection: Option<&Path>) -> Result<Located> {
    let file = file
        .canonicalize()
        .with_context(|| format!("cannot open `{}`", file.display()))?;
    let root = match collection {
        Some(dir) => dir
            .canonicalize()
            .with_context(|| format!("cannot open the collection folder `{}`", dir.display()))?,
        None => file
            .ancestors()
            .skip(1)
            .find(|dir| dir.join("opencollection.yml").is_file())
            .map(Path::to_path_buf)
            .with_context(|| {
                format!(
                    "`{}` is not inside a collection: no opencollection.yml in any folder above it",
                    file.display()
                )
            })?,
    };
    let relative = file.strip_prefix(&root).with_context(|| {
        format!(
            "`{}` is not inside the collection `{}`",
            file.display(),
            root.display()
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            bail!("unexpected path `{}`", relative.display());
        };
        let Some(part) = part.to_str() else {
            bail!("`{}` is not valid UTF-8", relative.display());
        };
        parts.push(part);
    }
    Ok(Located {
        root,
        request: RequestPath::new(parts.join("/")),
    })
}

/// The collection `dir` is in: `dir` itself or the nearest folder above it
/// with an opencollection.yml.
///
/// # Errors
///
/// If `dir` cannot be opened or is not inside a collection.
pub fn collection_root(dir: &Path) -> Result<PathBuf> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("cannot open `{}`", dir.display()))?;
    dir.ancestors()
        .find(|d| d.join("opencollection.yml").is_file())
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "`{}` is not inside a collection: no opencollection.yml in it or above it",
                dir.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("opencollection.yml"), "info: {name: c}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("users/admin")).unwrap();
        std::fs::write(dir.path().join("users/admin/list.yml"), "").unwrap();
        dir
    }

    #[test]
    fn finds_the_nearest_collection_above_the_file() {
        let dir = collection();
        let located = locate(&dir.path().join("users/admin/list.yml"), None).unwrap();
        assert_eq!(located.root, dir.path().canonicalize().unwrap());
        assert_eq!(located.request, RequestPath::new("users/admin/list.yml"));
    }

    #[test]
    fn an_explicit_collection_must_contain_the_file() {
        let dir = collection();
        let other = tempfile::tempdir().unwrap();
        let file = dir.path().join("users/admin/list.yml");
        assert!(locate(&file, Some(other.path())).is_err());
        let located = locate(&file, Some(&dir.path().join("users"))).unwrap();
        assert_eq!(located.request, RequestPath::new("admin/list.yml"));
    }

    #[test]
    fn a_file_outside_any_collection_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("r.yml"), "").unwrap();
        let error = locate(&dir.path().join("r.yml"), None).unwrap_err();
        assert!(
            error.to_string().contains("not inside a collection"),
            "{error}"
        );
        assert!(locate(&dir.path().join("missing.yml"), None).is_err());
    }
}
