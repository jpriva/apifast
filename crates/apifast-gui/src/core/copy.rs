//! "Copy as cURL": a request rendered as its send would render it, without
//! sending, as a command for the interface to put in the clipboard.

use crate::messages::Say as _;
use std::sync::Arc;

use apifast_app::{
    CopiedRequest, HistoryStore, NoPrompter, NoSecrets, SecretStore, SendOptions, SendPorts,
    render_for_copy,
};

use super::{State, TabKey};
use crate::system::{SystemClock, SystemRandom};

/// A text for the clipboard, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// Increases with every copy, so each is put in the clipboard once.
    pub id: u64,
    /// The language it is in.
    pub language: apifast_import::CodeLanguage,
    /// The text.
    pub text: String,
    /// What it could not fill in, or why there is nothing to copy.
    pub note: Option<String>,
}

impl State {
    /// Renders the request `key` (with its tab's draft, if open) as code in
    /// `language`.
    pub(super) fn copy_as(&mut self, key: &TabKey, language: apifast_import::CodeLanguage) {
        let id = self.next_revision();
        let Some(open) = self.open.get(&key.root) else {
            return;
        };
        let options = SendOptions {
            environment: open.environment.clone(),
            globals: open.globals.to_vec(),
            runtime: open.runtime.clone(),
            script_changes: open.script_changes.clone(),
            draft: self.tab(key).and_then(|tab| tab.draft.clone()),
            default_timeout_ms: Some(self.default_timeout_ms),
            ..SendOptions::default()
        };
        let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
            Some(vault) => vault.clone(),
            None => Arc::new(NoSecrets),
        };
        let (clock, random) = (SystemClock, SystemRandom::default());
        let ports = SendPorts {
            collections: &*open.repo,
            environments: &*open.repo,
            transport: &*open.transport,
            responses: &*open.responses,
            clock: &clock,
            random: &random,
            prompter: &NoPrompter,
            secrets: &*secrets,
            report: &|_| {},
            history: Some(&*open.history as &(dyn HistoryStore + Sync)),
            tokens: Some(open.tokens.as_ref()),
            authorizer: None,
            scripts: None,
        };
        let copied = match render_for_copy(&ports, &key.path, &options) {
            Ok(copied) => {
                let code = apifast_import::to_code(language, &copied.request, Some(&key.root));
                Copied {
                    id,
                    language,
                    text: code.text,
                    note: note(&copied, code.notes),
                }
            }
            Err(e) => Copied {
                id,
                language,
                text: String::new(),
                note: Some(e.say(self.strings)),
            },
        };
        self.snapshot.copied = Some(copied);
    }
}

/// What a copied request could not fill in, and what its code leaves to
/// the user.
fn note(copied: &CopiedRequest, code: Vec<String>) -> Option<String> {
    let mut missing: Vec<String> = copied.problems.iter().map(ToString::to_string).collect();
    missing.extend(code);
    if copied.missing_token {
        missing.push("no OAuth 2.0 token stored yet".into());
    }
    (!missing.is_empty()).then(|| missing.join("; "))
}
