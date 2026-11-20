//! OAuth 2.0 tokens: obtained, cached, refreshed and sent, against a
//! scripted token endpoint.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::future::Future;
use std::pin::pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, MemoryCollections, MemoryEnvironments, MemoryResponses, MemorySecrets,
    MemoryTokens, ScriptedPrompter,
};
use apifast_app::{
    AppError, BodyWriter, CancelToken, Clock, HttpTransport, RenderedBody, RenderedRequest,
    RequestPath, ResponseHead, SendOptions, SendPorts, TransportError, send_request,
};
use apifast_domain::{
    AuthScheme, Collection, CredentialsPlacement, HttpRequest, Method, OAuth2Auth, OAuth2Flow,
    Setting, TokenPlacement,
};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory futures complete on the first poll"),
    }
}

/// A clock the test moves.
#[derive(Default)]
struct TestClock(AtomicI64);

impl Clock for TestClock {
    fn unix_millis(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A token endpoint at `/token` and an API anywhere else. Tokens are
/// numbered; each lasts 60 s and can be refreshed.
#[derive(Default)]
struct Server {
    log: Mutex<Vec<String>>,
    issued: Mutex<u32>,
    last_fields: Mutex<Vec<(String, String)>>,
}

impl Server {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn field_of_last_token_request(&self, name: &str) -> String {
        let fields = self.last_fields.lock().unwrap();
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    fn answer(&self, request: &RenderedRequest) -> (u16, String) {
        let fields = match request.body() {
            RenderedBody::Form(fields) => fields.clone(),
            _ => Vec::new(),
        };
        let field = |name: &str| {
            fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let header = |name: &str| {
            request
                .headers()
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        if request.url().ends_with("/token") {
            fields.clone_into(&mut self.last_fields.lock().unwrap());
            let grant = field("grant_type");
            self.log.lock().unwrap().push(format!("token {grant}"));
            if grant == "password" && field("password") != "s3cret" {
                return (
                    400,
                    r#"{"error":"invalid_grant","error_description":"bad password"}"#.into(),
                );
            }
            if grant == "refresh_token" && field("refresh_token") == "expired" {
                return (400, r#"{"error":"invalid_grant"}"#.into());
            }
            let mut issued = self.issued.lock().unwrap();
            *issued += 1;
            return (
                200,
                format!(
                    r#"{{"access_token":"t{issued}","id_token":"id{issued}","token_type":"Bearer","expires_in":60,"refresh_token":"r{issued}"}}"#
                ),
            );
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("api {} {}", request.url(), header("authorization")));
        (200, "{}".into())
    }
}

impl HttpTransport for Server {
    fn send<W: BodyWriter + Send>(
        &self,
        request: &RenderedRequest,
        body: &mut W,
        _: &CancelToken,
    ) -> impl Future<Output = Result<ResponseHead, TransportError>> + Send {
        let (status, text) = self.answer(request);
        let result = body
            .write(text.as_bytes())
            .map_err(|e| TransportError::Other(e.to_string()))
            .map(|()| ResponseHead {
                status,
                version: "HTTP/1.1".into(),
                url: request.url().to_owned(),
                redirects: 0,
                headers: Vec::new(),
                elapsed_ms: 0,
                timeline: Vec::new(),
            });
        std::future::ready(result)
    }
}

fn oauth(flow: OAuth2Flow) -> OAuth2Auth {
    OAuth2Auth {
        flow,
        access_token_url: "http://auth/token".into(),
        client_id: "cli".into(),
        client_secret: "shh".into(),
        username: "ana".into(),
        password: "s3cret".into(),
        ..OAuth2Auth::default()
    }
}

struct World {
    collections: MemoryCollections,
    server: Server,
    clock: TestClock,
    tokens: MemoryTokens,
}

impl World {
    fn new(auth: OAuth2Auth) -> Self {
        let mut request = HttpRequest::new("Orders", Method::Get, "http://api/orders");
        request.auth = Setting::Set(AuthScheme::OAuth2(Box::new(auth)));
        Self {
            collections: MemoryCollections::new(Collection::new("Shop"))
                .with_request("orders.yml", request),
            server: Server::default(),
            clock: TestClock::default(),
            tokens: MemoryTokens::default(),
        }
    }

    fn send(&self) -> Result<(), AppError> {
        let (environments, responses, random, prompter, secrets) = (
            MemoryEnvironments::default(),
            MemoryResponses::default(),
            CountingRandom::new(1),
            ScriptedPrompter::default(),
            MemorySecrets::default(),
        );
        let ports = SendPorts {
            collections: &self.collections,
            environments: &environments,
            transport: &self.server,
            responses: &responses,
            clock: &self.clock,
            random: &random,
            prompter: &prompter,
            secrets: &secrets,
            report: &|_| {},
            history: None,
            tokens: Some(&self.tokens),
            authorizer: None,
            scripts: None,
        };
        let outcome = block_on(send_request(
            ports,
            &RequestPath::new("orders.yml"),
            &SendOptions::default(),
            &CancelToken::new(),
        ))?;
        // The token never shows in what is kept of the request.
        assert!(outcome.contains_secret);
        Ok(())
    }
}

#[test]
fn client_credentials_are_fetched_once_and_refreshed_when_expired() {
    let mut auth = oauth(OAuth2Flow::ClientCredentials);
    auth.auto_refresh = true;
    let world = World::new(auth);
    world.send().unwrap();
    world.send().unwrap();
    assert_eq!(
        world.server.log(),
        [
            "token client_credentials",
            "api http://api/orders Bearer t1",
            "api http://api/orders Bearer t1",
        ]
    );
    // A minute later the token has expired: refreshed, not fetched again.
    world.clock.0.store(61_000, Ordering::Relaxed);
    world.send().unwrap();
    assert_eq!(
        world.server.log()[3..],
        ["token refresh_token", "api http://api/orders Bearer t2"]
    );
}

#[test]
fn without_refresh_an_expired_token_is_fetched_again() {
    let world = World::new(oauth(OAuth2Flow::ClientCredentials));
    world.send().unwrap();
    world.clock.0.store(61_000, Ordering::Relaxed);
    world.send().unwrap();
    assert_eq!(world.server.log()[2], "token client_credentials");
}

#[test]
fn password_grant_errors_say_why() {
    let mut auth = oauth(OAuth2Flow::Password);
    auth.password = "wrong".into();
    let world = World::new(auth);
    assert_eq!(
        world.send().unwrap_err().to_string(),
        "the token request failed (400): invalid_grant: bad password"
    );
    let world = World::new(oauth(OAuth2Flow::Password));
    world.send().unwrap();
    assert_eq!(world.server.log()[0], "token password");
}

#[test]
fn the_token_can_go_in_the_query_or_be_the_id_token() {
    let mut auth = oauth(OAuth2Flow::ClientCredentials);
    auth.token_placement = TokenPlacement::Query("access_token".into());
    let world = World::new(auth);
    world.send().unwrap();
    assert_eq!(
        world.server.log()[1],
        "api http://api/orders?access_token=t1 "
    );

    let mut auth = oauth(OAuth2Flow::ClientCredentials);
    auth.token_source = "id_token".into();
    auth.token_placement = TokenPlacement::Header(String::new());
    auth.credentials_placement = CredentialsPlacement::BasicAuthHeader;
    let world = World::new(auth);
    world.send().unwrap();
    assert_eq!(world.server.log()[1], "api http://api/orders id1");
}

#[test]
fn without_auto_fetch_there_is_no_token() {
    let mut auth = oauth(OAuth2Flow::ClientCredentials);
    auth.auto_fetch = false;
    let world = World::new(auth);
    assert!(
        world
            .send()
            .unwrap_err()
            .to_string()
            .contains("getting one automatically is off")
    );
    assert_eq!(world.server.log(), Vec::<String>::new());
}

/// A browser that authorizes at once: it records the URL and answers with
/// `code` and the state it was given, or with what `answer` says.
#[derive(Default)]
struct Browser {
    seen: Mutex<Vec<String>>,
    answer: Option<Vec<(String, String)>>,
}

fn query_of(url: &str, name: &str) -> String {
    url.split(['?', '&'])
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap_or_default()
        .to_owned()
}

impl apifast_app::Authorizer for Browser {
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        callback: &'a str,
        _: &'a CancelToken,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<(String, String)>, AppError>> + Send + 'a>>
    {
        self.seen
            .lock()
            .unwrap()
            .push(format!("{url} -> {callback}"));
        let answer = self.answer.clone().unwrap_or_else(|| {
            vec![
                ("code".into(), "the-code".into()),
                ("state".into(), query_of(url, "state")),
            ]
        });
        Box::pin(std::future::ready(Ok(answer)))
    }
}

fn send_with_browser(world: &World, browser: &Browser) -> Result<(), AppError> {
    let (environments, responses, random, prompter, secrets) = (
        MemoryEnvironments::default(),
        MemoryResponses::default(),
        CountingRandom::new(1),
        ScriptedPrompter::default(),
        MemorySecrets::default(),
    );
    let ports = SendPorts {
        collections: &world.collections,
        environments: &environments,
        transport: &world.server,
        responses: &responses,
        clock: &world.clock,
        random: &random,
        prompter: &prompter,
        secrets: &secrets,
        report: &|_| {},
        history: None,
        tokens: Some(&world.tokens),
        authorizer: Some(browser),
        scripts: None,
    };
    block_on(send_request(
        ports,
        &RequestPath::new("orders.yml"),
        &SendOptions::default(),
        &CancelToken::new(),
    ))
    .map(|_| ())
}

fn code_flow() -> OAuth2Auth {
    OAuth2Auth {
        flow: OAuth2Flow::AuthorizationCode,
        authorization_url: "http://auth/authorize".into(),
        callback_url: "http://localhost:8765/callback".into(),
        scope: "read write".into(),
        pkce: true,
        ..oauth(OAuth2Flow::AuthorizationCode)
    }
}

#[test]
fn the_code_grant_asks_the_browser_and_proves_the_verifier() {
    let world = World::new(code_flow());
    let browser = Browser::default();
    send_with_browser(&world, &browser).unwrap();
    let seen = browser.seen.lock().unwrap().clone();
    let url = &seen[0];
    assert!(url.starts_with("http://auth/authorize?response_type=code&client_id=cli&redirect_uri=http%3A%2F%2Flocalhost%3A8765%2Fcallback&state="), "{url}");
    assert!(url.contains("&scope=read%20write&code_challenge="), "{url}");
    assert!(
        url.ends_with("&code_challenge_method=S256 -> http://localhost:8765/callback"),
        "{url}"
    );
    assert_eq!(query_of(url, "state").len(), 32);
    assert_eq!(
        world.server.log(),
        [
            "token authorization_code",
            "api http://api/orders Bearer t1"
        ]
    );
    // The verifier sent with the code hashes to the challenge of the URL.
    let verifier = world.server.field_of_last_token_request("code_verifier");
    let digest = <sha2::Sha256 as sha2::Digest>::digest(verifier.as_bytes());
    let challenge = query_of(url, "code_challenge");
    assert_eq!(base64url(&digest), challenge.split(' ').next().unwrap());
}

/// Base64 without padding, URL-safe.
fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

#[test]
fn a_refused_or_forged_authorization_is_an_error() {
    let world = World::new(code_flow());
    let refused = Browser {
        answer: Some(vec![
            ("error".into(), "access_denied".into()),
            ("error_description".into(), "the user said no".into()),
        ]),
        ..Browser::default()
    };
    assert_eq!(
        send_with_browser(&world, &refused).unwrap_err().to_string(),
        "the authorization was refused: access_denied: the user said no"
    );
    let forged = Browser {
        answer: Some(vec![
            ("code".into(), "c".into()),
            ("state".into(), "other".into()),
        ]),
        ..Browser::default()
    };
    assert!(
        send_with_browser(&world, &forged)
            .unwrap_err()
            .to_string()
            .contains("another state")
    );
    // Without a browser, as in CI.
    assert!(
        world
            .send()
            .unwrap_err()
            .to_string()
            .contains("needs a browser")
    );
    assert_eq!(
        world.server.log(),
        Vec::<String>::new(),
        "no token request was made"
    );
}
