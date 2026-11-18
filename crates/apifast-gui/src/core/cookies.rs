//! Each loaded collection's cookie jar: shared with its transport, sealed on
//! disk when it changes, and edited from the interface (docs/07, "Cookies").

use std::sync::Arc;

use apifast_app::{Clock, CookieStore, Cookies};
use apifast_domain::Cookie;

use super::State;
use crate::system::SystemClock;

/// Where a cookie is in its jar: its domain, path and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieSlot {
    /// Its domain.
    pub domain: String,
    /// Its path.
    pub path: String,
    /// Its name.
    pub name: String,
}

impl CookieSlot {
    /// The slot of `cookie`.
    pub fn of(cookie: &Cookie) -> Self {
        Self {
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            name: cookie.name.clone(),
        }
    }
}

/// A change to the focused collection's cookie jar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieCommand {
    /// Puts `cookie` in the jar, in place of the one at `replaces` (when
    /// editing changed its domain, path or name) or of the one in its slot.
    Set {
        /// The cookie that was edited, if any.
        replaces: Option<CookieSlot>,
        /// The cookie.
        cookie: Box<Cookie>,
    },
    /// Removes one cookie.
    Remove(CookieSlot),
    /// Removes every cookie of a domain.
    RemoveDomain(String),
    /// Removes every cookie.
    Clear,
}

/// A collection's jar, loaded from its store; an unreadable store gives an
/// empty jar and why.
pub(super) fn load_jar(store: &dyn CookieStore) -> (Cookies, Option<String>) {
    match Cookies::load(store, SystemClock.unix_millis()) {
        Ok(cookies) => (cookies, None),
        Err(e) => (Cookies::new(), Some(format!("cookies: {e}"))),
    }
}

impl State {
    /// Applies `command` to the focused collection's jar and saves it.
    pub(super) fn cookie_command(&mut self, command: CookieCommand) {
        let Some(open) = (self.focus.as_ref()).and_then(|root| self.open.get(root)) else {
            return;
        };
        let now = SystemClock.unix_millis();
        open.cookies.edit(|jar| match command {
            CookieCommand::Set { replaces, cookie } => {
                if let Some(old) = replaces {
                    jar.remove(&old.domain, &old.path, &old.name);
                }
                jar.set(*cookie, now);
            }
            CookieCommand::Remove(slot) => {
                jar.remove(&slot.domain, &slot.path, &slot.name);
            }
            CookieCommand::RemoveDomain(domain) => {
                jar.remove_domain(&domain);
            }
            CookieCommand::Clear => jar.clear(),
        });
        self.sync_cookies();
    }

    /// Sends and stores cookies, or not, in every loaded collection.
    pub(super) fn set_cookie_policy(&mut self, send: bool, store: bool) {
        self.cookie_policy = (send, store);
        for open in self.open.values() {
            open.cookies.set_policy(send, store);
        }
    }

    /// Saves the jars that changed (after a send, a run or an edit) and
    /// refreshes what the interface sees of them. A jar that cannot be
    /// saved stays as it is in memory, and the error shows.
    pub(super) fn sync_cookies(&mut self) {
        let mut error = None;
        for open in self.open.values_mut() {
            match open.cookies.save_if_changed(&*open.cookie_store) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(e) => error = Some(format!("cookies: {e}")),
            }
            open.cookie_view = Arc::new(open.cookies.jar().cookies().to_vec());
        }
        if error.is_some() {
            self.snapshot.error = error;
        }
    }
}
