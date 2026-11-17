//! OAuth 2.0 tokens on disk (docs/06): one file per collection, each token
//! sealed with the master key and bound to its key in the cache, so an
//! entry copied to another key does not open.
//!
//! ```json
//! {"format": 1, "tokens": {"credentials\nhttps://id/token\nmy-app": "enc:v1:..."}}
//! ```

use std::path::{Path, PathBuf};

use apifast_app::{OAuth2Token, StoreError, TokenStore};
use apifast_domain::Secret;
use serde_json::{Value, json};

use crate::{KeyBackend, KeyFile, SealedValues, SystemKeyring};

/// The tokens of one collection, sealed with the master key.
pub struct FsTokenStore<K = SystemKeyring, F = KeyFile> {
    values: SealedValues<K, F>,
}

impl FsTokenStore {
    /// The tokens in `file`, with the master key in `keyring` or in its file
    /// under `data_dir`.
    pub fn new(file: PathBuf, data_dir: &Path, keyring: SystemKeyring) -> Self {
        Self::with_backends(file, keyring, KeyFile::in_dir(data_dir))
    }
}

impl<K: KeyBackend, F: KeyBackend> FsTokenStore<K, F> {
    /// As [`FsTokenStore::new`], with the master key in `keyring` or
    /// `key_file`. For tests, a fake keyring.
    pub fn with_backends(file: PathBuf, keyring: K, key_file: F) -> Self {
        Self {
            values: SealedValues::in_section(file, "oauth2-token", "tokens", keyring, key_file),
        }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        self.values.file()
    }
}

fn store_error(e: &crate::SealedError) -> StoreError {
    StoreError(e.to_string())
}

fn to_json(token: &OAuth2Token) -> Value {
    let secret = |s: &Option<Secret>| s.as_ref().map(|s| s.expose().to_owned());
    json!({
        "access_token": token.access_token.expose(),
        "id_token": secret(&token.id_token),
        "refresh_token": secret(&token.refresh_token),
        "token_type": token.token_type,
        "expires_at_ms": token.expires_at_ms,
        "scope": token.scope,
    })
}

fn from_json(value: &Value) -> Option<OAuth2Token> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(OAuth2Token {
        access_token: Secret::new(text("access_token")?),
        id_token: text("id_token").map(Secret::new),
        refresh_token: text("refresh_token").map(Secret::new),
        token_type: text("token_type"),
        expires_at_ms: value.get("expires_at_ms").and_then(Value::as_i64),
        scope: text("scope"),
    })
}

impl<K, F> TokenStore for FsTokenStore<K, F>
where
    K: KeyBackend + Send + Sync,
    F: KeyBackend + Send + Sync,
{
    fn token(&self, key: &str) -> Result<Option<OAuth2Token>, StoreError> {
        // A token that no longer opens (another master key) is as good as
        // none: a new one is obtained.
        let Some(plain) = self.values.get(key).map_err(|e| store_error(&e))? else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&plain).map_err(|e| StoreError(e.to_string()))?;
        Ok(from_json(&value))
    }

    fn save(&self, key: &str, token: &OAuth2Token) -> Result<(), StoreError> {
        let plain = zeroize::Zeroizing::new(to_json(token).to_string());
        self.values
            .set(key, plain.as_bytes())
            .map_err(|e| store_error(&e))
    }

    fn forget(&self, key: &str) -> Result<(), StoreError> {
        self.values.remove(key).map_err(|e| store_error(&e))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn token() -> OAuth2Token {
        OAuth2Token {
            access_token: Secret::new("the-access-token"),
            id_token: None,
            refresh_token: Some(Secret::new("the-refresh-token")),
            token_type: Some("Bearer".into()),
            expires_at_ms: Some(1_700_000_000_000),
            scope: Some("read".into()),
        }
    }

    #[test]
    fn tokens_are_sealed_on_disk_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("oauth2/c.tokens");
        let store = FsTokenStore::new(file.clone(), dir.path(), SystemKeyring::off());
        assert_eq!(store.token("k").unwrap(), None);
        store.save("k", &token()).unwrap();
        assert_eq!(store.token("k").unwrap(), Some(token()));
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("the-access-token") && !text.contains("the-refresh-token"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // An entry copied under another key does not open.
        let doc: Value = serde_json::from_str(&text).unwrap();
        let envelope = doc["tokens"]["k"].clone();
        let moved = json!({"format": 1, "tokens": {"other": envelope}});
        fs::write(&file, moved.to_string()).unwrap();
        assert_eq!(store.token("other").unwrap(), None);
    }

    #[test]
    fn forgetting_removes_only_that_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsTokenStore::new(
            dir.path().join("t.tokens"),
            dir.path(),
            SystemKeyring::off(),
        );
        store.save("a", &token()).unwrap();
        store.save("b", &token()).unwrap();
        store.forget("a").unwrap();
        assert_eq!(store.token("a").unwrap(), None);
        assert!(store.token("b").unwrap().is_some());
    }
}
