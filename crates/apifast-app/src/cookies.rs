//! The cookie jar a collection's sends share (docs/07, "Cookies"): the
//! transport sends from it and keeps in it what each response sets, and the
//! driver persists it through a [`CookieStore`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use apifast_domain::{Cookie, CookieJar, CookieTarget};

use crate::StoreError;

/// Keeps a collection's cookies between runs of the application.
pub trait CookieStore: Send + Sync {
    /// The cookies kept.
    ///
    /// # Errors
    ///
    /// If the store cannot be read.
    fn load(&self) -> Result<Vec<Cookie>, StoreError>;

    /// Keeps `cookies`, in place of those kept before.
    ///
    /// # Errors
    ///
    /// If the store cannot be written.
    fn save(&self, cookies: &[Cookie]) -> Result<(), StoreError>;
}

/// A jar shared by the sends of a collection, and whether cookies are sent
/// and stored (both by default, as in Bruno).
pub struct Cookies {
    jar: Mutex<CookieJar>,
    send: AtomicBool,
    store: AtomicBool,
    changed: AtomicBool,
}

/// Cookie values can be session tokens: printed, it only says how many.
impl std::fmt::Debug for Cookies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cookies")
            .field("count", &self.lock().cookies().len())
            .field("send", &self.send.load(Ordering::Relaxed))
            .field("store", &self.store.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Default for Cookies {
    fn default() -> Self {
        Self::with_jar(CookieJar::new())
    }
}

impl Cookies {
    /// An empty jar that sends and stores.
    pub fn new() -> Self {
        Self::default()
    }

    /// `jar`, sending and storing.
    pub fn with_jar(jar: CookieJar) -> Self {
        Self {
            jar: Mutex::new(jar),
            send: AtomicBool::new(true),
            store: AtomicBool::new(true),
            changed: AtomicBool::new(false),
        }
    }

    /// The cookies in `store`, leaving out those expired at `now_ms`.
    ///
    /// # Errors
    ///
    /// If the store cannot be read.
    pub fn load(store: &dyn CookieStore, now_ms: i64) -> Result<Self, StoreError> {
        Ok(Self::with_jar(CookieJar::with_cookies(
            store.load()?,
            now_ms,
        )))
    }

    /// Whether cookies from the jar are sent, and whether the cookies that
    /// responses set are stored.
    pub fn set_policy(&self, send: bool, store: bool) {
        self.send.store(send, Ordering::Relaxed);
        self.store.store(store, Ordering::Relaxed);
    }

    /// The `Cookie` header from the jar for a request to `target`, unless
    /// cookies are not sent.
    pub fn header_for(&self, target: &CookieTarget<'_>, now_ms: i64) -> Option<String> {
        if !self.send.load(Ordering::Relaxed) {
            return None;
        }
        self.lock().header_for(target, now_ms)
    }

    /// Keeps what the `Set-Cookie` values `headers`, received from `target`,
    /// set, unless cookies are not stored.
    pub fn receive<'h>(
        &self,
        headers: impl IntoIterator<Item = &'h str>,
        target: &CookieTarget<'_>,
        now_ms: i64,
    ) {
        if !self.store.load(Ordering::Relaxed) {
            return;
        }
        let mut jar = self.lock();
        let mut changed = false;
        for header in headers {
            changed |= jar.receive(header, target, now_ms);
        }
        if changed {
            self.changed.store(true, Ordering::Relaxed);
        }
    }

    /// A copy of the jar.
    pub fn jar(&self) -> CookieJar {
        self.lock().clone()
    }

    /// Changes the jar with `edit`, as the cookie editor does.
    pub fn edit<R>(&self, edit: impl FnOnce(&mut CookieJar) -> R) -> R {
        let result = edit(&mut self.lock());
        self.changed.store(true, Ordering::Relaxed);
        result
    }

    /// Writes the jar to `store` if it changed since the last time.
    ///
    /// # Errors
    ///
    /// If the store cannot be written; the jar then counts as changed, to
    /// try again.
    pub fn save_if_changed(&self, store: &dyn CookieStore) -> Result<bool, StoreError> {
        if !self.changed.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let jar = self.jar();
        store.save(jar.cookies()).inspect_err(|_| {
            self.changed.store(true, Ordering::Relaxed);
        })?;
        Ok(true)
    }

    fn lock(&self) -> MutexGuard<'_, CookieJar> {
        // A jar is always whole: a panic elsewhere does not leave it half
        // changed.
        self.jar.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryCookies;

    const NOW: i64 = 1_700_000_000_000;

    fn target() -> CookieTarget<'static> {
        CookieTarget::new("http", "api.test", "/")
    }

    #[test]
    fn the_policy_turns_sending_and_storing_off() {
        let cookies = Cookies::new();
        cookies.set_policy(true, false);
        cookies.receive(["a=1"], &target(), NOW);
        assert!(cookies.jar().is_empty());
        cookies.set_policy(false, true);
        cookies.receive(["a=1", "b=2"], &target(), NOW);
        assert_eq!(cookies.header_for(&target(), NOW), None);
        cookies.set_policy(true, true);
        assert_eq!(
            cookies.header_for(&target(), NOW).as_deref(),
            Some("a=1; b=2")
        );
    }

    #[test]
    fn the_jar_is_saved_only_when_it_changed() {
        let store = MemoryCookies::default();
        let cookies = Cookies::new();
        assert!(!cookies.save_if_changed(&store).unwrap());
        cookies.receive(["a=1"], &target(), NOW);
        assert!(cookies.save_if_changed(&store).unwrap());
        assert!(!cookies.save_if_changed(&store).unwrap());
        cookies.receive(["a=1"], &target(), NOW + 1);
        assert!(!cookies.save_if_changed(&store).unwrap(), "same cookie");
        cookies.edit(CookieJar::clear);
        assert!(cookies.save_if_changed(&store).unwrap());
        assert_eq!(store.load().unwrap(), Vec::new());
    }

    #[test]
    fn printing_shows_no_cookie_value() {
        let cookies = Cookies::new();
        cookies.receive(["sid=s3cr3t"], &target(), NOW);
        let printed = format!("{cookies:?}");
        assert!(
            printed.contains("count: 1") && !printed.contains("s3cr3t"),
            "{printed}"
        );
    }

    #[test]
    fn a_loaded_jar_leaves_out_the_expired() {
        let store = MemoryCookies::default();
        let cookies = Cookies::new();
        cookies.receive(["a=1; Max-Age=10", "b=2"], &target(), NOW);
        cookies.save_if_changed(&store).unwrap();
        let loaded = Cookies::load(&store, NOW + 20_000).unwrap();
        let names: Vec<String> = loaded
            .jar()
            .cookies()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(names, ["b"]);
    }
}
