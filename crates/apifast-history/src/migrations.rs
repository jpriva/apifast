//! What a newer version of apifast does, once, to the data an older one left
//! (docs/13, "Actualizar"). The data directory keeps the newest version that
//! ran in `last-version`. When a newer one starts, the migrations of the
//! versions between them run, in order, after what they touch is copied to
//! `backups/<from>-<to>/`. An older one changes nothing: it only says so.
//!
//! There is no migration yet: [`MIGRATIONS`] is where the first goes, with
//! the version that needs it. Collections are never migrated here; they are
//! the user's files, and the storage keeps the keys it does not know.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The file, in the data directory, with the newest version that ran.
const MARKER: &str = "last-version";
/// The folder, in the data directory, with the copies made before migrating.
const BACKUPS: &str = "backups";

/// Where apifast keeps what it writes outside collections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// The data directory: history, responses, sessions, the vault's files.
    pub data: PathBuf,
    /// The configuration directory: settings and keymap.
    pub config: PathBuf,
}

/// A path a migration changes, relative to one of the [`Dirs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touched {
    /// Under the data directory.
    Data(&'static str),
    /// Under the configuration directory.
    Config(&'static str),
}

/// A change to apifast's data that a version needs once.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// The version that needs it: it runs when updating to it or past it.
    pub version: &'static str,
    /// What it does, for the user.
    pub what: &'static str,
    /// What it changes, copied before it runs.
    pub touches: &'static [Touched],
    /// The change.
    pub run: fn(&Dirs) -> io::Result<()>,
}

/// The migrations of every version, oldest first.
pub const MIGRATIONS: &[Migration] = &[];

/// What starting this version did to the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing was there: the first run.
    FirstRun,
    /// The same version ran last.
    Same,
    /// A newer version than the last one: what ran (maybe nothing), and
    /// where the copies went, if anything ran.
    Updated {
        /// The version that ran last.
        from: String,
        /// The migrations that ran, by what they do.
        ran: Vec<&'static str>,
        /// The copies of what they touched.
        backup: Option<PathBuf>,
    },
    /// An older version than the last one: nothing changed.
    Older {
        /// The newest version that ran.
        newest: String,
    },
}

/// Why the data could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// A version is not `major.minor.patch`.
    #[error("`{0}` is not a version")]
    Version(String),
    /// Reading, copying or migrating failed.
    #[error("{what}: {source}")]
    Io {
        /// What was being done.
        what: String,
        /// Why it failed.
        source: io::Error,
    },
}

fn io_error(what: impl Into<String>) -> impl FnOnce(io::Error) -> MigrationError {
    let what = what.into();
    move |source| MigrationError::Io { what, source }
}

/// `major.minor.patch`, without what follows a `-` or `+`.
fn parse(version: &str) -> Result<(u64, u64, u64), MigrationError> {
    let core = version.trim().split(['-', '+']).next().unwrap_or_default();
    let mut parts = core.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) => Ok((major, minor, patch)),
        _ => Err(MigrationError::Version(version.to_owned())),
    }
}

/// Prepares the data for `version`, with [`MIGRATIONS`].
///
/// # Errors
///
/// If the marker cannot be read or written, or a copy or a migration fails:
/// then this version should not go on with the data as it is.
pub fn prepare(dirs: &Dirs, version: &str) -> Result<Outcome, MigrationError> {
    prepare_with(dirs, version, MIGRATIONS)
}

