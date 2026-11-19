//! Raw connections through proxies: local echo servers and minimal
//! proxies. No test uses the external network.

#![allow(clippy::unwrap_used, reason = "test helpers: a failure fails the test")]

use std::sync::{Arc, Mutex};

use apifast_app::TransportError;
use apifast_domain::{ProxyChoice, ProxyConfig, ProxyProtocol};
use apifast_http::ReqwestTransport;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

/// Echoes what it reads; its port.
async fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut tcp, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                while let Ok(n) = tcp.read(&mut buf).await {
                    if n == 0 || tcp.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

/// An HTTP proxy that keeps the head of each `CONNECT` and answers
/// `status`; with 200, it joins the client to the host asked for.
async fn http_proxy(status: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let kept = heads.clone();
    tokio::spawn(async move {
        loop {
            let (mut tcp, _) = listener.accept().await.unwrap();
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(tcp.read_u8().await.unwrap());
                }
                let head = String::from_utf8(head).unwrap();
                let target = head.split_whitespace().nth(1).unwrap().to_owned();
                kept.lock().unwrap().push(head);
                tcp.write_all(format!("HTTP/1.1 {status}\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                if status.starts_with("200") {
                    let mut upstream = TcpStream::connect(target).await.unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut upstream).await;
                }
            });
        }
    });
    (port, heads)
}

/// A SOCKS5 proxy that wants `user`/`pass`, and keeps the host and port
/// each client asked for.
async fn socks5_proxy() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = asked.clone();
    tokio::spawn(async move {
        loop {
            let (mut tcp, _) = listener.accept().await.unwrap();
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut hello = [0u8; 2];
                tcp.read_exact(&mut hello).await.unwrap();
                let mut methods = vec![0u8; usize::from(hello[1])];
                tcp.read_exact(&mut methods).await.unwrap();
                assert!(methods.contains(&2), "{methods:?}");
                tcp.write_all(&[5, 2]).await.unwrap();
                let mut version = [0u8; 2];
                tcp.read_exact(&mut version).await.unwrap();
                let mut user = vec![0u8; usize::from(version[1])];
                tcp.read_exact(&mut user).await.unwrap();
                let mut pass = vec![0u8; usize::from(tcp.read_u8().await.unwrap())];
                tcp.read_exact(&mut pass).await.unwrap();
                let ok = user == b"user" && pass == b"pass";
                tcp.write_all(&[1, u8::from(!ok)]).await.unwrap();
                if !ok {
                    return;
                }
                let mut ask = [0u8; 5];
                tcp.read_exact(&mut ask).await.unwrap();
                let mut host = vec![0u8; usize::from(ask[4])];
                tcp.read_exact(&mut host).await.unwrap();
                let port = tcp.read_u16().await.unwrap();
                let host = String::from_utf8(host).unwrap();
                kept.lock().unwrap().push(format!("{host}:{port}"));
                let mut upstream = TcpStream::connect((host.as_str(), port)).await.unwrap();
                tcp.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut upstream).await;
            });
        }
    });
    (port, asked)
}

fn through(protocol: ProxyProtocol, port: u16, bypass: &str) -> ReqwestTransport {
    let config = ProxyConfig {
        protocol,
        hostname: "127.0.0.1".into(),
        port: port.to_string(),
        auth: true,
        username: "user".into(),
        password: "pass".into(),
        bypass: bypass.into(),
    };
    ReqwestTransport::with_proxy(".", ProxyChoice::Manual(config)).unwrap()
}

async fn ping(transport: &ReqwestTransport, host: &str, port: u16) -> Result<(), TransportError> {
    let mut connection = transport.open(host, port, false).await?;
    connection.write_all(b"ping").await.unwrap();
    let mut back = [0u8; 4];
    connection.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"ping");
    Ok(())
}

#[tokio::test]
async fn a_connection_goes_through_an_http_proxy_with_its_credentials() {
    let target = echo().await;
    let (proxy, heads) = http_proxy("200 Connection established").await;
    ping(
        &through(ProxyProtocol::Http, proxy, ""),
        "localhost",
        target,
    )
    .await
    .unwrap();
    let head = heads.lock().unwrap().pop().unwrap();
    assert!(
        head.starts_with(&format!("CONNECT localhost:{target} HTTP/1.1\r\n")),
        "{head}"
    );
    assert!(
        head.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"),
        "{head}"
    );

    // An excluded host goes straight to it.
    let transport = through(ProxyProtocol::Http, proxy, "localhost");
    ping(&transport, "localhost", target).await.unwrap();
    assert!(heads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_proxy_that_refuses_says_why() {
    let (proxy, _) = http_proxy("407 Proxy Authentication Required").await;
    let error = through(ProxyProtocol::Http, proxy, "")
        .open("localhost", 1, false)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("asks for credentials"),
        "{error}"
    );
}

#[tokio::test]
async fn a_connection_goes_through_a_socks5_proxy() {
    let target = echo().await;
    let (proxy, asked) = socks5_proxy().await;
    ping(
        &through(ProxyProtocol::Socks5, proxy, ""),
        "localhost",
        target,
    )
    .await
    .unwrap();
    assert_eq!(*asked.lock().unwrap(), [format!("localhost:{target}")]);
}
