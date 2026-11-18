//! TLS against a local server that requires a client certificate, with a
//! certificate authority, server and client certificates made for the test.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::Path;
use std::sync::Arc;

use apifast_app::{BodyWriter, CancelToken, HttpTransport, StoreError, StoredBody};
use apifast_domain::{
    ClientCertificate, ClientCertificateKind, HttpRequest, Method, ProxyChoice, TlsTrust,
};
use apifast_http::ReqwestTransport;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The files of a test PKI, in a temporary folder.
struct Pki {
    dir: tempfile::TempDir,
    server_chain: Vec<CertificateDer<'static>>,
    server_key: PrivateKeyDer<'static>,
    ca: CertificateDer<'static>,
}

fn pki() -> Pki {
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "apifast test CA");
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf = |name: &str, purpose: ExtendedKeyUsagePurpose| {
        let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        params.extended_key_usages = vec![purpose];
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &issuer).unwrap();
        (cert, key)
    };
    let (server, server_key) = leaf("localhost", ExtendedKeyUsagePurpose::ServerAuth);
    let (client, client_key) = leaf("client", ExtendedKeyUsagePurpose::ClientAuth);

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ca.pem"), ca_cert.pem()).unwrap();
    std::fs::write(dir.path().join("client.pem"), client.pem()).unwrap();
    std::fs::write(
        dir.path().join("client-key.pem"),
        client_key.serialize_pem(),
    )
    .unwrap();
    Pki {
        dir,
        server_chain: vec![server.der().clone()],
        server_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        ca: ca_cert.der().clone(),
    }
}

/// An HTTPS server on localhost that requires a client certificate signed by
/// the test CA, and answers `hello`.
async fn server(pki: &Pki) -> u16 {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(pki.ca.clone()).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .unwrap();
    let config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(pki.server_chain.clone(), pki.server_key.clone_key())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut head = Vec::new();
                let mut chunk = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });
    port
}

#[derive(Default)]
struct Collect(Vec<u8>);

impl BodyWriter for Collect {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(chunk);
        Ok(())
    }
    fn finish(self) -> Result<StoredBody, StoreError> {
        unreachable!()
    }
}

fn client_certificate(domain: &str) -> ClientCertificate {
    ClientCertificate {
        domain: domain.into(),
        kind: ClientCertificateKind::Pem {
            certificate: "client.pem".into(),
            private_key: "client-key.pem".into(),
        },
        passphrase: String::new(),
        enabled: true,
    }
}

async fn get(
    root: &Path,
    port: u16,
    trust: &TlsTrust,
    certificates: &[ClientCertificate],
) -> Result<Vec<u8>, String> {
    let transport = ReqwestTransport::configured(root, ProxyChoice::Off, trust, certificates, &[])
        .map_err(|e| e.to_string())?;
    let request = apifast_app::render_request(
        &apifast_domain::resolve(apifast_domain::Chain::new(
            &apifast_domain::Collection::new("t"),
            &[],
            &HttpRequest::new("r", Method::Get, format!("https://localhost:{port}/")),
        )),
        &apifast_domain::ResolvedVariables::default(),
        &mut apifast_app::AppHost::new(
            &apifast_app::memory::FixedClock(0),
            &apifast_app::memory::CountingRandom::new(0),
            &apifast_app::NoPrompter,
            std::collections::BTreeMap::new(),
        ),
        apifast_template::Mode::Strict,
    )
    .unwrap()
    .request;
    let mut body = Collect::default();
    let head = transport
        .send(&request, &mut body, &CancelToken::new())
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(head.status, 200);
    // Every new connection here is TLS: its handshake shows, after the
    // connection, with the version.
    let kinds: Vec<_> = head.timeline.iter().map(|e| &e.kind).collect();
    let connected = kinds
        .iter()
        .position(|k| matches!(k, apifast_app::TimelineKind::Connected { .. }));
    let tls = kinds.iter().position(|k| {
        matches!(
            k,
            apifast_app::TimelineKind::Tls { version: Some(v), .. } if v == "TLSv1.3"
        )
    });
    assert!(
        connected.is_some() && tls > connected,
        "{:#?}",
        head.timeline
    );
    Ok(body.0)
}

#[tokio::test]
async fn an_extra_ca_and_a_client_certificate_reach_the_server() {
    let pki = pki();
    let port = server(&pki).await;
    let root = pki.dir.path();
    let trust = TlsTrust {
        extra_ca: vec![root.join("ca.pem")],
        insecure: false,
    };
    assert_eq!(
        get(root, port, &trust, &[client_certificate("localhost")]).await,
        Ok(b"hello".to_vec())
    );
    // With a port and a wildcard too.
    let domain = format!("local*:{port}");
    assert_eq!(
        get(root, port, &trust, &[client_certificate(&domain)]).await,
        Ok(b"hello".to_vec())
    );
}

#[tokio::test]
async fn without_the_ca_or_the_certificate_the_handshake_fails() {
    let pki = pki();
    let port = server(&pki).await;
    let root = pki.dir.path();
    let error = get(
        root,
        port,
        &TlsTrust::default(),
        &[client_certificate("localhost")],
    )
    .await
    .unwrap_err();
    assert!(error.starts_with("TLS error"), "{error}");
    let trust = TlsTrust {
        extra_ca: vec![root.join("ca.pem")],
        insecure: false,
    };
    // A certificate for another host is not presented.
    let error = get(root, port, &trust, &[client_certificate("other.host")])
        .await
        .unwrap_err();
    assert!(!error.is_empty(), "{error}");
    // A disabled one neither.
    let mut disabled = client_certificate("localhost");
    disabled.enabled = false;
    assert!(get(root, port, &trust, &[disabled]).await.is_err());
}

#[tokio::test]
async fn insecure_skips_the_server_check() {
    let pki = pki();
    let port = server(&pki).await;
    let root = pki.dir.path();
    let trust = TlsTrust {
        extra_ca: Vec::new(),
        insecure: true,
    };
    assert_eq!(
        get(root, port, &trust, &[client_certificate("localhost")]).await,
        Ok(b"hello".to_vec())
    );
}

#[tokio::test]
async fn unsupported_certificates_and_bad_ca_files_say_why() {
    let pki = pki();
    let port = server(&pki).await;
    let root = pki.dir.path();
    let pkcs12 = ClientCertificate {
        kind: ClientCertificateKind::Pkcs12 {
            file: "client.pfx".into(),
        },
        ..client_certificate("localhost")
    };
    let error = get(root, port, &TlsTrust::default(), &[pkcs12])
        .await
        .unwrap_err();
    assert!(error.contains("openssl pkcs12"), "{error}");
    std::fs::write(root.join("bad.pem"), "not a certificate").unwrap();
    let trust = TlsTrust {
        extra_ca: vec![root.join("bad.pem")],
        insecure: false,
    };
    let error = get(root, port, &trust, &[]).await.unwrap_err();
    assert!(error.contains("holds no PEM certificate"), "{error}");
}
