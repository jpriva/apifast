//! TLS (docs/07): extra certificate authorities, turning checks off, and
//! client certificates by host.

use std::path::Path;

use apifast_app::TransportError;
use apifast_domain::{ClientCertificate, ClientCertificateKind, ProxyChoice, TlsTrust};

/// What every client of a transport shares: the proxy and the trust.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClientSettings {
    pub proxy: ProxyChoice,
    pub extra_ca: Vec<reqwest::Certificate>,
    pub insecure: bool,
}

impl ClientSettings {
    /// The settings for `proxy` and `trust`.
    ///
    /// # Errors
    ///
    /// If a certificate authority file cannot be read or holds no
    /// certificate.
    pub fn new(proxy: ProxyChoice, trust: &TlsTrust) -> Result<Self, TransportError> {
        let mut extra_ca = Vec::new();
        for file in &trust.extra_ca {
            let bytes = std::fs::read(file).map_err(|e| {
                TransportError::Tls(format!("cannot read the CA file `{}`: {e}", file.display()))
            })?;
            let certificates = reqwest::Certificate::from_pem_bundle(&bytes).map_err(|_| {
                TransportError::Tls(format!("`{}` holds no PEM certificate", file.display()))
            })?;
            if certificates.is_empty() {
                return Err(TransportError::Tls(format!(
                    "`{}` holds no PEM certificate",
                    file.display()
                )));
            }
            extra_ca.extend(certificates);
        }
        Ok(Self {
            proxy,
            extra_ca,
            insecure: trust.insecure,
        })
    }
}

/// The identity of `certificate`, its files resolved against `root`.
///
/// # Errors
///
/// What is wrong with the files, for the user to read.
pub(crate) fn identity(
    root: &Path,
    certificate: &ClientCertificate,
) -> Result<reqwest::Identity, String> {
    let read = |file: &str| {
        let path = root.join(file);
        std::fs::read(&path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))
    };
    match &certificate.kind {
        ClientCertificateKind::Pkcs12 { file } => Err(format!(
            "PKCS#12 client certificates (`{file}`) are not supported yet; convert it to PEM: openssl pkcs12 -in {file} -out client.pem -nodes"
        )),
        ClientCertificateKind::Pem {
            certificate: cert,
            private_key,
        } => {
            let mut pem = read(cert)?;
            let key = read(private_key)?;
            if String::from_utf8_lossy(&key).contains("ENCRYPTED") {
                return Err(format!(
                    "encrypted private keys (`{private_key}`) are not supported yet; decrypt it: openssl pkey -in {private_key} -out key.pem"
                ));
            }
            pem.push(b'\n');
            pem.extend_from_slice(&key);
            reqwest::Identity::from_pem(&pem).map_err(|e| {
                format!("`{cert}` and `{private_key}` are not a PEM certificate and key: {e}")
            })
        }
    }
}
