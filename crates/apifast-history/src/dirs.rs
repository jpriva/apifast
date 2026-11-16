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
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
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
