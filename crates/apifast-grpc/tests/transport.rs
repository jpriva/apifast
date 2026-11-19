//! The gRPC transport against a local server that offers reflection. No
//! test uses the external network.

#![allow(clippy::unwrap_used, reason = "test helpers: a failure fails the test")]

use std::collections::BTreeMap;
use std::path::Path;

use apifast_app::memory::{CountingRandom, FixedClock};
use apifast_app::{
    AppHost, CancelToken, GrpcTransport, NoPrompter, RenderedRequest, TransportError,
    render_request,
};
use apifast_domain::{
    AuthScheme, BearerAuth, Chain, Collection, GrpcMethodType, GrpcService, HttpRequest, Method,
    ResolvedVariables, Setting, resolve,
};
use apifast_grpc::TonicTransport;
use apifast_http::ReqwestTransport;
use apifast_template::Mode;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Server;
use tonic::{Request, Status};

const HELLO: &str = r#"syntax = "proto3";
package hello;
import "google/protobuf/empty.proto";

message HelloRequest { string greeting = 1; }
message HelloReply { string reply = 1; }

service Greeter {
  rpc SayHello(HelloRequest) returns (HelloReply);
  rpc Chat(stream HelloRequest) returns (stream HelloReply);
  rpc Ping(google.protobuf.Empty) returns (google.protobuf.Empty);
}
"#;

/// A folder with `hello.proto`.
fn protos() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.proto"), HELLO).unwrap();
    dir
}

/// Only lets in calls with the bearer token.
fn token(request: Request<()>) -> Result<Request<()>, Status> {
    match request.metadata().get("authorization") {
        Some(value) if value == "Bearer t0k3n" => Ok(request),
        _ => Err(Status::unauthenticated("no token")),
    }
}

/// A server offering reflection, v1 or else v1alpha, of `hello.proto`;
/// its address.
async fn server(alpha: bool) -> String {
    let dir = protos();
    let mut compiler = protox::Compiler::new([dir.path()]).unwrap();
    compiler.include_imports(true);
    compiler.open_file("hello.proto").unwrap();
    let reflection = tonic_reflection::server::Builder::configure()
        .register_file_descriptor_set(compiler.file_descriptor_set());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let incoming = TcpListenerStream::new(listener);
    if alpha {
        let service = InterceptedService::new(reflection.build_v1alpha().unwrap(), token);
        tokio::spawn(
            Server::builder()
                .add_service(service)
                .serve_with_incoming(incoming),
        );
    } else {
        let service = InterceptedService::new(reflection.build_v1().unwrap(), token);
        tokio::spawn(
            Server::builder()
                .add_service(service)
                .serve_with_incoming(incoming),
        );
    }
    address
}

fn rendered(url: &str, with_token: bool) -> RenderedRequest {
    let mut collection = Collection::new("t");
    if with_token {
        collection.defaults.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "t0k3n".into(),
        }));
    }
    let request = HttpRequest::new("call", Method::Get, url);
    let (clock, random) = (FixedClock(0), CountingRandom::new(0));
    let mut host = AppHost::new(&clock, &random, &NoPrompter, BTreeMap::new());
    render_request(
        &resolve(Chain::new(&collection, &[], &request)),
        &ResolvedVariables::default(),
        &mut host,
        Mode::Strict,
    )
    .unwrap()
    .request
}

fn transport(root: &Path) -> TonicTransport {
    TonicTransport::new(ReqwestTransport::new(root).unwrap())
}

/// Each service with its methods: `name type input`.
fn described(services: &[GrpcService]) -> Vec<String> {
    (services.iter())
        .flat_map(|s| &s.methods)
        .map(|m| format!("{} {} {}", m.path, m.method_type.as_str(), m.input))
        .collect()
}

fn hello_methods() -> Vec<String> {
    [
        "/hello.Greeter/SayHello unary hello.HelloRequest",
        "/hello.Greeter/Chat bidi-streaming hello.HelloRequest",
        "/hello.Greeter/Ping unary google.protobuf.Empty",
    ]
    .map(String::from)
    .to_vec()
}

#[tokio::test]
async fn reflection_lists_the_services_in_both_versions_with_the_metadata() {
    let dir = protos();
    for alpha in [false, true] {
        let address = server(alpha).await;
        let grpc = transport(dir.path());
        let services = grpc
            .describe(&rendered(&address, true), None, &CancelToken::new())
            .await
            .unwrap_or_else(|e| panic!("alpha {alpha}: {e}"));
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].name, "hello.Greeter");
        assert_eq!(described(&services), hello_methods());
        assert_eq!(services[0].methods[0].method_type, GrpcMethodType::Unary);

        // Without the token, the server says no.
        let error = grpc
            .describe(&rendered(&address, false), None, &CancelToken::new())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no token"), "{error}");
    }
}

