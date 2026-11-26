//! The application's settings, as `settings.yml` keeps them, in the
//! sections of the settings window: general, appearance, editor, network
//! and data.

use std::time::Duration;

use apifast_app::HistoryLimits;
use apifast_domain::{ProxyChoice, ProxyConfig, ProxyProtocol};
use apifast_history::Limits;

use apifast_log::LogLevel;

use crate::strings::{Language, Strings};
use crate::theme::{Accent, Density, Look, ThemeChoice};

/// Smallest and largest interface scale.
pub const SCALE_RANGE: (f32, f32) = (0.75, 2.0);
/// Bounds of the numeric settings, as `(min, max)`.
pub const PRETTY_MB_RANGE: (u32, u32) = (1, 64);
/// Bounds of the history entries kept.
pub const HISTORY_ENTRIES_RANGE: (u32, u32) = (10, 100_000);
/// Bounds of the days anything is kept.
pub const DAYS_RANGE: (u32, u32) = (1, 3650);
/// Bounds of the megabytes of response bodies kept.
pub const RESPONSES_MB_RANGE: (u32, u32) = (16, 100_000);
/// Bounds of the default timeout, in seconds; 0 is none.
pub const TIMEOUT_S_RANGE: (u32, u32) = (0, 3600);
/// What the Tab key can insert: a tab (0) or that many spaces.
pub const TAB_SIZES: [u8; 4] = [0, 2, 4, 8];

const MB: u64 = 1024 * 1024;

use crate::theme::text::CODE_RANGE;

/// [`Settings::language`] for the system's language.
pub const SYSTEM_LANGUAGE: &str = "system";

/// The settings.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches, as settings.yml writes them"
)]
pub struct Settings {
    /// The interface's language: the code of one of
    /// [`crate::strings::LANGUAGES`], or `system` for the system's.
    pub language: String,
    /// Open the workspace used last at the start; else the default one.
    pub reopen_workspace: bool,
    /// Ask before closing the window while tabs have unsaved changes.
    pub confirm_close: bool,
    /// The theme.
    pub theme: ThemeChoice,
    /// The color of focus, selection and primary actions.
    pub accent: Accent,
    /// How large everything is drawn, 1 being normal.
    pub ui_scale: f32,
    /// How much room there is between and inside controls.
    pub density: Density,
    /// apifast draws the title bar, with minimize, maximize and close
    /// always; else the system does. Only where [`crate::chrome::SUPPORTED`].
    pub own_title_bar: bool,
    /// A font file (TTF or OTF) for code; empty for the one included.
    pub code_font: String,
    /// The size of code, in points.
    pub code_size: f32,
    /// What the Tab key inserts in code: a tab (0) or that many spaces;
    /// one of [`TAB_SIZES`].
    pub tab_size: u8,
    /// Long lines of code wrap; else they scroll sideways.
    pub wrap_lines: bool,
    /// Code shows its line numbers.
    pub line_numbers: bool,
    /// The timeout of requests that set none, in seconds; 0 is none.
    pub timeout_s: u32,
    /// Bodies up to this many megabytes are shown pretty.
    pub pretty_limit_mb: u32,
    /// History entries kept.
    pub history_entries: u32,
    /// Days history entries and response bodies are kept.
    pub keep_days: u32,
    /// Megabytes of response bodies kept.
    pub responses_mb: u32,
    /// Which proxy the collections that inherit theirs use.
    pub proxy_mode: ProxyModeSetting,
    /// The manual proxy, without its password (kept sealed apart).
    pub proxy: ProxyConfig,
    /// A PEM file of certificate authorities trusted besides the system's;
    /// empty for none.
    pub ca_file: String,
    /// Do not check server certificates.
    pub tls_insecure: bool,
    /// Send the cookies of the collection's jar.
    pub send_cookies: bool,
    /// Keep in the jar the cookies that responses set.
    pub store_cookies: bool,
    /// How much of its work apifast logs, unless `APIFAST_LOG` says.
    pub log_level: LogLevel,
}

/// The application's proxy mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProxyModeSetting {
    /// The system's: the `HTTP_PROXY` family of variables.
    #[default]
    System,
    /// No proxy.
    Off,
    /// The one set here.
    Manual,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: SYSTEM_LANGUAGE.to_owned(),
            reopen_workspace: true,
            confirm_close: false,
            theme: ThemeChoice::System,
            accent: Accent::default(),
            ui_scale: 1.0,
            density: Density::default(),
            own_title_bar: true,
            code_font: String::new(),
            code_size: crate::theme::text::CODE,
            tab_size: 2,
            wrap_lines: true,
            line_numbers: true,
            timeout_s: 30,
            pretty_limit_mb: 2,
            history_entries: 1000,
            keep_days: 30,
            responses_mb: 512,
            proxy_mode: ProxyModeSetting::System,
            proxy: ProxyConfig::default(),
            ca_file: String::new(),
            tls_insecure: false,
            send_cookies: true,
            store_cookies: true,
            log_level: LogLevel::default(),
        }
    }
}

