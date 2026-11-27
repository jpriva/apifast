//! Captures of scope `environment` and `collection` (docs/08), and what
//! scripts asked to `persist`: kept as runtime values at once, and written
//! to the active environment or to the collection's file only when the
//! user agrees. A value for a secret environment variable goes to the
//! vault, never to the file; the collection keeps no secret values, so a
//! secret one stays a runtime value.

use crate::messages::Say as _;
use std::path::{Path, PathBuf};

use apifast_app::{CaptureOutcome, VariableChange, VariableScope, save_environment};
use apifast_domain::{CaptureScope, Variable, VariableValue};

use super::{PendingCaptures, State};

/// Values captured for the active environment and for the collection,
/// waiting for the user.
#[derive(Debug, Clone, Default)]
pub(super) struct Captured {
    /// The environment the values of `environment` are for.
    environment_name: Option<String>,
    /// Name, value and whether the variable is a secret, in capture order.
    environment: Vec<(String, VariableValue, bool)>,
    /// Name and value of the collection's, in capture order.
    collection: Vec<(String, VariableValue)>,
    /// The global environment the values of `global` are for.
    global_name: Option<String>,
    /// Name and value of the global environment's, in order.
    global: Vec<(String, VariableValue)>,
}

impl Captured {
    /// What the interface sees: no values, since some may be secrets.
    pub(super) fn view(&self) -> PendingCaptures {
        PendingCaptures {
            environment: self.environment_name.clone().unwrap_or_default(),
            names: self
                .environment
                .iter()
                .map(|(name, ..)| name.clone())
                .collect(),
            collection: self
                .collection
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            global_environment: self.global_name.clone().unwrap_or_default(),
            global: self.global.iter().map(|(name, _)| name.clone()).collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.environment.is_empty() && self.collection.is_empty() && self.global.is_empty()
    }
}

/// The variables scripts asked to keep in their file
/// (`bru.setEnvVar(name, value, {persist: true})` and
/// `bru.setCollectionVar(…)`), as captures of scope `environment` or
/// `collection`, so they wait for the user the same way.
pub(super) fn persisted(changes: &[VariableChange]) -> Vec<CaptureOutcome> {
    (changes.iter())
        .filter(|c| c.persist)
        .filter_map(|c| {
            let scope = match c.scope {
                VariableScope::Environment => CaptureScope::Environment,
                VariableScope::Collection => CaptureScope::Collection,
                _ => return None,
            };
            Some(CaptureOutcome {
                variable: c.name.clone(),
                scope,
                value: Ok(c.value.clone()?),
                secret: false,
            })
        })
        .collect()
}

/// `name` set to `value` in `list`, or added to it.
fn set(list: &mut Vec<Variable>, name: String, value: VariableValue) {
    match list.iter_mut().find(|v| v.name == name) {
        Some(variable) => variable.value = Some(value),
        None => list.push(Variable {
            name,
            value: Some(value),
            enabled: true,
            secret: false,
        }),
    }
}

impl State {
    /// Notes the `environment` and `collection` captures of a send, adding
    /// to those still waiting. Without an active environment there is
    /// nowhere to write the former: they stay runtime values. A secret
    /// one for the collection does too.
    pub(super) fn note_captures(&mut self, root: &PathBuf, captures: &[CaptureOutcome]) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let value = |c: &CaptureOutcome| c.value.clone().ok();
        let environment: Vec<_> = (captures.iter())
            .filter(|c| c.scope == CaptureScope::Environment)
            .filter_map(|c| Some((c.variable.clone(), value(c)?, c.secret)))
            .collect();
        let collection: Vec<_> = (captures.iter())
            .filter(|c| c.scope == CaptureScope::Collection && !c.secret)
            .filter_map(|c| Some((c.variable.clone(), value(c)?)))
            .collect();
        let mut pending = open.captured.take().unwrap_or_default();
        // Values for another environment than the active one are dropped.
        if pending.environment_name != open.environment {
            pending.environment.clear();
            pending.environment_name.clone_from(&open.environment);
        }
        if open.environment.is_some() {
            for (name, value, secret) in environment {
                pending.environment.retain(|(n, ..)| *n != name);
                pending.environment.push((name, value, secret));
            }
        }
        for (name, value) in collection {
            pending.collection.retain(|(n, _)| *n != name);
            pending.collection.push((name, value));
        }
        open.captured = (!pending.is_empty()).then_some(pending);
    }