#[tokio::test]
async fn a_proto_file_is_read_without_asking_the_server() {
    let dir = protos();
    let services = transport(dir.path())
        .describe(
            &rendered("grpc://127.0.0.1:1", false),
            Some("hello.proto"),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(described(&services), hello_methods());
}

#[tokio::test]
async fn a_server_that_is_not_there_cannot_be_reached() {
    let dir = protos();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let error = transport(dir.path())
        .describe(&rendered(&address, true), None, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, TransportError::Connect(_)), "{error:?}");
}

/// A test PKI's files in a folder: `ca.pem`, `client.pem`,
/// `client-key.pem`; and the server's certificate and key, in PEM.
fn pki(dir: &Path) -> (String, String) {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
        KeyPair, KeyUsagePurpose,
    };
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
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let leaf = |name: &str, purpose| {
        let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        params.extended_key_usages = vec![purpose];
        let key = KeyPair::generate().unwrap();
        (params.signed_by(&key, &issuer).unwrap(), key)
    };
    let (server, server_key) = leaf("localhost", ExtendedKeyUsagePurpose::ServerAuth);
    let (client, client_key) = leaf("client", ExtendedKeyUsagePurpose::ClientAuth);
    std::fs::write(dir.join("ca.pem"), ca.pem()).unwrap();
    std::fs::write(dir.join("client.pem"), client.pem()).unwrap();
    std::fs::write(dir.join("client-key.pem"), client_key.serialize_pem()).unwrap();
    (server.pem(), server_key.serialize_pem())
}

/// A server in TLS that requires a client certificate of the test CA,
/// offering reflection of `hello.proto`; its port.
async fn tls_server(dir: &Path) -> u16 {
    use tonic::transport::{Certificate, Identity, ServerTlsConfig};
    let (cert, key) = pki(dir);
    let ca = std::fs::read_to_string(dir.join("ca.pem")).unwrap();
    let mut compiler = protox::Compiler::new([dir]).unwrap();
    compiler.include_imports(true);
    compiler.open_file("hello.proto").unwrap();
    let reflection = tonic_reflection::server::Builder::configure()
        .register_file_descriptor_set(compiler.file_descriptor_set())
        .build_v1()
        .unwrap();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(cert, key))
        .client_ca_root(Certificate::from_pem(ca));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let serving = Server::builder()
        .tls_config(tls)
        .unwrap()
        .add_service(reflection)
        .serve_with_incoming(TcpListenerStream::new(listener));
    tokio::spawn(serving);
    port
}

#[tokio::test]
async fn a_server_in_tls_is_trusted_and_given_the_client_certificate() {
    use apifast_domain::{ClientCertificate, ClientCertificateKind, ProxyChoice, TlsTrust};
    let dir = protos();
    let port = tls_server(dir.path()).await;
    let url = format!("grpcs://localhost:{port}");
    let trust = TlsTrust {
        extra_ca: vec![dir.path().join("ca.pem")],
        insecure: false,
    };
    let certificate = ClientCertificate {
        domain: "localhost".into(),
        kind: ClientCertificateKind::Pem {
            certificate: "client.pem".into(),
            private_key: "client-key.pem".into(),
        },
        passphrase: String::new(),
        enabled: true,
    };
    let configured = |certificates: &[ClientCertificate]| {
        TonicTransport::new(
            ReqwestTransport::configured(dir.path(), ProxyChoice::Off, &trust, certificates, &[])
                .unwrap(),
        )
    };
    let services = configured(&[certificate])
        .describe(&rendered(&url, false), None, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(described(&services), hello_methods());

    // Without the certificate the server does not let it in; without the
    // CA, the server is not trusted.
    let error = configured(&[])
        .describe(&rendered(&url, false), None, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("CertificateRequired"), "{error}");
    let untrusted =
        TonicTransport::new(ReqwestTransport::with_proxy(dir.path(), ProxyChoice::Off).unwrap());
    let error = untrusted
        .describe(&rendered(&url, false), None, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, TransportError::Tls(_)), "{error:?}");
}

/// An HTTP proxy that joins each `CONNECT` to its host, and counts them.
async fn http_proxy() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = std::sync::Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    tokio::spawn(async move {
        loop {
            let (mut tcp, _) = listener.accept().await.unwrap();
            seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(tcp.read_u8().await.unwrap());
                }
                let head = String::from_utf8(head).unwrap();
                let target = head.split_whitespace().nth(1).unwrap().to_owned();
                tcp.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
                let mut upstream = tokio::net::TcpStream::connect(target).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut upstream).await;
            });
        }
    });
    (port, count)
}

#[tokio::test]
async fn a_call_goes_through_the_collections_proxy() {
    use apifast_domain::{ProxyChoice, ProxyConfig};
    let dir = protos();
    let address = server(false).await;
    let (proxy, count) = http_proxy().await;
    let config = ProxyConfig {
        hostname: "127.0.0.1".into(),
        port: proxy.to_string(),
        ..ProxyConfig::default()
    };
    let grpc = TonicTransport::new(
        ReqwestTransport::with_proxy(dir.path(), ProxyChoice::Manual(config)).unwrap(),
    );
    let services = grpc
        .describe(&rendered(&address, true), None, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(described(&services), hello_methods());
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}
