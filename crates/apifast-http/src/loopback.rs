//! The browser part of the OAuth 2.0 authorization code grant (RFC 8252):
//! open the authorization URL in the system's browser and catch the
//! redirect to a callback on this machine.

use std::time::Duration;

use apifast_app::{AppError, Authorizer, CancelToken};
use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How long the user has to authorize.
const WAIT: Duration = Duration::from_secs(300);

/// Shown in the browser once the redirect is caught.
const DONE_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>apifast</title></head>\
<body style=\"font-family: system-ui, sans-serif; margin: 3em\"><h1>Authorized</h1>\
<p>apifast got the answer. You can close this window.</p></body></html>";

/// Opens a URL; the system's browser by default.
type Open = Box<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;

/// Catches the redirect on the callback URL's port, on this machine.
pub struct LoopbackAuthorizer {
    open: Open,
    wait: Duration,
}

impl LoopbackAuthorizer {
    /// Opens URLs in the system's browser.
    pub fn system() -> Self {
        Self::new(Box::new(open_in_browser))
    }

    /// Opens URLs with `open`: for tests, a fake browser.
    pub fn new(open: Open) -> Self {
        Self { open, wait: WAIT }
    }

    /// Waits `wait` for the redirect instead of five minutes.
    #[must_use]
    pub fn waiting(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    async fn catch(&self, url: &str, callback: &str) -> Result<Vec<(String, String)>, AppError> {
        let invalid = |reason: &str| {
            AppError::Validation(format!("the OAuth 2.0 callback URL `{callback}` {reason}"))
        };
        let parsed = Url::parse(callback).map_err(|_| invalid("is not a URL"))?;
        let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if parsed.scheme() != "http" || !local {
            return Err(invalid(
                "must be http://localhost:<port>/… or http://127.0.0.1:<port>/…, so apifast can catch the redirect",
            ));
        }
        let port = parsed.port().unwrap_or(80);
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| invalid(&format!("cannot be listened on: {e}")))?;
        (self.open)(url).map_err(|e| {
            AppError::Validation(format!(
                "cannot open the browser: {e}; open this URL: {url}"
            ))
        })?;
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| AppError::Validation(format!("the OAuth 2.0 callback failed: {e}")))?;
            let mut head = Vec::new();
            let mut chunk = [0u8; 2048];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&chunk[..n]),
                }
            }
            let line = String::from_utf8_lossy(&head);
            let target = line.split_whitespace().nth(1).unwrap_or_default();
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            if path != parsed.path() {
                // A favicon, or something else the browser asks for.
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{DONE_PAGE}",
                DONE_PAGE.len()
            );
            let _ = stream.write_all(reply.as_bytes()).await;
            let _ = stream.shutdown().await;
            return Ok(form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect());
        }
    }
}

impl Authorizer for LoopbackAuthorizer {
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        callback: &'a str,
        cancel: &'a CancelToken,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<(String, String)>, AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            tokio::select! {
                result = tokio::time::timeout(self.wait, self.catch(url, callback)) => {
                    result.unwrap_or_else(|_| Err(AppError::Validation(
                        "nobody authorized in the browser in time".into(),
                    )))
                }
                () = crate::cancelled(cancel) => Err(AppError::Transport(
                    apifast_app::TransportError::Cancelled,
                )),
            }
        })
    }
}

/// Opens `url` in the system's browser, without waiting for it.
///
/// # Errors
///
/// If the opener cannot be started.
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        // `start` would split the URL at its `&`.
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}
