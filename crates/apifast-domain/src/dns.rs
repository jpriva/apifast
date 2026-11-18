//! DNS overrides of a collection (docs/07, "DNS"): like `/etc/hosts`, but
//! for the collection's requests only. Kept in `.apifast/collection.yml`.

use std::net::IpAddr;

use crate::certificates::wildcard;

/// A host that goes to a fixed address instead of the one DNS gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsOverride {
    /// The host name; `*` stands for any run of characters
    /// (`*.example.com`).
    pub host: String,
    /// The IP address it goes to.
    pub address: String,
    /// When `false`, it is ignored.
    pub enabled: bool,
}

impl DnsOverride {
    /// Whether it is for `host`, in any case.
    pub fn matches(&self, host: &str) -> bool {
        let pattern = self.host.trim().trim_end_matches('.').to_ascii_lowercase();
        !pattern.is_empty() && wildcard(&pattern, &host.trim_end_matches('.').to_ascii_lowercase())
    }

    /// Its address, if it is an IP address (IPv6 with or without brackets).
    pub fn ip(&self) -> Option<IpAddr> {
        let text = self.address.trim();
        let text = text
            .strip_prefix('[')
            .and_then(|t| t.strip_suffix(']'))
            .unwrap_or(text);
        text.parse().ok()
    }
}

/// The addresses `overrides` give `host`: those of every enabled override
/// for it, in order. `None` if none is for it; an empty list if they are,
/// but none has a valid address.
pub fn overridden_addresses(overrides: &[DnsOverride], host: &str) -> Option<Vec<IpAddr>> {
    let matching: Vec<&DnsOverride> = (overrides.iter())
        .filter(|o| o.enabled && o.matches(host))
        .collect();
    (!matching.is_empty()).then(|| matching.iter().filter_map(|o| o.ip()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str, address: &str) -> DnsOverride {
        DnsOverride {
            host: host.into(),
            address: address.into(),
            enabled: true,
        }
    }

    #[test]
    fn hosts_match_without_case_and_with_wildcards() {
        assert!(entry("API.test", "1.2.3.4").matches("api.TEST"));
        assert!(entry("api.test.", "1.2.3.4").matches("api.test"));
        assert!(entry("*.example.com", "1.2.3.4").matches("a.b.example.com"));
        assert!(!entry("*.example.com", "1.2.3.4").matches("example.com"));
        assert!(!entry("api.test", "1.2.3.4").matches("www.api.test"));
        assert!(!entry(" ", "1.2.3.4").matches(""));
    }

    #[test]
    fn addresses_are_ip_addresses() {
        assert_eq!(entry("h", " 10.0.0.1 ").ip(), "10.0.0.1".parse().ok());
        assert_eq!(entry("h", "[::1]").ip(), "::1".parse().ok());
        assert_eq!(entry("h", "::1").ip(), "::1".parse().ok());
        assert_eq!(entry("h", "localhost").ip(), None);
    }

    #[test]
    fn every_enabled_override_of_a_host_gives_its_address() {
        let mut off = entry("api.test", "10.0.0.9");
        off.enabled = false;
        let overrides = [
            entry("api.test", "10.0.0.1"),
            off,
            entry("*.test", "10.0.0.2"),
            entry("api.test", "not an ip"),
            entry("other.test", "10.0.0.3"),
        ];
        let ips = |host| overridden_addresses(&overrides, host);
        assert_eq!(
            ips("api.test"),
            Some(vec![
                "10.0.0.1".parse().unwrap(),
                "10.0.0.2".parse().unwrap()
            ])
        );
        assert_eq!(ips("nothing.else"), None);
        assert_eq!(
            overridden_addresses(&[entry("bad.test", "nope")], "bad.test"),
            Some(Vec::new())
        );
    }
}
