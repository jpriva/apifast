//! A collection's cookie jar on disk (docs/06): one file per collection,
//! `cookies/<collection-id>.jar`, with the whole jar in one envelope sealed
//! with the master key.
//!
//! ```json
//! {"format": 1, "cookies": {"jar": "enc:v1:..."}}
//! ```

use std::path::{Path, PathBuf};

use apifast_app::{CookieStore, StoreError};
use apifast_domain::{Cookie, SameSite};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{KeyBackend, KeyFile, SealedValues, SystemKeyring};

/// The name of the one sealed value in the file.
const JAR: &str = "jar";

/// The cookie jar of one collection, sealed with the master key.
pub struct FsCookieStore<K = SystemKeyring, F = KeyFile> {
    values: SealedValues<K, F>,
}

impl FsCookieStore {
    /// The jar in `file`, with the master key in `keyring` or in its file
    /// under `data_dir`.
    pub fn new(file: PathBuf, data_dir: &Path, keyring: SystemKeyring) -> Self {
        Self::with_backends(file, keyring, KeyFile::in_dir(data_dir))
    }
}

impl<K: KeyBackend, F: KeyBackend> FsCookieStore<K, F> {
    /// As [`FsCookieStore::new`], with the master key in `keyring` or
    /// `key_file`. For tests, a fake keyring.
    pub fn with_backends(file: PathBuf, keyring: K, key_file: F) -> Self {
        Self {
            values: SealedValues::in_section(file, "cookie-jar", "cookies", keyring, key_file),
        }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        self.values.file()
    }
}

impl<K, F> CookieStore for FsCookieStore<K, F>
where
    K: KeyBackend + Send + Sync,
    F: KeyBackend + Send + Sync,
{
    fn load(&self) -> Result<Vec<Cookie>, StoreError> {
        // A jar that no longer opens (another master key) is as good as an
        // empty one: servers set the cookies again.
        let Some(plain) = self
            .values
            .get(JAR)
            .map_err(|e| StoreError(e.to_string()))?
        else {
            return Ok(Vec::new());
        };
        let value: Value = serde_json::from_slice(&plain).map_err(|e| StoreError(e.to_string()))?;
        let cookies = value.as_array().map(Vec::as_slice).unwrap_or_default();
        Ok(cookies.iter().filter_map(from_json).collect())
    }

    fn save(&self, cookies: &[Cookie]) -> Result<(), StoreError> {
        let result = if cookies.is_empty() {
            self.values.remove(JAR)
        } else {
            let all: Vec<Value> = cookies.iter().map(to_json).collect();
            let plain = Zeroizing::new(Value::Array(all).to_string());
            self.values.set(JAR, plain.as_bytes())
        };
        result.map_err(|e| StoreError(e.to_string()))
    }
}

fn to_json(cookie: &Cookie) -> Value {
    json!({
        "name": cookie.name,
        "value": cookie.value,
        "domain": cookie.domain,
        "host_only": cookie.host_only,
        "path": cookie.path,
        "secure": cookie.secure,
        "http_only": cookie.http_only,
        "same_site": cookie.same_site.map(SameSite::as_str),
        "expires_ms": cookie.expires_ms,
        "created_ms": cookie.created_ms,
    })
}

/// A cookie from its JSON, or `None` if a field is missing.
fn from_json(value: &Value) -> Option<Cookie> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    Some(Cookie {
        name: text("name")?,
        value: text("value")?,
        domain: text("domain")?,
        host_only: flag("host_only"),
        path: text("path")?,
        secure: flag("secure"),
        http_only: flag("http_only"),
        same_site: text("same_site").as_deref().and_then(SameSite::parse),
        expires_ms: value.get("expires_ms").and_then(Value::as_i64),
        created_ms: value.get("created_ms").and_then(Value::as_i64).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn cookie(name: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: "the-session-value".into(),
            domain: "api.test".into(),
            host_only: true,
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: Some(SameSite::Lax),
            expires_ms: Some(1_800_000_000_000),
            created_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn the_jar_is_sealed_on_disk_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cookies/c.jar");
        let store = FsCookieStore::new(file.clone(), dir.path(), SystemKeyring::off());
        assert_eq!(store.load().unwrap(), Vec::new());
        let mut session = cookie("b");
        session.expires_ms = None;
        session.same_site = None;
        let jar = [cookie("a"), session];
        store.save(&jar).unwrap();
        assert_eq!(store.load().unwrap(), jar);
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("the-session-value") && !text.contains("api.test"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.save(&[]).unwrap();
        assert_eq!(store.load().unwrap(), Vec::new());
    }

    #[test]
    fn a_cookie_missing_a_field_is_left_out() {
        assert_eq!(from_json(&json!({"name": "a", "value": "1"})), None);
        let read = from_json(&json!({"name": "a", "value": "1", "domain": "d", "path": "/"}));
        assert!(read.is_some_and(|c| !c.secure && c.expires_ms.is_none()));
    }
}
