//! TLS (docs/07): extra certificate authorities, turning checks off, and
//! client certificates by host.
//!
//! The rustls configuration is built here, not by reqwest, for its session
//! store to mark when each handshake starts ([`crate::probe`]). It is what
//! reqwest would build: ring, the platform's verifier with the extra
//! authorities (or none with checks off), the client certificate, and ALPN
//! for HTTP/2 and HTTP/1.1.

use std::path::Path;
use std::sync::Arc;

use apifast_app::TransportError;
use apifast_domain::{ClientCertificate, ClientCertificateKind, ProxyChoice, TlsTrust};
use rustls::client::Resumption;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

use crate::probe::ProbedSessions;

/// What every client of a transport shares: the proxy and the trust.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClientSettings {
    pub proxy: ProxyChoice,
    pub extra_ca: Vec<CertificateDer<'static>>,
    pub insecure: bool,
}

/// A client certificate with its chain, and its key.
#[derive(Debug)]
pub(crate) struct Identity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

impl Clone for Identity {
    fn clone(&self) -> Self {
        Self {
            chain: self.chain.clone(),
            key: self.key.clone_key(),
        }
    }
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
            let certificates: Vec<_> = CertificateDer::pem_slice_iter(&bytes)
                .collect::<Result<_, _>>()
                .unwrap_or_default();
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

    /// The rustls configuration of a client, presenting `identity` if
    /// given; with `http1_only`, ALPN offers only HTTP/1.1.
    ///
    /// # Errors
    ///
    /// If the platform's verifier or the client certificate cannot be used.
    pub fn rustls_config(
        &self,
        identity: Option<Identity>,
        http1_only: bool,
    ) -> Result<ClientConfig, TransportError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| TransportError::Tls(e.to_string()))?
            .dangerous();
        let builder = if self.insecure {
            builder.with_custom_certificate_verifier(Arc::new(NoVerifier))
        } else {
            let verifier = if self.extra_ca.is_empty() {
                rustls_platform_verifier::Verifier::new(provider)
            } else {
                rustls_platform_verifier::Verifier::new_with_extra_roots(
                    self.extra_ca.iter().cloned(),
                    provider,
                )
            }
            .map_err(|e| TransportError::Tls(format!("the system's certificates: {e}")))?;
            builder.with_custom_certificate_verifier(Arc::new(verifier))
        };
        let mut config = match identity {
            Some(Identity { chain, key }) => builder
                .with_client_auth_cert(chain, key)
                .map_err(|e| TransportError::Tls(format!("the client certificate: {e}")))?,
            None => builder.with_no_client_auth(),
        };
        config.alpn_protocols = if http1_only {
            vec![b"http/1.1".to_vec()]
        } else {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        };
        config.resumption = Resumption::store(Arc::new(ProbedSessions::new()));
        Ok(config)
    }
}

/// The identity of `certificate`, its files resolved against `root`.
///
/// # Errors
///
/// What is wrong with the files, for the user to read.
pub(crate) fn identity(root: &Path, certificate: &ClientCertificate) -> Result<Identity, String> {
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
            let pem = read(cert)?;
            let key = read(private_key)?;
            if String::from_utf8_lossy(&key).contains("ENCRYPTED") {
                return Err(format!(
                    "encrypted private keys (`{private_key}`) are not supported yet; decrypt it: openssl pkey -in {private_key} -out key.pem"
                ));
            }
            let not_pem =
                || format!("`{cert}` and `{private_key}` are not a PEM certificate and key");
            let chain: Vec<_> = CertificateDer::pem_slice_iter(&pem)
                .collect::<Result<_, _>>()
                .map_err(|e| format!("{}: {e}", not_pem()))?;
            if chain.is_empty() {
                return Err(format!("{}: no certificate in `{cert}`", not_pem()));
            }
            let key =
                PrivateKeyDer::from_pem_slice(&key).map_err(|e| format!("{}: {e}", not_pem()))?;
            Ok(Identity { chain, key })
        }
    }
}

/// Accepts any server certificate: "Do not check certificates".
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        // Every scheme ring verifies, as reqwest's own does.
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
