//! Where apifast's log goes, and at which level (docs/14, "El registro").
//!
//! Every crate emits events with `tracing` (`tracing::info!`, `warn!`…);
//! this one decides where they go: the terminal, and optionally a file that
//! rotates when it grows past [`MAX_FILE_BYTES`]. Events of libraries (wgpu,
//! winit, hyper…) arrive too, `log` ones included.
//!
//! The filter is [`LogLevel`] for apifast's own crates and `warn` for the
//! libraries, unless `APIFAST_LOG` gives one in `tracing`'s syntax
//! (`debug`, `apifast_http=trace,wgpu=info`): then it rules, and
//! [`Logging::set_level`] does not change it.
//!
//! Some warnings of libraries that are not problems (a Vulkan driver that
//! cannot start here, a clipboard protocol the desktop lacks) are replaced by
//! an `info` event that explains them: [`explained`].
//!
//! Nothing here keeps secrets out of the log: what emits an event does,
//! as errors already do (they never carry secret values nor a URL's query).

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub mod explained;

use tracing_log::AsLog as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Registry, reload};

/// A log file larger than this is moved to `<name>.1` (replacing an older
/// one) and started again: at most twice this on disk.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// The crates whose events [`LogLevel`] sets, by their target prefix.
pub const OWN_CRATES: &[&str] = &[
    "apifast_app",
    "apifast_cli",
    "apifast_domain",
    "apifast_git",
    "apifast_grpc",
    "apifast_gui",
    "apifast_history",
    "apifast_http",
    "apifast_import",
    "apifast_log",
    "apifast_query",
    "apifast_script_js",
    "apifast_storage_fs",
    "apifast_template",
    "apifast_vault",
    "apifast_watch",
    "apifast_ws",
];

/// How much of apifast's own work is logged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Only what failed.
    Error,
    /// What failed and what may be wrong.
    #[default]
    Warn,
    /// Also what it did: a collection opened, a request sent.
    Info,
    /// Also the details, to find a problem.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevel {
    /// Every level, from the least to the most.
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    /// As settings and `APIFAST_LOG` write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// From [`Self::as_str`], in any case.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(text.trim()))
    }

    /// [`Self::filter`], ready to use.
    #[must_use]
    pub fn targets(self) -> Targets {
        self.filter().parse().unwrap_or_default()
    }

    /// The filter: this level for apifast's crates, `warn` (or `error`, if
    /// this is `error`) for the libraries.
    #[must_use]
    pub fn filter(self) -> String {
        let libraries = if self == Self::Error { "error" } else { "warn" };
        let mut filter = libraries.to_owned();
        for target in OWN_CRATES {
            filter.push(',');
            filter.push_str(target);
            filter.push('=');
            filter.push_str(self.as_str());
        }
        filter
    }
}

/// Where the log goes.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// `APIFAST_LOG`, if set: a filter in `tracing`'s syntax that rules
    /// over [`Self::level`].
    pub env: Option<String>,
    /// The level when `env` does not say.
    pub level: LogLevel,
    /// Write to the terminal (stderr), with colors if it is one.
    pub stderr: bool,
    /// Also write here, rotating at [`MAX_FILE_BYTES`].
    pub file: Option<PathBuf>,
}

/// Why the log could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// The log file cannot be opened.
    #[error("cannot open the log `{path}`: {source}")]
    File {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// Another subscriber was set up first.
    #[error("the log is already set up")]
    AlreadySet,
}

/// The log, once set up: changes its level while running.
pub struct Logging {
    filter: reload::Handle<Targets, Registry>,
    /// `APIFAST_LOG` gave the filter: settings do not change it.
    from_env: bool,
    /// What was wrong with `APIFAST_LOG`, if it could not be read.
    problem: Option<String>,
}

impl std::fmt::Debug for Logging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Logging")
            .field("from_env", &self.from_env)
            .field("problem", &self.problem)
            .finish_non_exhaustive()
    }
}

impl Logging {
    /// Logs apifast's crates at `level` from now on, unless `APIFAST_LOG`
    /// rules.
    pub fn set_level(&self, level: LogLevel) {
        if !self.from_env {
            let _ = self.filter.reload(level.targets());
            follow_log_level();
        }
    }

    /// Whether `APIFAST_LOG` sets the filter, so the level setting does not.
    #[must_use]
    pub const fn from_env(&self) -> bool {
        self.from_env
    }

