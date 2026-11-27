//! The vault in the core: what the interface sees of it (without opening any
//! value) and the commands that store, show and forget secrets and keys.

use crate::messages::Say as _;
use crate::strings::Strings;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use apifast_domain::{Environment, Secret};
use apifast_vault::{Orphan, SecretKind, VaultSecrets};

use super::{Message, State, TabKey};
use crate::system::SystemRandom;

/// How long a revealed value or the shown collection key stays on screen.
pub const SHOWN_FOR: Duration = Duration::from_secs(15);

/// Where the value of a secret variable is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStatus {
    /// On this machine only.
    Local,
    /// In the collection, and this machine has its key.
    Shared,
    /// In the collection, but this machine does not have its key.
    Locked,
    /// Nowhere yet.
    Missing,
}

/// A secret variable and where its value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretState {
    /// Its environment.
    pub environment: String,
    /// Its name.
    pub name: String,
    /// Where its value is.
    pub status: SecretStatus,
}

/// A value shown on request, for a while.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revealed {
    /// Its environment.
    pub environment: String,
    /// Its variable.
    pub name: String,
    /// The value.
    pub value: Secret,
}

/// What the interface sees of the vault. Built from names only: no value is
/// opened and the keyring is not asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultView {
    /// Every secret variable of the environments and where its value is.
    pub secrets: Vec<SecretState>,
    /// Stored values no secret variable uses.
    pub orphans: Vec<Orphan>,
    /// Whether the collection shares secrets (it has a challenge).
    pub shares: bool,
    /// Whether this machine has the collection's key.
    pub has_key: bool,
    /// Whether the master key is in a file rather than the keyring.
    pub master_in_file: bool,
    /// A value shown on request.
    pub revealed: Option<Revealed>,
    /// The collection's key, as people pass it, shown on request.
    pub shareable_key: Option<Secret>,
}

impl VaultView {
    /// Where the value of secret `name` of `environment` is.
    pub fn status(&self, environment: &str, name: &str) -> SecretStatus {
        self.secrets
            .iter()
            .find(|s| s.environment == environment && s.name == name)
            .map_or(SecretStatus::Missing, |s| s.status)
    }
}

/// What the interface asks of the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultCommand {
    /// Stores the value of a secret, on this machine or shared.
    Set {
        /// Its environment.
        environment: String,
        /// Its variable.
        name: String,
        /// The value.
        value: Secret,
        /// Shared with the team (sealed with the collection's key) rather
        /// than kept on this machine.
        shared: bool,
    },
    /// Shows the value of a secret for a while.
    Reveal {
        /// Its environment.
        environment: String,
        /// Its variable.
        name: String,
    },
    /// Hides what is shown: a value or the collection's key.
    Hide,
    /// Forgets a stored value, such as an orphan.
    Forget(Orphan),
    /// Shows the collection's key, to give it to the team.
    ShowKey,
    /// Takes the collection's key given by the team.
    ImportKey(Secret),
    /// Replaces the collection's key with a new one, which is then shown.
    RotateKey,
}

/// Every secret variable of `environments`, without value.
fn missing(environments: &[Environment]) -> Vec<SecretState> {
    environments
        .iter()
        .flat_map(|env| {
            env.variables
                .iter()
                .filter(|v| v.secret)
                .map(|var| SecretState {
                    environment: env.name.clone(),
                    name: var.name.clone(),
                    status: SecretStatus::Missing,
                })
        })
        .collect()
}

/// The view of `vault` for `environments`.
pub(super) fn view(
    vault: &VaultSecrets,
    environments: &[Environment],
    master_in_file: bool,
    strings: &Strings,
) -> Result<VaultView, String> {
    let error = |e: apifast_vault::KeyStoreError| e.say(strings);
    let stored = vault.stored().map_err(error)?;
    let has_key = vault.has_collection_key();
    let secrets = environments
        .iter()
        .flat_map(|env| {
            let stored = &stored;
            env.variables.iter().filter(|v| v.secret).map(move |var| {
                let kind = stored
                    .iter()
                    .find(|(e, n, _)| *e == env.name && *n == var.name)
                    .map(|(_, _, kind)| *kind);
                let status = match kind {
                    Some(SecretKind::Local) => SecretStatus::Local,
                    Some(SecretKind::Shared) if has_key => SecretStatus::Shared,
                    Some(SecretKind::Shared) => SecretStatus::Locked,
                    None => SecretStatus::Missing,
                };
                SecretState {
                    environment: env.name.clone(),
                    name: var.name.clone(),
                    status,
                }
            })
        })
        .collect();
    Ok(VaultView {
        secrets,
        orphans: vault.orphans(environments).map_err(error)?,
        shares: vault.shares_secrets().map_err(error)?,
        has_key,
        master_in_file,
        revealed: None,
        shareable_key: None,
    })
}

impl State {
    /// The focused collection's vault, if it has one (a collection id).
    fn vault(&self) -> Option<Arc<VaultSecrets>> {
        self.focused()?.vault.clone()
    }

    /// The focused collection's view of its vault, to change what it shows.
    fn view_mut(&mut self) -> Option<&mut VaultView> {
        Some(&mut self.focused_mut()?.vault_view)
    }

