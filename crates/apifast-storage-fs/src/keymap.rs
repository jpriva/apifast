//! `keymap.yml`: the shortcuts the user changed, by action.
//!
//! ```yaml
//! send: Ctrl+Enter
//! palette: [Ctrl+K, Ctrl+Shift+P]
//! reopen_tab: []          # no shortcut
//! ```
//!
//! The file holds only what differs from the defaults; what each name and
//! shortcut means is up to the interface.

use std::path::{Path, PathBuf};

use apifast_app::StoreError;
use oneyaml::Value;

use crate::{fs, write};

/// A keymap file.
#[derive(Debug, Clone)]
pub struct FsKeymapStore {
    file: PathBuf,
}

impl FsKeymapStore {
    /// The keymap in `file`, created on the first save.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Each action named in the file and its shortcuts, as written. A
    /// missing file is an empty keymap; entries that are not text or lists
    /// of text are skipped.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read or is not YAML.
    pub fn load(&self) -> Result<Vec<(String, Vec<String>)>, StoreError> {
        let text = match std::fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(error("cannot read", &self.file, &e)),
        };
        let doc: Value =
            oneyaml::from_str(&text).map_err(|e| error("not valid", &self.file, &e))?;
        let Some(map) = doc.as_mapping() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (key, value) in map {
            let Some(action) = key.as_str() else { continue };
            let shortcuts = match value {
                Value::String(one) => vec![one.clone()],
                Value::Sequence(many) => many
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect(),
                Value::Null => Vec::new(),
                _ => continue,
            };
            out.push((action.to_owned(), shortcuts));
        }
        Ok(out)
    }

    /// Writes `entries` as the whole file.
    ///
    /// # Errors
    ///
    /// If the file or its folder cannot be written.
    pub fn save(&self, entries: &[(String, Vec<String>)]) -> Result<(), StoreError> {
        let mut map = oneyaml::Mapping::new();
        for (action, shortcuts) in entries {
            let value = match shortcuts.as_slice() {
                [one] => Value::String(one.clone()),
                many => Value::from(many.iter().cloned().map(Value::String).collect::<Vec<_>>()),
            };
            map.insert(Value::String(action.clone()), value);
        }
        let text = oneyaml::to_string_with(&Value::Mapping(map), &write::settings())
            .map_err(|e| error("cannot write", &self.file, &e))?;
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| error("cannot create", dir, &e))?;
        }
        fs::write_atomic(&self.file, text.as_bytes())
            .map_err(|e| error("cannot write", &self.file, &e))
    }
}

fn error(what: &str, path: &Path, e: &dyn std::fmt::Display) -> StoreError {
    StoreError(format!("{what} `{}`: {e}", path.display()))
}