/// [`prepare`] with `migrations`.
///
/// # Errors
///
/// As [`prepare`].
pub fn prepare_with(
    dirs: &Dirs,
    version: &str,
    migrations: &[Migration],
) -> Result<Outcome, MigrationError> {
    let this = parse(version)?;
    let marker = dirs.data.join(MARKER);
    let last = match fs::read_to_string(&marker) {
        Ok(text) => Some(text.trim().to_owned()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_error("reading the last version")(e)),
    };
    // Data without a marker is from before there was one: 0.0.0.
    let from = match last {
        Some(last) => last,
        None if has_data(dirs) => "0.0.0".to_owned(),
        None => {
            write_marker(&marker, version)?;
            return Ok(Outcome::FirstRun);
        }
    };
    let last = parse(&from)?;
    if last == this {
        return Ok(Outcome::Same);
    }
    if last > this {
        return Ok(Outcome::Older { newest: from });
    }
    let mut due = Vec::new();
    for migration in migrations {
        let at = parse(migration.version)?;
        if at > last && at <= this {
            due.push(migration);
        }
    }
    let backup = if due.is_empty() {
        None
    } else {
        Some(back_up(dirs, &from, version, &due)?)
    };
    let mut ran = Vec::new();
    for migration in due {
        (migration.run)(dirs).map_err(io_error(format!(
            "migrating to {}: {}",
            migration.version, migration.what
        )))?;
        ran.push(migration.what);
    }
    // Only once everything ran: a failure runs it all again next time.
    write_marker(&marker, version)?;
    Ok(Outcome::Updated { from, ran, backup })
}

fn has_data(dirs: &Dirs) -> bool {
    [&dirs.data, &dirs.config]
        .iter()
        .any(|dir| fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()))
}

fn write_marker(marker: &Path, version: &str) -> Result<(), MigrationError> {
    let write = || {
        if let Some(dir) = marker.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(marker, format!("{version}\n"))
    };
    write().map_err(io_error("writing the last version"))
}

/// Copies what `due` touches to `backups/<from>-<to>/data|config/`.
fn back_up(
    dirs: &Dirs,
    from: &str,
    to: &str,
    due: &[&Migration],
) -> Result<PathBuf, MigrationError> {
    let backup = dirs.data.join(BACKUPS).join(format!("{from}-{to}"));
    for touched in due.iter().flat_map(|m| m.touches) {
        let (source, copy) = match touched {
            Touched::Data(path) => (dirs.data.join(path), backup.join("data").join(path)),
            Touched::Config(path) => (dirs.config.join(path), backup.join("config").join(path)),
        };
        if source.exists() {
            copy_all(&source, &copy)
                .map_err(io_error(format!("backing up {}", source.display())))?;
        }
    }
    Ok(backup)
}

