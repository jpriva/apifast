//! `apifast vault`: secrets and the collection's key from the command line.
//!
//! Values and keys are read from standard input, never from arguments: those
//! end up in the shell's history and in the process list.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use apifast_app::{CollectionRepository, EnvironmentRepository};
use apifast_storage_fs::FsCollection;
use apifast_vault::{SecretKind, SystemKeyring, VaultSecrets};
use clap::Subcommand;

use crate::{locate, system};

/// `apifast vault`.
#[derive(clap::Args)]
pub struct VaultArgs {
    #[command(subcommand)]
    command: VaultCommand,
    /// The collection folder, or a folder inside it.
    #[arg(long, global = true, default_value = ".")]
    collection: PathBuf,
}

#[derive(Subcommand)]
enum VaultCommand {
    /// List each secret variable and where its value is (names only).
    Status,
    /// Store the value of a secret, read from standard input:
    /// `printf %s "$TOKEN" | apifast vault set prod token`.
    Set {
        /// Its environment.
        environment: String,
        /// Its variable, marked `secret: true` in the environment.
        name: String,
        /// Share it with the team: sealed with the collection's key in
        /// `.apifast/secrets/`, which goes to git.
        #[arg(long)]
        shared: bool,
    },
    /// The collection's key.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Print the collection's key, to give it to the team.
    Show,
    /// Take the key the team gave, read from standard input.
    Import,
    /// Make a new key and seal every shared secret again; prints it.
    Rotate,
}

/// The keyring to use: none when `APIFAST_NO_KEYRING` is set (CI, tests),
/// so the master key goes to its file.
pub fn keyring() -> SystemKeyring {
    if std::env::var_os("APIFAST_NO_KEYRING").is_some() {
        SystemKeyring::off()
    } else {
        SystemKeyring::default()
    }
}

/// Runs `apifast vault`.
pub fn run(args: &VaultArgs) -> Result<()> {
    let root = locate::collection_root(&args.collection)?;
    let repo = Arc::new(FsCollection::open(&root)?);
    match &args.command {
        VaultCommand::Status => status(&root, &repo),
        VaultCommand::Set {
            environment,
            name,
            shared,
        } => {
            let declared = repo.environments()?.into_iter().any(|env| {
                env.name == *environment
                    && env.variables.iter().any(|v| v.secret && v.name == *name)
            });
            if !declared {
                bail!(
                    "`{name}` is not a secret of environment `{environment}`: mark it `secret: true` first"
                );
            }
            let value = read_stdin("Value")?;
            let vault = vault(&root, &repo, true)?.context("cannot open the vault")?;
            if *shared {
                vault.share(environment, name, &value)?;
            } else {
                vault.set_local(environment, name, &value)?;
            }
            eprintln!("stored `{name}` of `{environment}`");
            Ok(())
        }
        VaultCommand::Key { command } => key(command, &root, &repo),
    }
}

fn key(command: &KeyCommand, root: &Path, repo: &Arc<FsCollection>) -> Result<()> {
    match command {
        KeyCommand::Show => {
            let vault = vault(root, repo, false)?.context("this collection shares no secrets")?;
            let key = vault
                .export_collection_key()?
                .context("this machine does not have the collection's key")?;
            println!("{key}");
        }
        KeyCommand::Import => {
            let text = read_stdin("Collection key")?;
            let vault = vault(root, repo, true)?.context("cannot open the vault")?;
            vault.import_collection_key(&text)?;
            eprintln!("the collection's key is stored");
        }
        KeyCommand::Rotate => {
            let vault = vault(root, repo, false)?.context("this collection shares no secrets")?;
            let key = vault.rotate_collection_key()?;
            eprintln!("new key; commit the changed files and give it to your team:");
            println!("{key}");
        }
    }
    Ok(())
}

/// The collection's vault. With `create`, a collection without an id gets
/// one, and what apifast kept under its path moves to it.
fn vault(root: &Path, repo: &Arc<FsCollection>, create: bool) -> Result<Option<VaultSecrets>> {
    let data_dir = apifast_history::dirs::data_dir();
    let id = match repo.collection()?.id {
        Some(id) => id,
        None if create => {
            let id = repo.ensure_id(&system::SystemRandom::new())?;
            apifast_history::dirs::move_to_id(&data_dir, root, id.as_str())
                .context("cannot move this collection's data to its new id")?;
            id
        }
        None => return Ok(None),
    };
    Ok(Some(VaultSecrets::for_collection(
        &data_dir,
        id.as_str(),
        repo.clone(),
        keyring(),
    )?))
}

fn status(root: &Path, repo: &Arc<FsCollection>) -> Result<()> {
    let environments = repo.environments()?;
    let vault = vault(root, repo, false)?;
    let stored = match &vault {
        Some(vault) => vault.stored()?,
        None => Vec::new(),
    };
    let has_key = vault.as_ref().is_some_and(VaultSecrets::has_collection_key);
    for env in &environments {
        for var in env.variables.iter().filter(|v| v.secret) {
            let kind = stored
                .iter()
                .find(|(e, n, _)| *e == env.name && *n == var.name)
                .map(|(_, _, kind)| *kind);
            let status = match kind {
                Some(SecretKind::Local) => "stored on this machine",
                Some(SecretKind::Shared) if has_key => "shared with the team",
                Some(SecretKind::Shared) => "locked: this machine lacks the collection key",
                None => "no value",
            };
            println!("{}\t{}\t{status}", env.name, var.name);
        }
    }
    if let Some(vault) = &vault {
        for orphan in vault.orphans(&environments)? {
            println!(
                "{}\t{}\tnot used by any secret variable",
                orphan.environment, orphan.name
            );
        }
    }
    Ok(())
}

/// A value from standard input, without its final newline. At a terminal,
/// asks for it first.
fn read_stdin(what: &str) -> Result<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("{what} (then Enter and Ctrl+D; it is shown as you type):");
    }
    let mut text = String::new();
    stdin
        .read_to_string(&mut text)
        .context("cannot read standard input")?;
    let text = text
        .strip_suffix('\n')
        .map_or(text.as_str(), |t| t.strip_suffix('\r').unwrap_or(t));
    let text = text.to_owned();
    if text.is_empty() {
        bail!("nothing was read from standard input");
    }
    Ok(text)
}
