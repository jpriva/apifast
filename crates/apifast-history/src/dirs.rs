//! Where apifast keeps its local state (ADR 0008): the user's data
//! directory, outside every collection.

use std::path::{Path, PathBuf};

/// apifast's data directory: `APIFAST_DATA_DIR` if set, otherwise
/// `apifast/` in the user's data directory as each platform defines it, or
/// in the temporary directory as a last resort.
pub fn data_dir() -> PathBuf {
    std::env::var_os("APIFAST_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| user_data_dir().map(|d| d.join("apifast")))
        .unwrap_or_else(|| std::env::temp_dir().join("apifast"))
}

/// apifast's configuration directory (settings and keymap):
/// `APIFAST_CONFIG_DIR` if set, otherwise `apifast/` in the user's
/// configuration directory as each platform defines it, or in the temporary
/// directory as a last resort.
pub fn config_dir() -> PathBuf {
    std::env::var_os("APIFAST_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| user_config_dir().map(|d| d.join("apifast")))
        .unwrap_or_else(|| std::env::temp_dir().join("apifast"))
}

/// The settings file under `config_dir`.
pub fn settings_file_in(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.yml")
}

/// The keymap file under `config_dir`.
pub fn keymap_file_in(config_dir: &Path) -> PathBuf {
    config_dir.join("keymap.yml")
}

/// The list of known workspaces under `config_dir` (ADR 0009).
pub fn known_workspaces_file_in(config_dir: &Path) -> PathBuf {
    config_dir.join("workspaces.yml")
}

/// The folder of the default workspace, "My workspace", under `data_dir`.
pub fn default_workspace_in(data_dir: &Path) -> PathBuf {
    data_dir.join("default-workspace")
}

/// What this machine keeps of the workspace in `folder` (expanded
/// collections, open tabs), under `data_dir`, keyed by a hash of its path.
pub fn workspace_state_file_in(data_dir: &Path, folder: &Path) -> PathBuf {
    data_dir
        .join("workspaces")
        .join(format!(
            "{:016x}",
            fnv1a(folder.to_string_lossy().as_bytes())
        ))
        .join("state.yml")
}

/// Folder for the responses of the collection at `root`, under
/// [`data_dir`]. The key is the collection id when it has one, or a hash of
/// its path, so opening or sending never writes into the collection.
pub fn responses_dir(root: &Path, collection_id: Option<&str>) -> PathBuf {
    responses_dir_in(&data_dir(), root, collection_id)
}

/// [`responses_dir`] under an explicit data directory, for callers that
/// choose it (such as tests).
pub fn responses_dir_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("responses")
        .join(collection_key(root, collection_id))
}

/// Folder for the interface session (open tabs and unsaved drafts) of the
/// collection at `root`, under `data_dir`, keyed like [`responses_dir`].
pub fn sessions_dir_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("sessions")
        .join(collection_key(root, collection_id))
}

/// File with the history of sends of the collection at `root`, under
/// `data_dir`, keyed like [`responses_dir`].
pub fn history_file_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("history")
        .join(format!("{}.jsonl", collection_key(root, collection_id)))
}

/// File with the OAuth 2.0 tokens of the collection at `root`, sealed with
/// the master key, under `data_dir`, keyed like [`responses_dir`].
pub fn tokens_file_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("oauth2")
        .join(format!("{}.tokens", collection_key(root, collection_id)))
}

/// File with the cookie jar of the collection at `root`, sealed with the
/// master key, under `data_dir`, keyed like [`responses_dir`].
pub fn cookies_file_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("cookies")
        .join(format!("{}.jar", collection_key(root, collection_id)))
}

/// Folder with the GraphQL schemas of the collection at `root`, under
/// `data_dir`, keyed like [`responses_dir`].
pub fn schemas_dir_in(data_dir: &Path, root: &Path, collection_id: Option<&str>) -> PathBuf {
    data_dir
        .join("schemas")
        .join(collection_key(root, collection_id))
}