    /// Builds again the view of the vault of the collection at `root`,
    /// keeping what is shown.
    pub(super) fn refresh_vault_of(&mut self, root: &Path) {
        let master_in_file = apifast_vault::KeyFile::in_dir(&self.data_dir).holds_key();
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let shown = (
            open.vault_view.revealed.take(),
            open.vault_view.shareable_key.take(),
        );
        let view = match &open.vault {
            Some(vault) => view(vault, &open.environment_files, master_in_file, self.strings),
            // No vault yet: every secret is without value.
            None => Ok(VaultView {
                secrets: missing(&open.environment_files),
                master_in_file,
                ..VaultView::default()
            }),
        };
        match view {
            Ok(view) => open.vault_view = view,
            Err(e) => self.snapshot.error = Some(e),
        }
        (open.vault_view.revealed, open.vault_view.shareable_key) = shown;
        // Shown even with no collection focused.
        self.snapshot.vault.master_in_file = master_in_file;
    }

    /// Runs a vault command on the focused collection; its error goes on
    /// screen.
    pub(super) fn vault_command(&mut self, command: VaultCommand) {
        if let Err(e) = self.try_vault_command(command) {
            self.snapshot.error = Some(e);
        }
        if let Some(root) = self.focus.clone() {
            self.refresh_vault_of(&root);
        }
    }

    fn try_vault_command(&mut self, command: VaultCommand) -> Result<(), String> {
        let strings = self.strings;
        let error = |e: apifast_vault::KeyStoreError| e.say(strings);
        let none = strings.errors.no_secrets_yet;
        match command {
            VaultCommand::Set {
                environment,
                name,
                value,
                shared,
            } => {
                let vault = self.vault_or_new()?;
                if shared {
                    vault.share(&environment, &name, value.expose())
                } else {
                    vault.set_local(&environment, &name, value.expose())
                }
                .map_err(error)?;
                self.snapshot.error = None;
            }
            VaultCommand::Reveal { environment, name } => {
                let vault = self.vault().ok_or(none)?;
                let value = vault.reveal(&environment, &name).map_err(error)?;
                if let Some(view) = self.view_mut() {
                    view.revealed = value.map(|value| Revealed {
                        environment,
                        name,
                        value,
                    });
                }
                self.hide_later();
            }
            VaultCommand::Hide => {
                if let Some(view) = self.view_mut() {
                    view.revealed = None;
                    view.shareable_key = None;
                }
            }
            VaultCommand::Forget(orphan) => {
                let vault = self.vault().ok_or(none)?;
                vault
                    .forget(&orphan.environment, &orphan.name, orphan.kind)
                    .map_err(error)?;
            }
            VaultCommand::ShowKey => {
                let vault = self.vault().ok_or(none)?;
                let key = vault.export_collection_key().map_err(error)?;
                if let Some(view) = self.view_mut() {
                    view.shareable_key = key.map(Secret::new);
                }
                self.hide_later();
            }
            VaultCommand::ImportKey(text) => {
                let vault = self.vault_or_new()?;
                vault.import_collection_key(text.expose()).map_err(error)?;
                self.snapshot.error = None;
            }
            VaultCommand::RotateKey => {
                let vault = self.vault().ok_or(none)?;
                let key = vault.rotate_collection_key().map_err(error)?;
                if let Some(view) = self.view_mut() {
                    view.shareable_key = Some(Secret::new(key));
                }
                self.hide_later();
            }
        }
        Ok(())
    }

    /// The focused collection's vault, giving the collection an id first if
    /// it has none: its first secret is being stored. What apifast kept
    /// under the collection's path (session, history, responses) moves to
    /// the id, and the collection is loaded again with its tabs.
    pub(super) fn vault_or_new(&mut self) -> Result<Arc<VaultSecrets>, String> {
        if let Some(vault) = self.vault() {
            return Ok(vault);
        }
        let none = self.strings.errors.no_collection_open;
        let root: PathBuf = self.focus.clone().ok_or(none)?;
        let repo = self.focused().ok_or(none)?.repo.clone();
        let id = repo
            .ensure_id(&SystemRandom::default())
            .map_err(|e| e.say(self.strings))?;
        // Its tabs come back after loading it again.
        let tabs: Vec<TabKey> = (self.snapshot.tabs.iter())
            .filter(|tab| tab.root == root)
            .map(super::Tab::key)
            .collect();
        let active = self.snapshot.active.clone();
        self.unload(&root);
        apifast_history::dirs::move_to_id(&self.data_dir, &root, id.as_str()).map_err(|why| {
            crate::strings::fill(self.strings.errors.data_not_moved, &[("why", &why)])
        })?;
        self.load(&root)?;
        for key in &tabs {
            self.restore_tab(key);
        }
        if active.as_ref().is_some_and(|key| self.tab(key).is_some()) {
            self.snapshot.active = active;
        }
        self.focus = Some(root);
        self.vault()
            .ok_or_else(|| self.strings.errors.vault_unavailable.to_owned())
    }

    /// Hides what is shown after [`SHOWN_FOR`].
    pub(super) fn hide_later(&mut self) {
        self.shown += 1;
        let shown = self.shown;
        let messages = self.messages.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SHOWN_FOR).await;
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::HideShown(shown));
            }
        });
    }

    /// Hides what was shown, unless something was shown since.
    pub(super) fn hide_shown(&mut self, shown: u64) {
        if self.shown == shown {
            for open in self.open.values_mut() {
                open.vault_view.revealed = None;
                open.vault_view.shareable_key = None;
            }
            self.snapshot.global_vault.revealed = None;
        }
    }
}