    /// Notes the values scripts asked to keep in the workspace's active
    /// global environment (`bru.setGlobalEnvVar(…, {persist: true})`),
    /// adding to those still waiting. Without an active one they stay
    /// runtime values.
    pub(super) fn note_global(&mut self, root: &PathBuf, changes: &[VariableChange]) {
        let active = (self.workspace.as_ref()).and_then(|w| w.global_environment.clone());
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let Some(active) = active else {
            return;
        };
        let values: Vec<_> = (changes.iter())
            .filter(|c| c.scope == VariableScope::Global && c.persist)
            .filter_map(|c| Some((c.name.clone(), c.value.clone()?)))
            .collect();
        if values.is_empty() {
            return;
        }
        let mut pending = open.captured.take().unwrap_or_default();
        if pending.global_name.as_deref() != Some(active.as_str()) {
            pending.global.clear();
            pending.global_name = Some(active);
        }
        for (name, value) in values {
            pending.global.retain(|(n, _)| *n != name);
            pending.global.push((name, value));
        }
        open.captured = Some(pending);
    }

    /// Writes the waiting values for the global environment into its file.
    /// A variable it declares secret is left as it is.
    fn write_global_captures(&mut self, name: &str, values: Vec<(String, VariableValue)>) {
        let files = (self.workspace.as_ref()).map(|w| w.global_files.clone());
        let Some(mut environment) =
            files.and_then(|files| files.iter().find(|e| e.name == name).cloned())
        else {
            let errors = &self.strings.errors;
            self.snapshot.error = Some(crate::strings::fill(
                errors.environment_gone,
                &[("name", &name)],
            ));
            return;
        };
        for (variable, value) in values {
            let secret = (environment.variables.iter()).any(|v| v.name == variable && v.secret);
            if !secret {
                set(&mut environment.variables, variable, value);
            }
        }
        self.global_environment(&super::GlobalCommand::Save(Box::new(environment)));
    }

    /// Writes the waiting captures of the collection at `root` to their
    /// environment and to the collection (`save`), or forgets them.
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
        if !pending.environment.is_empty()
            && let Err(message) = self.write_captures(root, &pending)
        {
            self.snapshot.error = Some(message);
        }
        if !pending.collection.is_empty() {
            self.write_collection_captures(root, pending.collection);
        }
        if let Some(name) = pending.global_name.filter(|_| !pending.global.is_empty()) {
            self.write_global_captures(&name, pending.global);
        }
    }

    /// Writes the waiting values of the collection into its file. A
    /// variable the file declares secret is left as it is: the collection
    /// keeps no secret values.
    fn write_collection_captures(&mut self, root: &Path, values: Vec<(String, VariableValue)>) {
        self.save_defaults(root, None, |defaults| {
            for (name, value) in values {
                let secret = (defaults.variables.iter()).any(|v| v.name == name && v.secret);
                if !secret {
                    set(&mut defaults.variables, name, value);
                }
            }
        });
    }

    fn write_captures(&mut self, root: &Path, pending: &Captured) -> Result<(), String> {
        let errors = &self.strings.errors;
        let open = (self.open.get(root)).ok_or(errors.collection_not_loaded)?;
        let repo = open.repo.clone();
        let name = pending.environment_name.clone().unwrap_or_default();
        let mut environment = (open.environment_files.iter())
            .find(|e| e.name == name)
            .cloned()
            .ok_or_else(|| crate::strings::fill(errors.environment_gone, &[("name", &name)]))?;
        let declared_secret = |name: &str| {
            environment
                .variables
                .iter()
                .any(|v| v.name == name && v.secret)
        };
        let (secret, plain): (Vec<_>, Vec<_>) = (pending.environment.iter())
            .cloned()
            .partition(|(name, _, secret)| *secret || declared_secret(name));
        if !plain.is_empty() {
            for (name, value, _) in plain {
                set(&mut environment.variables, name, value);
            }
            save_environment(&*repo, &environment).map_err(|e| e.say(self.strings))?;
        }
        if !secret.is_empty() {
            // The vault belongs to the focused collection.
            self.focus = Some(root.to_path_buf());
            let vault = self.vault_or_new()?;
            for (variable, value, _) in secret {
                vault
                    .set_local(&name, &variable, &value.data)
                    .map_err(|e| e.say(self.strings))?;
            }
        }
        self.reload_environments(root);
        self.refresh_collection(root);
        Ok(())
    }
}
