//! TLS (docs/07): the client certificates a collection presents, by host,
//! and the trust settings of the application.

/// A client certificate and the hosts it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCertificate {
    /// The host it is presented to, with an optional port and `*`
    /// wildcards: `api.example.com`, `*.example.com`, `localhost:8443`.
    pub domain: String,
    /// The files.
    pub kind: ClientCertificateKind,
    /// Passphrase of the key or the PKCS#12 file.
    pub passphrase: String,
    /// When `false`, it is not presented.
    pub enabled: bool,
}

/// Where a client certificate is, relative to the collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCertificateKind {
    /// A PEM certificate (with its chain) and its private key.
    Pem {
        /// The certificate file.
        certificate: String,
        /// The private key file.
        private_key: String,
    },
    /// A PKCS#12 (`.pfx`, `.p12`) file.
    Pkcs12 {
        /// The file.
        file: String,
    },
}

impl ClientCertificate {
    /// Whether it is for `host` on `port`. A domain with a port matches only
    /// that port; `*` matches any run of characters.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        let domain = self.domain.trim().to_ascii_lowercase();
        let host = host.to_ascii_lowercase();
        let (pattern, target) = match domain.rsplit_once(':') {
            Some((_, p)) if p.chars().all(|c| c.is_ascii_digit()) => {
                (domain.clone(), format!("{host}:{port}"))
            }
            _ => (domain.clone(), host),
        };
        wildcard(&pattern, &target)
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any run of
/// characters.
fn wildcard(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || !text[first.len()..].ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
}

/// How the application trusts servers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TlsTrust {
    /// PEM files with extra certificate authorities, trusted besides the
    /// system's.
    pub extra_ca: Vec<std::path::PathBuf>,
    /// Do not check certificates at all. Dangerous: anyone on the way can
    /// read and change the traffic.
    pub insecure: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn for_domain(domain: &str) -> ClientCertificate {
        ClientCertificate {
            domain: domain.into(),
            kind: ClientCertificateKind::Pkcs12 {
                file: String::new(),
            },
            passphrase: String::new(),
            enabled: true,
        }
    }

    #[test]
    fn domains_match_hosts_ports_and_wildcards() {
        assert!(for_domain("api.example.com").matches("API.example.com", 443));
        assert!(!for_domain("api.example.com").matches("example.com", 443));
        assert!(for_domain("*.example.com").matches("a.b.example.com", 443));
        assert!(!for_domain("*.example.com").matches("example.org", 443));
        assert!(for_domain("localhost:8443").matches("localhost", 8443));
        assert!(!for_domain("localhost:8443").matches("localhost", 443));
        assert!(for_domain("*").matches("anything", 1));
        assert!(for_domain("a*c*e").matches("abcde", 1));
        assert!(!for_domain("a*c*e").matches("abde", 1));
    }
}
