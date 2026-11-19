//! GraphQL schemas kept between sessions: one file per key, named by its
//! hash, holding the server's introspection answer as it came.

use std::path::PathBuf;

use apifast_app::{SchemaCache, StoreError};

/// Introspection answers in one folder, such as `schemas/<collection-id>/`.
#[derive(Debug, Clone)]
pub struct FsSchemaCache {
    dir: PathBuf,
}

impl FsSchemaCache {
    /// A cache in `dir`, created on the first save.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn file(&self, key: &str) -> PathBuf {
        self.dir
            .join(format!("{:016x}.json", crate::dirs::fnv1a(key.as_bytes())))
    }
}

impl SchemaCache for FsSchemaCache {
    fn cached_schema(&self, key: &str) -> Result<Option<String>, StoreError> {
        match std::fs::read_to_string(self.file(key)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError(format!("cannot read the schema: {e}"))),
        }
    }

    fn save_schema(&self, key: &str, answer: &str) -> Result<(), StoreError> {
        let error = |e: std::io::Error| StoreError(format!("cannot keep the schema: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(error)?;
        let file = self.file(key);
        // Written beside it, then renamed: a crash leaves the old one whole.
        let partial = file.with_extension("partial");
        std::fs::write(&partial, answer).map_err(error)?;
        std::fs::rename(&partial, &file).map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_kept_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FsSchemaCache::new(dir.path().join("schemas/c"));
        assert_eq!(cache.cached_schema("a").unwrap(), None);
        cache.save_schema("a", "{\"x\": 1}").unwrap();
        cache.save_schema("b", "other").unwrap();
        assert_eq!(
            cache.cached_schema("a").unwrap().as_deref(),
            Some("{\"x\": 1}")
        );
        cache.save_schema("a", "new").unwrap();
        assert_eq!(cache.cached_schema("a").unwrap().as_deref(), Some("new"));
    }
}
