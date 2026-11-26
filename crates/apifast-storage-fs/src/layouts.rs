//! `layouts.yml`: where the editor's panes were left, and the layouts the
//! user saved with a name (ADR 0015).
//!
//! ```yaml
//! current:            # as the window was left
//!   root: 3
//!   tiles: { … }
//! saved:
//!   - name: Two monitors
//!     panes: { … }
//! ```
//!
//! Each layout is kept as the interface wrote it; what it means is up to
//! the interface, which checks it when it reads it back.

use std::path::{Path, PathBuf};

use apifast_app::StoreError;
use oneyaml::Value;

use crate::{fs, write};

/// What `layouts.yml` holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayoutFile {
    /// The layout as the window was left, if any.
    pub current: Option<serde_json::Value>,
    /// The layouts saved with a name, in order.
    pub saved: Vec<(String, serde_json::Value)>,
}

/// A layouts file.
#[derive(Debug, Clone)]
pub struct FsLayoutStore {
    file: PathBuf,
}

impl FsLayoutStore {
    /// The layouts in `file`, created on the first save.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// What the file holds. A missing file holds nothing; saved layouts
    /// without a name are skipped.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read or is not YAML.
    pub fn load(&self) -> Result<LayoutFile, StoreError> {
        let text = match std::fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LayoutFile::default());
            }
            Err(e) => return Err(error("cannot read", &self.file, &e)),
        };
        let doc: Value =
            oneyaml::from_str(&text).map_err(|e| error("not valid", &self.file, &e))?;
        let json = |value: &Value| oneyaml::from_value::<serde_json::Value>(value.clone()).ok();
        let current = doc.get("current").filter(|v| !v.is_null()).and_then(json);
        let saved = (doc
            .get("saved")
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten())
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.trim();
            let panes = json(entry.get("panes")?)?;
            (!name.is_empty()).then(|| (name.to_owned(), panes))
        })
        .collect();
        Ok(LayoutFile { current, saved })
    }

    /// Writes `layouts` as the whole file.
    ///
    /// # Errors
    ///
    /// If the file or its folder cannot be written.
    pub fn save(&self, layouts: &LayoutFile) -> Result<(), StoreError> {
        let yaml = |value: &serde_json::Value| {
            oneyaml::to_value(value).map_err(|e| error("cannot write", &self.file, &e))
        };
        let mut map = oneyaml::Mapping::new();
        if let Some(current) = &layouts.current {
            map.insert(Value::from("current"), yaml(current)?);
        }
        let mut saved = Vec::new();
        for (name, panes) in &layouts.saved {
            let mut entry = oneyaml::Mapping::new();
            entry.insert(Value::from("name"), Value::String(name.clone()));
            entry.insert(Value::from("panes"), yaml(panes)?);
            saved.push(Value::Mapping(entry));
        }
        map.insert(Value::from("saved"), Value::from(saved));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layouts_survive_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsLayoutStore::new(dir.path().join("config/layouts.yml"));
        assert_eq!(store.load().unwrap(), LayoutFile::default());
        let file = LayoutFile {
            current: Some(
                json!({"root": 3, "tiles": {"1": {"Pane": "Request"}}, "shares": [0.5, 1.5]}),
            ),
            saved: vec![
                ("Bruno".into(), json!({"root": 1})),
                ("Two monitors".into(), json!({"root": 2, "invisible": []})),
            ],
        };
        store.save(&file).unwrap();
        assert_eq!(store.load().unwrap(), file);
    }

    #[test]
    fn entries_without_a_name_are_skipped_and_bad_yaml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsLayoutStore::new(dir.path().join("layouts.yml"));
        std::fs::write(
            store.file(),
            "saved:\n  - name: ''\n    panes: {root: 1}\n  - panes: {root: 2}\n  - name: Ok\n    panes: {root: 3}\n",
        )
        .unwrap();
        let file = store.load().unwrap();
        assert_eq!(file.current, None);
        assert_eq!(file.saved, [("Ok".to_owned(), json!({"root": 3}))]);

        std::fs::write(store.file(), "saved: [").unwrap();
        assert!(store.load().is_err());
    }
}