impl Settings {
    /// The settings in `entries`, as `settings.yml` holds them. Also returns
    /// what could not be understood; defaults stand in for it, and values
    /// out of range are brought into it.
    pub fn from_entries(entries: &[(String, String)]) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut problems = Vec::new();
        for (key, value) in entries {
            if let Some(understood) = settings.read_added(key, value) {
                if !understood {
                    problems.push(format!("`{key}: {value}`"));
                }
                continue;
            }
            let ok = match key.as_str() {
                "language" => (value == SYSTEM_LANGUAGE)
                    .then(|| SYSTEM_LANGUAGE.to_owned())
                    .or_else(|| crate::strings::language(value).map(|l| l.code.to_owned()))
                    .map(|code| settings.language = code),
                "theme" => match value.as_str() {
                    "system" => Some(ThemeChoice::System),
                    "light" => Some(ThemeChoice::Light),
                    "dark" => Some(ThemeChoice::Dark),
                    _ => None,
                }
                .map(|theme| settings.theme = theme),
                "ui_scale" => value
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| settings.ui_scale = n.clamp(SCALE_RANGE.0, SCALE_RANGE.1)),
                "pretty_limit_mb" => {
                    number(value, PRETTY_MB_RANGE).map(|n| settings.pretty_limit_mb = n)
                }
                "history_entries" => {
                    number(value, HISTORY_ENTRIES_RANGE).map(|n| settings.history_entries = n)
                }
                "keep_days" => number(value, DAYS_RANGE).map(|n| settings.keep_days = n),
                "responses_mb" => {
                    number(value, RESPONSES_MB_RANGE).map(|n| settings.responses_mb = n)
                }
                "proxy" => match value.as_str() {
                    "system" => Some(ProxyModeSetting::System),
                    "off" => Some(ProxyModeSetting::Off),
                    "manual" => Some(ProxyModeSetting::Manual),
                    _ => None,
                }
                .map(|mode| settings.proxy_mode = mode),
                "proxy_protocol" => {
                    settings.proxy.protocol = ProxyProtocol::parse(value);
                    Some(())
                }
                "proxy_host" => {
                    value.clone_into(&mut settings.proxy.hostname);
                    Some(())
                }
                "proxy_port" => value
                    .parse::<u16>()
                    .ok()
                    .or(value.is_empty().then_some(0))
                    .map(|_| value.clone_into(&mut settings.proxy.port)),
                "proxy_auth" => value.parse::<bool>().ok().map(|b| settings.proxy.auth = b),
                "proxy_username" => {
                    value.clone_into(&mut settings.proxy.username);
                    Some(())
                }
                "proxy_bypass" => {
                    value.clone_into(&mut settings.proxy.bypass);
                    Some(())
                }
                "ca_file" => {
                    value.clone_into(&mut settings.ca_file);
                    Some(())
                }
                "tls_insecure" => value
                    .parse::<bool>()
                    .ok()
                    .map(|b| settings.tls_insecure = b),
                "send_cookies" => value
                    .parse::<bool>()
                    .ok()
                    .map(|b| settings.send_cookies = b),
                "store_cookies" => value
                    .parse::<bool>()
                    .ok()
                    .map(|b| settings.store_cookies = b),
                "log_level" => LogLevel::parse(value).map(|level| settings.log_level = level),
                _ => None,
            };
            if ok.is_none() {
                problems.push(format!("`{key}: {value}`"));
            }
        }
        (settings, problems)
    }

    /// Reads the entry `key` of the general, appearance and editor
    /// sections, and the timeout: `None` if `key` is not one of them, else
    /// whether `value` was understood.
    fn read_added(&mut self, key: &str, value: &str) -> Option<bool> {
        let settings = self;
        let read = match key {
            "reopen_workspace" => value
                .parse::<bool>()
                .ok()
                .map(|b| settings.reopen_workspace = b),
            "confirm_close" => value
                .parse::<bool>()
                .ok()
                .map(|b| settings.confirm_close = b),
            "accent" => Accent::parse(value).map(|accent| settings.accent = accent),
            "density" => Density::parse(value).map(|density| settings.density = density),
            "own_title_bar" => value
                .parse::<bool>()
                .ok()
                .map(|b| settings.own_title_bar = b),
            "code_font" => {
                value.clone_into(&mut settings.code_font);
                Some(())
            }
            "code_size" => value
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite())
                .map(|n| settings.code_size = n.clamp(CODE_RANGE.0, CODE_RANGE.1)),
            "tab_size" => value
                .parse::<u8>()
                .ok()
                .filter(|n| TAB_SIZES.contains(n))
                .map(|n| settings.tab_size = n),
            "wrap_lines" => value.parse::<bool>().ok().map(|b| settings.wrap_lines = b),
            "line_numbers" => value
                .parse::<bool>()
                .ok()
                .map(|b| settings.line_numbers = b),
            "timeout_s" => number(value, TIMEOUT_S_RANGE).map(|n| settings.timeout_s = n),
            _ => return None,
        };
        Some(read.is_some())
    }

    /// The entries for `settings.yml`: every setting, so the file shows
    /// what can be set.
    pub fn entries(&self) -> Vec<(String, String)> {
        let theme = match self.theme {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        };
        vec![
            ("language".into(), self.language.clone()),
            ("reopen_workspace".into(), self.reopen_workspace.to_string()),
            ("confirm_close".into(), self.confirm_close.to_string()),
            ("theme".into(), theme.into()),
            ("accent".into(), self.accent.as_str().into()),
            ("ui_scale".into(), format!("{}", self.ui_scale)),
            ("density".into(), self.density.as_str().into()),
            ("own_title_bar".into(), self.own_title_bar.to_string()),
            ("code_font".into(), self.code_font.clone()),
            ("code_size".into(), format!("{}", self.code_size)),
            ("tab_size".into(), self.tab_size.to_string()),
            ("wrap_lines".into(), self.wrap_lines.to_string()),
            ("line_numbers".into(), self.line_numbers.to_string()),
            ("timeout_s".into(), self.timeout_s.to_string()),
            ("pretty_limit_mb".into(), self.pretty_limit_mb.to_string()),
            ("history_entries".into(), self.history_entries.to_string()),
            ("keep_days".into(), self.keep_days.to_string()),
            ("responses_mb".into(), self.responses_mb.to_string()),
            (
                "proxy".into(),
                match self.proxy_mode {
                    ProxyModeSetting::System => "system",
                    ProxyModeSetting::Off => "off",
                    ProxyModeSetting::Manual => "manual",
                }
                .into(),
            ),
            ("proxy_protocol".into(), self.proxy.protocol.as_str().into()),
            ("proxy_host".into(), self.proxy.hostname.clone()),
            ("proxy_port".into(), self.proxy.port.clone()),
            ("proxy_auth".into(), self.proxy.auth.to_string()),
            ("proxy_username".into(), self.proxy.username.clone()),
            ("proxy_bypass".into(), self.proxy.bypass.clone()),
            ("ca_file".into(), self.ca_file.clone()),
            ("tls_insecure".into(), self.tls_insecure.to_string()),
            ("send_cookies".into(), self.send_cookies.to_string()),
            ("store_cookies".into(), self.store_cookies.to_string()),
            ("log_level".into(), self.log_level.as_str().into()),
        ]
    }

    /// The language [`Self::language`] chooses; for `system`, `system`
    /// (the system's language, if apifast has it). English if neither is.
    pub fn chosen_language(&self, system: Option<&'static Language>) -> &'static Language {
        let chosen = if self.language == SYSTEM_LANGUAGE {
            system
        } else {
            crate::strings::language(&self.language)
        };
        chosen.unwrap_or_else(crate::strings::english)
    }

    /// The texts of [`Self::chosen_language`].
    pub fn strings(&self, system: Option<&'static Language>) -> &'static Strings {
        self.chosen_language(system).strings
    }

    /// The application's proxy, without its password.
    pub fn proxy_choice(&self) -> ProxyChoice {
        match self.proxy_mode {
            ProxyModeSetting::System => ProxyChoice::System,
            ProxyModeSetting::Off => ProxyChoice::Off,
            ProxyModeSetting::Manual => ProxyChoice::Manual(ProxyConfig {
                password: String::new(),
                ..self.proxy.clone()
            }),
        }
    }

    /// How servers are trusted.
    pub fn trust(&self) -> apifast_domain::TlsTrust {
        apifast_domain::TlsTrust {
            extra_ca: (!self.ca_file.trim().is_empty())
                .then(|| std::path::PathBuf::from(self.ca_file.trim()))
                .into_iter()
                .collect(),
            insecure: self.tls_insecure,
        }
    }

    /// Whether apifast draws the title bar: where it can, and the settings
    /// say so.
    pub fn draws_title_bar(&self) -> bool {
        crate::chrome::SUPPORTED && self.own_title_bar
    }

    /// The accent, spacing and code size.
    pub fn look(&self) -> Look {
        Look {
            accent: self.accent,
            density: self.density,
            code_size: self.code_size,
        }
    }

    /// How code editors behave.
    pub fn editor(&self) -> crate::atoms::EditorOptions {
        crate::atoms::EditorOptions {
            tab_size: self.tab_size,
            wrap: self.wrap_lines,
            line_numbers: self.line_numbers,
        }
    }

    /// The timeout of requests that set none, in milliseconds; 0 is none.
    pub fn timeout_ms(&self) -> u64 {
        u64::from(self.timeout_s) * 1000
    }

    /// The largest body shown pretty, in bytes.
    pub fn pretty_limit(&self) -> u64 {
        u64::from(self.pretty_limit_mb) * MB
    }

    fn keep(&self) -> Duration {
        Duration::from_hours(24 * u64::from(self.keep_days))
    }

    /// How much history is kept.
    pub fn history_limits(&self) -> HistoryLimits {
        HistoryLimits {
            max_entries: usize::try_from(self.history_entries).ok(),
            max_age: Some(self.keep()),
        }
    }

    /// How many response bodies are kept: one per history entry at most.
    pub fn response_limits(&self) -> Limits {
        Limits {
            max_count: usize::try_from(self.history_entries).ok(),
            max_total_bytes: Some(u64::from(self.responses_mb) * MB),
            max_age: Some(self.keep()),
        }
    }
}

/// `value` as a whole number, brought into `range`.
fn number(value: &str, (min, max): (u32, u32)) -> Option<u32> {
    let n: u64 = value.trim().parse().ok()?;
    Some(u32::try_from(n).unwrap_or(u32::MAX).clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_their_entries() {
        let settings = Settings {
            language: "es".into(),
            reopen_workspace: false,
            confirm_close: true,
            theme: ThemeChoice::Dark,
            accent: Accent::Teal,
            ui_scale: 1.25,
            density: Density::Compact,
            own_title_bar: false,
            code_font: "/usr/share/fonts/mono.ttf".into(),
            code_size: 14.5,
            tab_size: 4,
            wrap_lines: false,
            line_numbers: true,
            timeout_s: 0,
            pretty_limit_mb: 8,
            history_entries: 500,
            keep_days: 7,
            responses_mb: 256,
            proxy_mode: ProxyModeSetting::Manual,
            proxy: ProxyConfig {
                protocol: ProxyProtocol::Socks5,
                hostname: "proxy.corp".into(),
                port: "1080".into(),
                auth: true,
                username: "ana".into(),
                password: String::new(),
                bypass: "localhost, .internal".into(),
            },
            ca_file: "/etc/ssl/corp.pem".into(),
            tls_insecure: true,
            send_cookies: false,
            store_cookies: false,
            log_level: LogLevel::Debug,
        };
        let (read, problems) = Settings::from_entries(&settings.entries());
        assert_eq!(read, settings);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn unknown_or_bad_values_keep_the_default_and_are_reported() {
        let entries = [
            ("theme".to_owned(), "purple".to_owned()),
            ("language".to_owned(), "tlh".to_owned()),
            ("ui_scale".to_owned(), "9".to_owned()),
            ("keep_days".to_owned(), "-3".to_owned()),
            ("tab_size".to_owned(), "3".to_owned()),
            ("accent".to_owned(), "gold".to_owned()),
            ("code_size".to_owned(), "99".to_owned()),
            ("nope".to_owned(), "1".to_owned()),
        ];
        let (settings, problems) = Settings::from_entries(&entries);
        assert_eq!(settings.theme, ThemeChoice::System);
        assert!(
            (settings.ui_scale - SCALE_RANGE.1).abs() < f32::EPSILON,
            "clamped"
        );
        assert_eq!(settings.keep_days, 30);
        assert_eq!(settings.tab_size, 2);
        assert_eq!(settings.accent, Accent::Blue);
        assert!(
            (settings.code_size - CODE_RANGE.1).abs() < f32::EPSILON,
            "clamped"
        );
        assert_eq!(
            problems,
            [
                "`theme: purple`",
                "`language: tlh`",
                "`keep_days: -3`",
                "`tab_size: 3`",
                "`accent: gold`",
                "`nope: 1`"
            ]
        );
    }

    #[test]
    fn limits_follow_the_settings() {
        let settings = Settings {
            history_entries: 50,
            keep_days: 2,
            responses_mb: 16,
            ..Settings::default()
        };
        let history = settings.history_limits();
        assert_eq!(history.max_entries, Some(50));
        assert_eq!(history.max_age, Some(Duration::from_hours(48)));
        let responses = settings.response_limits();
        assert_eq!(responses.max_total_bytes, Some(16 * MB));
        assert_eq!(Settings::default().pretty_limit(), 2 * MB);
        assert_eq!(Settings::default().timeout_ms(), 30_000);
    }
}
