//! The interface session of a collection on disk: open tabs in
//! `session.yml`, and each unsaved draft as a request file in `drafts/`,
//! in the same format as the collection's own files.

use std::path::{Path, PathBuf};

use apifast_app::{RequestPath, Session, SessionStore, SessionTab, StoreError};
use apifast_domain::HttpRequest;
use oneyaml::Value;

use crate::yaml::{self, mapping, string};
use crate::{fs, read, write};

const SESSION_FILE: &str = "session.yml";
const DRAFTS_DIR: &str = "drafts";

/// The session of one collection, kept in its own folder (normally under
/// the user's data directory, outside the collection).
#[derive(Debug, Clone)]
pub struct FsSessionStore {
    dir: PathBuf,
}

impl FsSessionStore {
    /// A store in `dir`, created on the first save.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder it writes into.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Draft files are named by tab position, never by a path read from the
    /// session file, so a damaged file cannot make the store read or write
    /// elsewhere.
    fn draft_file(&self, index: usize) -> PathBuf {
        self.dir.join(DRAFTS_DIR).join(format!("{index}.yml"))
    }

    fn read_draft(&self, index: usize) -> Option<HttpRequest> {
        let text = std::fs::read_to_string(self.draft_file(index)).ok()?;
        match read::item(&oneyaml::from_str(&text).ok()?) {
            read::Item::Http(request) => Some(request),
            read::Item::Other { .. } => None,
        }
    }
}

impl SessionStore for FsSessionStore {
    fn load(&self) -> Result<Session, StoreError> {
        let text = match std::fs::read_to_string(self.dir.join(SESSION_FILE)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Session::default()),
            Err(e) => return Err(error("cannot read the session", &e)),
        };
        let doc: Value =
            oneyaml::from_str(&text).map_err(|e| error("the session is not valid", &e))?;
        let tabs = yaml::items(yaml::get(&doc, "tabs"))
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                let path = yaml::text(yaml::get(tab, "path"))?;
                // A draft that cannot be read is lost, not the whole session.
                let draft = yaml::boolean(yaml::get(tab, "draft"))
                    .unwrap_or(false)
                    .then(|| self.read_draft(index))
                    .flatten();
                Some(SessionTab {
                    path: RequestPath::new(path),
                    draft,
                })
            })
            .collect();
        Ok(Session {
            tabs,
            active: yaml::text(yaml::get(&doc, "active")).map(RequestPath::new),
            environment: yaml::text(yaml::get(&doc, "environment")),
        })
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        let drafts = self.dir.join(DRAFTS_DIR);
        std::fs::create_dir_all(&drafts).map_err(|e| error("cannot create the session", &e))?;

        // Drafts first, then the file that refers to them: a crash in
        // between leaves the previous session readable.
        let mut tabs = Vec::with_capacity(session.tabs.len());
        let mut kept = Vec::new();
        for (index, tab) in session.tabs.iter().enumerate() {
            if let Some(draft) = &tab.draft {
                let text = write::to_text(&write::new_request(draft))
                    .map_err(|e| error("cannot write a draft", &e))?;
                fs::write_atomic(&self.draft_file(index), text.as_bytes())
                    .map_err(|e| error("cannot write a draft", &e))?;
                kept.push(format!("{index}.yml"));
            }
            tabs.push(mapping([
                ("path", Some(string(tab.path.as_str()))),
                ("draft", tab.draft.is_some().then_some(Value::Bool(true))),
            ]));
        }
        let doc = mapping([
            ("tabs", Some(Value::from(tabs))),
            (
                "active",
                session.active.as_ref().map(|p| string(p.as_str())),
            ),
            ("environment", session.environment.clone().map(string)),
        ]);
        let text = oneyaml::to_string_with(&doc, &write::settings())
            .map_err(|e| error("cannot write the session", &e))?;
        fs::write_atomic(&self.dir.join(SESSION_FILE), text.as_bytes())
            .map_err(|e| error("cannot write the session", &e))?;

        // Drafts of tabs that are gone or saved.
        if let Ok(entries) = std::fs::read_dir(&drafts) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if !kept.iter().any(|k| *k.as_str() == *name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}

fn error(what: &str, e: &dyn std::fmt::Display) -> StoreError {
    StoreError(format!("{what}: {e}"))
}
