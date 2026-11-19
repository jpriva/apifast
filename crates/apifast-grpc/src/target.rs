//! Where a gRPC request goes: its URL as the user writes it (`host:port`,
//! `grpc://`, `grpcs://`, `http://` or `https://`), with an optional path
//! the server's methods are under.

use apifast_app::TransportError;

/// The server of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub host: String,
    pub port: u16,
    /// Whether the connection is in TLS: `grpcs://` and `https://`.
    pub tls: bool,
    /// The path the methods are under, without a trailing `/`; empty for
    /// none.
    pub prefix: String,
}

impl Target {
    /// The server of `url`. Without a scheme, the connection is in plain
    /// text, as with `grpc://`; without a port, it is 80 or 443.
    ///
    /// # Errors
    ///
    /// If the URL has another scheme or no host.
    pub(crate) fn parse(url: &str) -> Result<Self, TransportError> {
        let url = url.trim();
        let invalid = |why: &str| TransportError::InvalidUrl(format!("`{url}`: {why}"));
        let (tls, rest) = match url.split_once("://") {
            None => (false, url),
            Some((scheme, rest)) => match scheme.to_ascii_lowercase().as_str() {
                "grpc" | "http" => (false, rest),
                "grpcs" | "https" => (true, rest),
                _ => {
                    return Err(invalid(
                        "a gRPC URL is `grpc://`, `grpcs://`, `http://` or `https://`",
                    ));
                }
            },
        };
        let (authority, path) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let (host, port) = match authority.rsplit_once(':') {
            // An IPv6 address without a port has `:` inside its brackets.
            Some((host, port)) if !port.contains(']') => {
                let port = port
                    .parse()
                    .map_err(|_| invalid("the port is not a number"))?;
                (host, port)
            }
            _ => (authority, if tls { 443 } else { 80 }),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() {
            return Err(invalid("there is no host"));
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            tls,
            prefix: path.trim_end_matches('/').to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(url: &str) -> (String, u16, bool, String) {
        let t = Target::parse(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        (t.host, t.port, t.tls, t.prefix)
    }

    #[test]
    fn urls_are_read_as_bruno_writes_them() {
        assert_eq!(
            target("localhost:50051"),
            ("localhost".into(), 50051, false, String::new())
        );
        assert_eq!(
            target(" grpcs://api.test "),
            ("api.test".into(), 443, true, String::new())
        );
        assert_eq!(
            target("grpc://localhost:50051/Missing.Api/"),
            ("localhost".into(), 50051, false, "/Missing.Api".into())
        );
        assert_eq!(
            target("https://[::1]:8443"),
            ("::1".into(), 8443, true, String::new())
        );
        assert!(Target::parse("ws://h").is_err());
        assert!(Target::parse("grpc://:1").is_err());
        assert!(Target::parse("h:port").is_err());
    }
}
