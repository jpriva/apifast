//! Captures of scope `environment` (docs/08): kept as runtime values at
//! once, and written to the active environment only when the user agrees.
//! A value for a secret variable goes to the vault, never to the file.

use crate::messages::Say as _;
use std::path::{Path, PathBuf};

use apifast_app::{CaptureOutcome, VariableChange, VariableScope, save_environment};
use apifast_domain::{CaptureScope, Variable, VariableValue};

use super::{PendingCaptures, State};

/// Values captured for the active environment, waiting for the user.
#[derive(Debug, Clone)]
pub(super) struct Captured {
    environment: String,
    /// Name, value and whether the variable is a secret, in capture order.
    values: Vec<(String, VariableValue, bool)>,
}

impl Captured {
    /// What the interface sees: no values, since some may be secrets.
    pub(super) fn view(&self) -> PendingCaptures {
        PendingCaptures {
            environment: self.environment.clone(),
            names: self.values.iter().map(|(name, ..)| name.clone()).collect(),
        }
    }
}

/// The environment variables scripts asked to keep in the file
/// (`bru.setEnvVar(name, value, {persist: true})`), as captures of scope
/// `environment`, so they wait for the user the same way.
pub(super) fn persisted(changes: &[VariableChange]) -> Vec<CaptureOutcome> {
    (changes.iter())
        .filter(|c| c.scope == VariableScope::Environment && c.persist)
        .filter_map(|c| {
            Some(CaptureOutcome {
                variable: c.name.clone(),
                scope: CaptureScope::Environment,
                value: Ok(c.value.clone()?),
                secret: false,
            })
        })
        .collect()
}

impl State {
    /// Notes the `environment` captures of a send, adding to those still
    /// waiting. Without an active environment there is nowhere to write
    /// them: they stay runtime values.
    pub(super) fn note_environment_captures(
        &mut self,
        root: &PathBuf,
        captures: &[CaptureOutcome],
    ) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let Some(environment) = open.environment.clone() else {
            return;
        };
        let values: Vec<(String, VariableValue, bool)> = (captures.iter())
            .filter(|c| c.scope == CaptureScope::Environment)
            .filter_map(|c| Some((c.variable.clone(), c.value.clone().ok()?, c.secret)))
            .collect();
        if values.is_empty() {
            return;
        }
        let pending = match open.captured.take() {
            Some(pending) if pending.environment == environment => pending,
            _ => Captured {
                environment,
                values: Vec::new(),
            },
        };
        let mut pending = pending;
        for (name, value, secret) in values {
            pending.values.retain(|(n, ..)| *n != name);
            pending.values.push((name, value, secret));
        }
        open.captured = Some(pending);
    }

    /// Writes the waiting captures of the collection at `root` to their
    /// environment (`save`), or forgets them.
    pub(super) fn save_captures(&mut self, root: &Path, save: bool) {
        let Some(pending) = self
            .open
            .get_mut(root)
            .and_then(|open| open.captured.take())
        else {
            return;
        };
        if !save {
            return;
        }
        if let Err(message) = self.write_captures(root, &pending) {
            self.snapshot.error = Some(message);
        }
    }

    fn write_captures(&mut self, root: &Path, pending: &Captured) -> Result<(), String> {
        let errors = &self.strings.errors;
        let open = (self.open.get(root)).ok_or(errors.collection_not_loaded)?;
        let repo = open.repo.clone();
        let mut environment = (open.environment_files.iter())
            .find(|e| e.name == pending.environment)
            .cloned()
            .ok_or_else(|| {
                crate::strings::fill(errors.environment_gone, &[("name", &pending.environment)])
            })?;
        let declared_secret = |name: &str| {
            environment
                .variables
                .iter()
                .any(|v| v.name == name && v.secret)
        };
        let (secret, plain): (Vec<_>, Vec<_>) = (pending.values.iter())
            .cloned()
            .partition(|(name, _, secret)| *secret || declared_secret(name));
        if !plain.is_empty() {
            for (name, value, _) in plain {
                match environment.variables.iter_mut().find(|v| v.name == name) {
                    Some(variable) => variable.value = Some(value),
                    None => environment.variables.push(Variable {
                        name,
                        value: Some(value),
                        enabled: true,
                        secret: false,
                    }),
                }
            }
            save_environment(&*repo, &environment).map_err(|e| e.say(self.strings))?;
        }
        if !secret.is_empty() {
            // The vault belongs to the focused collection.
            self.focus = Some(root.to_path_buf());
            let vault = self.vault_or_new()?;
            for (name, value, _) in secret {
                vault
                    .set_local(&pending.environment, &name, &value.data)
                    .map_err(|e| e.say(self.strings))?;
            }
        }
        self.reload_environments(root);
        self.refresh_collection(root);
        Ok(())
    }
}
