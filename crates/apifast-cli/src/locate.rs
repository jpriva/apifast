//! Finding the collection a request file belongs to, and where its responses go.

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

/// Folder for the responses of the collection at `root`: under
/// `APIFAST_DATA_DIR`, or the user's data directory, then
/// `responses/<key>`. The key is the collection id when it has one, or a
/// hash of its path: `send` never writes into the collection to create an id.
pub fn responses_dir(root: &Path, collection_id: Option<&str>) -> PathBuf {
    let base = std::env::var_os("APIFAST_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| user_data_dir().map(|d| d.join("apifast")))
        .unwrap_or_else(|| std::env::temp_dir().join("apifast"));
    let key = collection_id.map_or_else(
        || format!("path-{:016x}", fnv1a(root.to_string_lossy().as_bytes())),
        str::to_owned,
    );
    base.join("responses").join(key)
}

/// The user's data directory, as each platform defines it.
fn user_data_dir() -> Option<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        // XDG Base Directory specification.
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local/share")))
    }
}

/// FNV-1a: a stable hash (unlike the standard library's, which is seeded).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
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

    #[test]
    fn the_path_hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        let a = responses_dir(Path::new("/c"), None);
        assert_eq!(a, responses_dir(Path::new("/c"), None));
        assert_ne!(a, responses_dir(Path::new("/d"), None));
        assert!(responses_dir(Path::new("/c"), Some("col_x")).ends_with("responses/col_x"));
    }
}
