//! Proxies (docs/07): what a collection asks for, what the application
//! sets for all, and which one a send uses.

/// The protocol of a proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyProtocol {
    /// Plain HTTP, with `CONNECT` for HTTPS targets.
    #[default]
    Http,
    /// HTTP over TLS to the proxy.
    Https,
    /// SOCKS version 4.
    Socks4,
    /// SOCKS version 5.
    Socks5,
}

impl ProxyProtocol {
    /// Its name in the files and in proxy URLs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks4 => "socks4",
            Self::Socks5 => "socks5",
        }
    }

    /// The protocol named `name`; HTTP when unknown, as Bruno does.
    pub fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "https" => Self::Https,
            "socks4" => Self::Socks4,
            "socks5" | "socks" => Self::Socks5,
            _ => Self::Http,
        }
    }
}

/// A proxy to send through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyConfig {
    /// Protocol.
    pub protocol: ProxyProtocol,
    /// Host name or address.
    pub hostname: String,
    /// Port, as written; empty for the protocol's default.
    pub port: String,
    /// Whether the credentials are sent.
    pub auth: bool,
    /// User name.
    pub username: String,
    /// Password.
    pub password: String,
    /// Hosts that go direct, separated by commas: `localhost, .internal,
    /// 10.0.0.0/8`.
    pub bypass: String,
}

impl ProxyConfig {
    /// The proxy's URL, without credentials: `socks5://host:1080`. `None`
    /// without a host.
    pub fn url(&self) -> Option<String> {
        let host = self.hostname.trim();
        if host.is_empty() {
            return None;
        }
        let port = self.port.trim();
        Some(if port.is_empty() {
            format!("{}://{host}", self.protocol.as_str())
        } else {
            format!("{}://{host}:{port}", self.protocol.as_str())
        })
    }
}

/// What a collection asks of the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// The application's proxy (`inherit: true`).
    #[default]
    Inherit,
    /// No proxy (`disabled: true`).
    Off,
    /// The collection's own.
    Custom,
}

/// A collection's proxy: its mode, and its own proxy, kept even when another
/// mode is chosen, as Bruno does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectionProxy {
    /// Which proxy is used.
    pub mode: ProxyMode,
    /// The collection's own.
    pub config: ProxyConfig,
}

/// The proxy a send goes through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyChoice {
    /// The system's: the `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and
    /// `NO_PROXY` variables.
    #[default]
    System,
    /// Straight to the server.
    Off,
    /// This one.
    Manual(ProxyConfig),
}

impl CollectionProxy {
    /// The proxy a send of this collection uses, `application` being the one
    /// set for all.
    pub fn choose(&self, application: &ProxyChoice) -> ProxyChoice {
        match self.mode {
            ProxyMode::Inherit => application.clone(),
            ProxyMode::Off => ProxyChoice::Off,
            ProxyMode::Custom => ProxyChoice::Manual(self.config.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_choices() {
        let config = ProxyConfig {
            protocol: ProxyProtocol::parse("SOCKS5"),
            hostname: " proxy.corp ".into(),
            port: "1080".into(),
            ..ProxyConfig::default()
        };
        assert_eq!(config.url().as_deref(), Some("socks5://proxy.corp:1080"));
        assert_eq!(ProxyConfig::default().url(), None);
        let custom = CollectionProxy {
            mode: ProxyMode::Custom,
            config: config.clone(),
        };
        assert_eq!(
            custom.choose(&ProxyChoice::Off),
            ProxyChoice::Manual(config)
        );
        let inherit = CollectionProxy::default();
        assert_eq!(inherit.choose(&ProxyChoice::Off), ProxyChoice::Off);
        let off = CollectionProxy {
            mode: ProxyMode::Off,
            ..CollectionProxy::default()
        };
        assert_eq!(off.choose(&ProxyChoice::System), ProxyChoice::Off);
    }
}
