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
}

impl Server {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
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