    /// What was wrong with `APIFAST_LOG`, which was then ignored.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Sets the log up for the whole process, `log` records included.
///
/// # Errors
///
/// If the file cannot be opened, or a log was set up already.
pub fn init(options: &LogOptions) -> Result<Logging, LogError> {
    let (subscriber, logging) = subscriber(options)?;
    tracing::subscriber::set_global_default(subscriber).map_err(|_| LogError::AlreadySet)?;
    log::set_boxed_logger(Box::new(explained::Explained::new()))
        .map_err(|_| LogError::AlreadySet)?;
    follow_log_level();
    Ok(logging)
}

/// Lets through the `log` records `tracing` would keep, and no more: a
/// record below the filter is not even built.
fn follow_log_level() {
    log::set_max_level(tracing::level_filters::LevelFilter::current().as_log());
}

/// The subscriber [`init`] sets, without setting it: for tests.
///
/// # Errors
///
/// If the file cannot be opened.
pub fn subscriber(
    options: &LogOptions,
) -> Result<(impl tracing::Subscriber + Send + Sync + 'static, Logging), LogError> {
    let env = options
        .env
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty());
    let (filter, from_env, problem) = match env.map(str::parse::<Targets>) {
        Some(Ok(filter)) => (filter, true, None),
        Some(Err(e)) => (
            options.level.targets(),
            false,
            Some(format!(
                "APIFAST_LOG is not a filter ({e}); the level setting is used"
            )),
        ),
        None => (options.level.targets(), false, None),
    };
    let (filter, handle) = reload::Layer::new(filter);
    let stderr = options.stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_ansi(io::stderr().is_terminal())
    });
    let file = match &options.file {
        Some(path) => {
            let file =
                RotatingFile::open(path, MAX_FILE_BYTES).map_err(|source| LogError::File {
                    path: path.clone(),
                    source,
                })?;
            Some(
                tracing_subscriber::fmt::layer()
                    .with_writer(Mutex::new(file))
                    .with_ansi(false),
            )
        }
        None => None,
    };
    let subscriber = Registry::default().with(filter).with(stderr).with(file);
    let logging = Logging {
        filter: handle,
        from_env,
        problem,
    };
    Ok((subscriber, logging))
}

/// A file that starts again when it grows past its limit, keeping the
/// previous one as `<name>.1`.
#[derive(Debug)]
pub struct RotatingFile {
    path: PathBuf,
    max: u64,
    file: File,
    written: u64,
}

impl RotatingFile {
    /// Opens `path` to append, making its folder.
    ///
    /// # Errors
    ///
    /// If it cannot be opened.
    pub fn open(path: &Path, max: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            path: path.to_owned(),
            max,
            file,
            written,
        })
    }

    /// `<name>.1`.
    fn previous(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(".1");
        PathBuf::from(name)
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        fs::rename(&self.path, self.previous())?;
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.written = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.written > 0 && self.written.saturating_add(len) > self.max {
            self.rotate()?;
        }
        let n = self.file.write(buf)?;
        self.written = self
            .written
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_read_and_write_and_make_a_filter() {
        assert_eq!(LogLevel::parse(" DEBUG "), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("loud"), None);
        let filter = LogLevel::Info.filter();
        assert!(filter.starts_with("warn,apifast_app=info,"), "{filter}");
        assert!(filter.contains("apifast_gui=info"));
        assert!(LogLevel::Error.filter().starts_with("error,"));
        for level in LogLevel::ALL {
            assert!(level.filter().parse::<Targets>().is_ok(), "{level:?}");
        }
    }

    #[test]
    fn events_go_to_the_file_at_the_level_and_env_rules() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("logs/apifast.log");
        let options = LogOptions {
            level: LogLevel::Info,
            file: Some(path.clone()),
            ..LogOptions::default()
        };
        let (made, logging) = subscriber(&options).unwrap_or_else(|e| panic!("{e}"));
        tracing::subscriber::with_default(made, || {
            tracing::info!(target: "apifast_gui::core", status = 200, "request sent");
            tracing::debug!(target: "apifast_gui::core", "not at info");
            tracing::info!(target: "wgpu_core", "a library, below warn");
            logging.set_level(LogLevel::Debug);
            tracing::debug!(target: "apifast_http", "now at debug");
        });
        let text = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            text.contains("request sent") && text.contains("status=200"),
            "{text}"
        );
        assert!(
            !text.contains("not at info") && !text.contains("a library"),
            "{text}"
        );
        assert!(text.contains("now at debug"), "{text}");
        assert!(!text.contains("\u{1b}["), "no colors in the file");

        let ruled = LogOptions {
            env: Some("apifast_http=trace".into()),
            level: LogLevel::Error,
            ..LogOptions::default()
        };
        let (_, logging) = subscriber(&ruled).unwrap_or_else(|e| panic!("{e}"));
        assert!(logging.from_env() && logging.problem().is_none());
        let bad = LogOptions {
            env: Some("apifast_http=loudest".into()),
            ..LogOptions::default()
        };
        let (_, logging) = subscriber(&bad).unwrap_or_else(|e| panic!("{e}"));
        assert!(!logging.from_env());
        assert!(
            logging
                .problem()
                .is_some_and(|p| p.starts_with("APIFAST_LOG"))
        );
    }

    #[test]
    fn a_full_file_moves_to_dot_1_and_starts_again() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("apifast.log");
        let mut file = RotatingFile::open(&path, 10).unwrap_or_else(|e| panic!("{e}"));
        file.write_all(b"first 8\n")
            .unwrap_or_else(|e| panic!("{e}"));
        file.write_all(b"second\n")
            .unwrap_or_else(|e| panic!("{e}"));
        file.write_all(b"third\n").unwrap_or_else(|e| panic!("{e}"));
        let read = |p: &Path| fs::read_to_string(p).unwrap_or_default();
        assert_eq!(read(&path), "third\n");
        assert_eq!(read(&dir.path().join("apifast.log.1")), "second\n");
        // Opened again, it goes on from its size.
        drop(file);
        let mut file = RotatingFile::open(&path, 10).unwrap_or_else(|e| panic!("{e}"));
        file.write_all(b"fourth\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&path), "fourth\n");
        assert_eq!(read(&dir.path().join("apifast.log.1")), "third\n");
    }
}