/// Moves what apifast keeps of the collection at `root` under `data_dir`
/// (responses, session, history, tokens, cookies and schemas) from its path's hash to `id`, when the
/// collection gets an id. What is already under `id` is left alone.
///
/// # Errors
///
/// If something cannot be moved; what moved before stays moved.
pub fn move_to_id(data_dir: &Path, root: &Path, id: &str) -> std::io::Result<()> {
    let moves = [
        (
            responses_dir_in(data_dir, root, None),
            responses_dir_in(data_dir, root, Some(id)),
        ),
        (
            sessions_dir_in(data_dir, root, None),
            sessions_dir_in(data_dir, root, Some(id)),
        ),
        (
            history_file_in(data_dir, root, None),
            history_file_in(data_dir, root, Some(id)),
        ),
        (
            tokens_file_in(data_dir, root, None),
            tokens_file_in(data_dir, root, Some(id)),
        ),
        (
            cookies_file_in(data_dir, root, None),
            cookies_file_in(data_dir, root, Some(id)),
        ),
        (
            schemas_dir_in(data_dir, root, None),
            schemas_dir_in(data_dir, root, Some(id)),
        ),
    ];
    for (from, to) in moves {
        if from.exists() && !to.exists() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from, &to)?;
        }
    }
    Ok(())
}

fn collection_key(root: &Path, collection_id: Option<&str>) -> String {
    collection_id.map_or_else(
        || format!("path-{:016x}", fnv1a(root.to_string_lossy().as_bytes())),
        str::to_owned,
    )
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

/// The user's configuration directory, as each platform defines it.
fn user_config_dir() -> Option<PathBuf> {
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
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))
    }
}

/// FNV-1a: a stable hash (unlike the standard library's, which is seeded).
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_workspace_has_its_own_state_file() {
        let data = Path::new("/data");
        let a = workspace_state_file_in(data, Path::new("/w/a"));
        assert_eq!(a, workspace_state_file_in(data, Path::new("/w/a")));
        assert_ne!(a, workspace_state_file_in(data, Path::new("/w/b")));
        assert!(a.starts_with("/data/workspaces") && a.ends_with("state.yml"));
        assert_eq!(
            default_workspace_in(data),
            Path::new("/data/default-workspace")
        );
        assert_eq!(
            known_workspaces_file_in(Path::new("/c")),
            Path::new("/c/workspaces.yml")
        );
    }

    #[test]
    fn the_path_hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn data_kept_by_path_moves_to_the_new_id() {
        let data = std::env::temp_dir().join(format!("apifast-move-{}", std::process::id()));
        let root = Path::new("/somewhere/collection");
        let old = sessions_dir_in(&data, root, None);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("session.yml"), "tabs: []").unwrap();
        let history = history_file_in(&data, root, None);
        std::fs::create_dir_all(history.parent().unwrap()).unwrap();
        std::fs::write(&history, "{}\n").unwrap();
        let cookies = cookies_file_in(&data, root, None);
        std::fs::create_dir_all(cookies.parent().unwrap()).unwrap();
        std::fs::write(&cookies, "{}").unwrap();

        move_to_id(&data, root, "col_1").unwrap();
        assert!(
            sessions_dir_in(&data, root, Some("col_1"))
                .join("session.yml")
                .exists()
        );
        assert!(history_file_in(&data, root, Some("col_1")).exists());
        assert!(cookies_file_in(&data, root, Some("col_1")).ends_with("cookies/col_1.jar"));
        assert!(cookies_file_in(&data, root, Some("col_1")).exists());
        assert!(!old.exists() && !history.exists() && !cookies.exists());
        // Nothing to move the second time.
        move_to_id(&data, root, "col_1").unwrap();
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn collections_are_keyed_by_id_or_by_path() {
        let a = collection_key(Path::new("/c"), None);
        assert!(a.starts_with("path-"));
        assert_eq!(a, collection_key(Path::new("/c"), None));
        assert_ne!(a, collection_key(Path::new("/d"), None));
        assert_eq!(collection_key(Path::new("/c"), Some("col_x")), "col_x");
        assert!(responses_dir(Path::new("/c"), Some("col_x")).ends_with("responses/col_x"));
    }
}