/// Copies `source` (a file, or a folder and everything in it) to `copy`.
fn copy_all(source: &Path, copy: &Path) -> io::Result<()> {
    if source.is_dir() {
        fs::create_dir_all(copy)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_all(&entry.path(), &copy.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(dir) = copy.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::copy(source, copy).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(root: &Path) -> Dirs {
        Dirs {
            data: root.join("data"),
            config: root.join("config"),
        }
    }

    fn marker(dirs: &Dirs) -> String {
        fs::read_to_string(dirs.data.join(MARKER)).unwrap_or_default()
    }

    fn rename_settings(dirs: &Dirs) -> io::Result<()> {
        fs::rename(dirs.config.join("old.yml"), dirs.config.join("new.yml"))
    }

    fn mark_history(dirs: &Dirs) -> io::Result<()> {
        fs::write(dirs.data.join("history/migrated"), "yes")
    }

    const TEST_MIGRATIONS: &[Migration] = &[
        Migration {
            version: "0.2.0",
            what: "settings renamed",
            touches: &[Touched::Config("old.yml")],
            run: rename_settings,
        },
        Migration {
            version: "0.3.0",
            what: "history marked",
            touches: &[Touched::Data("history")],
            run: mark_history,
        },
    ];

    #[test]
    fn the_first_run_and_the_same_version_change_nothing() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dirs = dirs(root.path());
        assert_eq!(
            prepare_with(&dirs, "0.1.0", TEST_MIGRATIONS).ok(),
            Some(Outcome::FirstRun)
        );
        assert_eq!(marker(&dirs), "0.1.0\n");
        assert_eq!(
            prepare_with(&dirs, "0.1.0", TEST_MIGRATIONS).ok(),
            Some(Outcome::Same)
        );
    }

    #[test]
    fn updating_runs_the_migrations_between_once_after_a_backup() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dirs = dirs(root.path());
        fs::create_dir_all(dirs.data.join("history")).unwrap_or_else(|e| panic!("{e}"));
        fs::write(dirs.data.join("history/a.jsonl"), "{}").unwrap_or_else(|e| panic!("{e}"));
        fs::create_dir_all(&dirs.config).unwrap_or_else(|e| panic!("{e}"));
        fs::write(dirs.config.join("old.yml"), "theme: dark").unwrap_or_else(|e| panic!("{e}"));

        // Data from before the marker counts as 0.0.0.
        let outcome =
            prepare_with(&dirs, "0.3.0", TEST_MIGRATIONS).unwrap_or_else(|e| panic!("{e}"));
        let backup = dirs.data.join("backups/0.0.0-0.3.0");
        assert_eq!(
            outcome,
            Outcome::Updated {
                from: "0.0.0".into(),
                ran: vec!["settings renamed", "history marked"],
                backup: Some(backup.clone()),
            }
        );
        assert!(
            dirs.config.join("new.yml").exists() && dirs.data.join("history/migrated").exists()
        );
        assert_eq!(
            fs::read_to_string(backup.join("config/old.yml"))
                .ok()
                .as_deref(),
            Some("theme: dark")
        );
        assert!(backup.join("data/history/a.jsonl").exists());
        assert_eq!(marker(&dirs), "0.3.0\n");

        // Once: the same version again does nothing.
        assert_eq!(
            prepare_with(&dirs, "0.3.0", TEST_MIGRATIONS).ok(),
            Some(Outcome::Same)
        );
    }

    #[test]
    fn only_the_migrations_after_the_last_version_run_and_an_update_without_any_needs_no_backup() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dirs = dirs(root.path());
        fs::create_dir_all(dirs.data.join("history")).unwrap_or_else(|e| panic!("{e}"));
        write_marker(&dirs.data.join(MARKER), "0.2.0").unwrap_or_else(|e| panic!("{e}"));
        let outcome =
            prepare_with(&dirs, "0.3.1", TEST_MIGRATIONS).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            matches!(&outcome, Outcome::Updated { ran, .. } if *ran == ["history marked"]),
            "{outcome:?}"
        );
        let outcome =
            prepare_with(&dirs, "0.4.0", TEST_MIGRATIONS).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            outcome,
            Outcome::Updated {
                from: "0.3.1".into(),
                ran: Vec::new(),
                backup: None,
            }
        );
    }

    #[test]
    fn an_older_version_changes_nothing_and_keeps_the_newest() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dirs = dirs(root.path());
        write_marker(&dirs.data.join(MARKER), "0.3.0").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            prepare_with(&dirs, "0.2.0", TEST_MIGRATIONS).ok(),
            Some(Outcome::Older {
                newest: "0.3.0".into()
            })
        );
        assert_eq!(marker(&dirs), "0.3.0\n");
    }

    #[test]
    fn a_failed_migration_runs_again_next_time() {
        fn fails(_: &Dirs) -> io::Result<()> {
            Err(io::Error::other("disk full"))
        }
        let failing = [Migration {
            version: "0.2.0",
            what: "fails",
            touches: &[],
            run: fails,
        }];
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dirs = dirs(root.path());
        write_marker(&dirs.data.join(MARKER), "0.1.0").unwrap_or_else(|e| panic!("{e}"));
        let error = prepare_with(&dirs, "0.2.0", &failing)
            .err()
            .map(|e| e.to_string());
        assert_eq!(
            error.as_deref(),
            Some("migrating to 0.2.0: fails: disk full")
        );
        assert_eq!(marker(&dirs), "0.1.0\n");
    }

    #[test]
    fn versions_are_major_minor_patch() {
        assert_eq!(parse("1.2.3").ok(), Some((1, 2, 3)));
        assert_eq!(parse("1.10.0-rc.1").ok(), Some((1, 10, 0)));
        assert!(parse("1.2").is_err() && parse("v1.2.3").is_err());
        assert!(parse("0.10.0").ok() > parse("0.9.9").ok());
    }
}
