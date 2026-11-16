//! `settings.yml`: the application's settings, one value per key.
//!
//! ```yaml
//! theme: dark
//! ui_scale: 1.25
//! history_days: 30
//! ```
//!
//! What each key means is up to the interface; this store only reads and
//! writes the values, as text.

use std::path::{Path, PathBuf};

use apifast_app::StoreError;
use oneyaml::{Number, Value};

use crate::{fs, write};

/// A settings file.
#[derive(Debug, Clone)]
pub struct FsSettingsStore {
    file: PathBuf,
}

impl FsSettingsStore {
    /// The settings in `file`, created on the first save.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Each key with its value as text. A missing file has no settings;
    /// values that are lists or mappings are skipped.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read or is not YAML.
    pub fn load(&self) -> Result<Vec<(String, String)>, StoreError> {
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
            let Some(key) = key.as_str() else { continue };
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => continue,
            };
            out.push((key.to_owned(), text));
        }
        Ok(out)
    }

    /// Writes `entries` as the whole file. Numbers and `true`/`false` are
    /// written as such, the rest as text.
    ///
    /// # Errors
    ///
    /// If the file or its folder cannot be written.
    pub fn save(&self, entries: &[(String, String)]) -> Result<(), StoreError> {
        let mut map = oneyaml::Mapping::new();
        for (key, text) in entries {
            let value = if let Ok(n) = text.parse::<i64>() {
                Value::Number(Number::from(n))
            } else if let Ok(n) = text.parse::<f64>() {
                Value::Number(Number::from(n))
            } else if let Ok(flag) = text.parse::<bool>() {
                Value::Bool(flag)
            } else {
                Value::String(text.clone())
            };
            map.insert(Value::String(key.clone()), value);
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
