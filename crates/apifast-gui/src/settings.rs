//! The application's settings, as `settings.yml` keeps them: appearance,
//! how large a response is shown pretty, and how much history and how many
//! response bodies are kept.

use std::time::Duration;

use apifast_app::HistoryLimits;
use apifast_domain::{ProxyChoice, ProxyConfig, ProxyProtocol};
use apifast_history::Limits;

use crate::strings::{Language, Strings};
use crate::theme::ThemeChoice;

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

const MB: u64 = 1024 * 1024;

/// [`Settings::language`] for the system's language.
pub const SYSTEM_LANGUAGE: &str = "system";

/// The settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The interface's language: the code of one of
    /// [`crate::strings::LANGUAGES`], or `system` for the system's.
    pub language: String,
    /// The theme.
    pub theme: ThemeChoice,
    /// How large everything is drawn, 1 being normal.
    pub ui_scale: f32,
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
            theme: ThemeChoice::System,
            ui_scale: 1.0,
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
                _ => None,
            };
            if ok.is_none() {
                problems.push(format!("`{key}: {value}`"));
            }
        }
        (settings, problems)
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
            ("theme".into(), theme.into()),
            ("ui_scale".into(), format!("{}", self.ui_scale)),
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
        ]
    }

    /// The texts of [`Self::language`]; for `system`, those of `system`
    /// (the system's language, if apifast has it), else English.
    pub fn strings(&self, system: Option<&'static Language>) -> &'static Strings {
        let chosen = if self.language == SYSTEM_LANGUAGE {
            system
        } else {
            crate::strings::language(&self.language)
        };
        chosen.map_or(&crate::strings::EN, |language| language.strings)
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
            theme: ThemeChoice::Dark,
            ui_scale: 1.25,
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
            ("nope".to_owned(), "1".to_owned()),
        ];
        let (settings, problems) = Settings::from_entries(&entries);
        assert_eq!(settings.theme, ThemeChoice::System);
        assert!(
            (settings.ui_scale - SCALE_RANGE.1).abs() < f32::EPSILON,
            "clamped"
        );
        assert_eq!(settings.keep_days, 30);
        assert_eq!(
            problems,
            [
                "`theme: purple`",
                "`language: tlh`",
                "`keep_days: -3`",
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
    }
}
