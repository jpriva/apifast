use crate::{
    AuthScheme, Body, Collection, Environment, Folder, Header, HttpRequest, Method, Origin, Param,
    RequestDefaults, Resolved, ResolvedSettings, Setting, Settings, Variable,
};

/// Inheritance chain of a request: collection → folders → request.
#[derive(Debug, Clone, Copy)]
pub struct Chain<'a> {
    /// The collection.
    pub collection: &'a Collection,
    /// Folders that contain the request, **from outermost to innermost**.
    pub folders: &'a [Folder],
    /// The request.
    pub request: &'a HttpRequest,
}

impl<'a> Chain<'a> {
    /// Creates the chain. `folders` goes from the outermost folder to the innermost.
    pub fn new(
        collection: &'a Collection,
        folders: &'a [Folder],
        request: &'a HttpRequest,
    ) -> Self {
        Self {
            collection,
            folders,
            request,
        }
    }

    /// Variable scopes of this request, from lowest to highest precedence:
    /// collection, environment (`environment`, from most general to most
    /// specific), folders and request.
    ///
    /// Global goes before them and runtime and prompts after; the caller adds
    /// those, since they are not part of the collection.
    pub fn variable_layers(
        &self,
        environment: &[&'a Environment],
    ) -> Vec<(Origin, &'a [Variable])> {
        let mut layers = vec![(
            Origin::Collection,
            self.collection.defaults.variables.as_slice(),
        )];
        layers.extend(environment.iter().map(|env| {
            (
                Origin::Environment(env.name.clone()),
                env.variables.as_slice(),
            )
        }));
        layers.extend(
            self.folders
                .iter()
                .enumerate()
                .map(|(i, f)| (Origin::Folder(i), f.defaults.variables.as_slice())),
        );
        layers.push((Origin::Request, self.request.variables.as_slice()));
        layers
    }

    /// Levels that can define defaults, from outermost to innermost.
    fn defaults(&self) -> impl DoubleEndedIterator<Item = (Origin, &'a RequestDefaults)> {
        std::iter::once((Origin::Collection, &self.collection.defaults)).chain(
            self.folders
                .iter()
                .enumerate()
                .map(|(i, f)| (Origin::Folder(i), &f.defaults)),
        )
    }

    /// Effective value of a setting: the request's own if set; otherwise the
    /// closest parent's; `default` if nobody sets it.
    fn setting<T: Clone>(
        &self,
        own: &Setting<T>,
        of: impl Fn(&RequestDefaults) -> &Setting<T>,
        default: T,
    ) -> Resolved<T> {
        if let Some(value) = own.as_set() {
            return Resolved::new(value.clone(), Origin::Request);
        }
        self.defaults()
            .rev()
            .find_map(|(origin, d)| of(d).as_set().map(|v| Resolved::new(v.clone(), origin)))
            .unwrap_or_else(|| Resolved::new(default, Origin::Default))
    }
}

/// A request with inheritance already applied.
///
/// It can only be obtained through [`resolve`], so a request cannot be sent
/// without resolving its inheritance. Texts are still unrendered templates.
///
/// Building one by hand must not compile. This guards the invariant: if the
/// fields ever become public, this doctest fails. Stable rustdoc does not
/// check the error code, so keep the example in sync with the fields: if it
/// stops compiling for another reason, the test passes without guarding
/// anything.
///
/// ```compile_fail,E0451
/// use apifast_domain::{
///     AuthScheme, Body, Method, Origin, Resolved, ResolvedRequest, ResolvedSettings,
/// };
///
/// let forged = ResolvedRequest {
///     method: Method::Get,
///     url: String::new(),
///     params: Vec::new(),
///     headers: Vec::new(),
///     body: Body::None,
///     auth: Resolved::new(AuthScheme::None, Origin::Default),
///     settings: ResolvedSettings {
///         timeout_ms: Resolved::new(0, Origin::Default),
///         follow_redirects: Resolved::new(true, Origin::Default),
///         max_redirects: Resolved::new(0, Origin::Default),
///         encode_url: Resolved::new(true, Origin::Default),
///     },
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequest {
    method: Method,
    url: String,
    params: Vec<Param>,
    headers: Vec<Resolved<Header>>,
    body: Body,
    auth: Resolved<AuthScheme>,
    settings: ResolvedSettings,
}

impl ResolvedRequest {
    /// Method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The same request with another auth, from the same origin: how an
    /// auth that is obtained first (an OAuth 2.0 token) becomes one that is
    /// sent.
    #[must_use]
    pub fn with_auth(mut self, auth: AuthScheme) -> Self {
        self.auth.value = auth;
        self
    }

    /// URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Enabled parameters.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// Enabled headers from every level, with their origin.
    pub fn headers(&self) -> &[Resolved<Header>] {
        &self.headers
    }

    /// Body.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Effective authentication and the level that defines it.
    pub fn auth(&self) -> &Resolved<AuthScheme> {
        &self.auth
    }

    /// Effective send settings.
    pub fn settings(&self) -> &ResolvedSettings {
        &self.settings
    }

    /// Changes the method, as a script does.
    pub fn set_method(&mut self, method: Method) {
        self.method = method;
    }

    /// Changes the URL, as a script does.
    pub fn set_url(&mut self, url: String) {
        self.url = url;
    }

    /// Replaces the headers, as a script does.
    pub fn set_headers(&mut self, headers: Vec<Resolved<Header>>) {
        self.headers = headers;
    }

    /// Replaces the body, as a script does.
    pub fn set_body(&mut self, body: Body) {
        self.body = body;
    }

    /// Changes the timeout, as a script does: it then comes from the request.
    pub fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.settings.timeout_ms = Resolved::new(timeout_ms, Origin::Request);
    }
}

/// Applies header, auth and settings inheritance to a request.
///
/// - **Headers:** accumulate from the collection down to the request. A header
///   replaces those with the same name from earlier levels, case-insensitively.
///   Repeated headers within one level are kept. Disabled ones are ignored.
/// - **Auth and settings:** the value of the closest level that sets it wins; if
///   nobody does, the default applies (no auth, and those of [`Settings`]).
/// - **Parameters and body:** belong to the request; they are not inherited.
pub fn resolve(chain: Chain<'_>) -> ResolvedRequest {
    let request = chain.request;
    let settings = &request.settings;

    ResolvedRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        params: request
            .params
            .iter()
            .filter(|p| p.enabled)
            .cloned()
            .collect(),
        headers: resolve_headers(chain),
        body: request.body.clone(),
        auth: chain.setting(&request.auth, |d| &d.auth, AuthScheme::None),
        settings: ResolvedSettings {
            timeout_ms: chain.setting(
                &settings.timeout_ms,
                |d| &d.settings.timeout_ms,
                Settings::DEFAULT_TIMEOUT_MS,
            ),
            follow_redirects: chain.setting(
                &settings.follow_redirects,
                |d| &d.settings.follow_redirects,
                Settings::DEFAULT_FOLLOW_REDIRECTS,
            ),
            max_redirects: chain.setting(
                &settings.max_redirects,
                |d| &d.settings.max_redirects,
                Settings::DEFAULT_MAX_REDIRECTS,
            ),
            encode_url: chain.setting(
                &settings.encode_url,
                |d| &d.settings.encode_url,
                Settings::DEFAULT_ENCODE_URL,
            ),
        },
    }
}

fn resolve_headers(chain: Chain<'_>) -> Vec<Resolved<Header>> {
    let levels = chain
        .defaults()
        .map(|(origin, d)| (origin, d.headers.as_slice()))
        .chain(std::iter::once((
            Origin::Request,
            chain.request.headers.as_slice(),
        )));

    let mut result: Vec<Resolved<Header>> = Vec::new();
    for (origin, headers) in levels {
        let enabled: Vec<&Header> = headers.iter().filter(|h| h.enabled).collect();
        result.retain(|r| {
            !enabled
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case(&r.value.name))
        });
        result.extend(
            enabled
                .into_iter()
                .map(|h| Resolved::new(h.clone(), origin.clone())),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BearerAuth, ParamKind, resolve_variables};

    fn bearer(token: &str) -> Setting<AuthScheme> {
        Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: token.into(),
        }))
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest::new("r", Method::Get, url)
    }

    fn header_pairs(resolved: &ResolvedRequest) -> Vec<(&str, &str, Origin)> {
        resolved
            .headers()
            .iter()
            .map(|h| {
                (
                    h.value.name.as_str(),
                    h.value.value.as_str(),
                    h.origin.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn uses_defaults_when_nothing_is_inherited() {
        let collection = Collection::new("c");
        let request = get("{{host}}/users");
        let resolved = resolve(Chain::new(&collection, &[], &request));

        assert_eq!(resolved.url(), "{{host}}/users");
        assert_eq!(
            resolved.auth(),
            &Resolved::new(AuthScheme::None, Origin::Default)
        );
        let s = resolved.settings();
        assert_eq!(s.timeout_ms, Resolved::new(30_000, Origin::Default));
        assert_eq!(s.follow_redirects, Resolved::new(true, Origin::Default));
        assert_eq!(s.max_redirects, Resolved::new(5, Origin::Default));
        assert_eq!(s.encode_url, Resolved::new(true, Origin::Default));
    }

    #[test]
    fn inherited_auth_comes_from_the_closest_folder_that_sets_it() {
        let mut collection = Collection::new("c");
        collection.defaults.auth = bearer("col");
        let mut outer = Folder::new("outer");
        outer.defaults.auth = bearer("outer");
        let inner = Folder::new("inner"); // inherits
        let folders = [outer, inner];
        let request = get("/");

        let resolved = resolve(Chain::new(&collection, &folders, &request));
        assert_eq!(resolved.auth().origin, Origin::Folder(0));
        assert_eq!(
            resolved.auth().value,
            AuthScheme::Bearer(BearerAuth {
                token: "outer".into()
            })
        );
    }

    #[test]
    fn the_request_own_auth_wins_even_when_none() {
        let mut collection = Collection::new("c");
        collection.defaults.auth = bearer("col");
        let mut request = get("/");
        request.auth = Setting::Set(AuthScheme::None);

        let resolved = resolve(Chain::new(&collection, &[], &request));
        assert_eq!(
            resolved.auth(),
            &Resolved::new(AuthScheme::None, Origin::Request)
        );
    }

    #[test]
    fn each_setting_is_inherited_independently() {
        let mut collection = Collection::new("c");
        collection.defaults.settings.timeout_ms = Setting::Set(1_000);
        collection.defaults.settings.max_redirects = Setting::Set(10);
        let mut folder = Folder::new("f");
        folder.defaults.settings.timeout_ms = Setting::Set(2_000);
        let folders = [folder];
        let mut request = get("/");
        request.settings.follow_redirects = Setting::Set(false);

        let s = resolve(Chain::new(&collection, &folders, &request))
            .settings()
            .clone();
        assert_eq!(s.timeout_ms, Resolved::new(2_000, Origin::Folder(0)));
        assert_eq!(s.max_redirects, Resolved::new(10, Origin::Collection));
        assert_eq!(s.follow_redirects, Resolved::new(false, Origin::Request));
        assert_eq!(s.encode_url, Resolved::new(true, Origin::Default));
    }

    #[test]
    fn headers_accumulate_and_override_case_insensitively() {
        let mut collection = Collection::new("c");
        collection.defaults.headers = vec![
            Header::new("Accept", "application/json"),
            Header::new("X-Team", "core"),
        ];
        let mut folder = Folder::new("f");
        folder.defaults.headers = vec![Header::new("x-team", "plataforma")];
        let folders = [folder];
        let mut request = get("/");
        request.headers = vec![Header::new("X-Request", "1")];

        let resolved = resolve(Chain::new(&collection, &folders, &request));
        assert_eq!(
            header_pairs(&resolved),
            [
                ("Accept", "application/json", Origin::Collection),
                ("x-team", "plataforma", Origin::Folder(0)),
                ("X-Request", "1", Origin::Request),
            ]
        );
    }

    #[test]
    fn repeated_headers_within_a_level_are_kept() {
        let collection = Collection::new("c");
        let mut request = get("/");
        request.headers = vec![Header::new("Accept", "a"), Header::new("accept", "b")];

        let resolved = resolve(Chain::new(&collection, &[], &request));
        assert_eq!(resolved.headers().len(), 2);
    }

    #[test]
    fn a_disabled_header_is_neither_sent_nor_overrides() {
        let mut collection = Collection::new("c");
        collection.defaults.headers = vec![Header::new("Accept", "json")];
        let mut request = get("/");
        let mut disabled = Header::new("Accept", "xml");
        disabled.enabled = false;
        request.headers = vec![disabled];

        let resolved = resolve(Chain::new(&collection, &[], &request));
        assert_eq!(
            header_pairs(&resolved),
            [("Accept", "json", Origin::Collection)]
        );
    }

    #[test]
    fn only_enabled_params_are_kept() {
        let collection = Collection::new("c");
        let mut request = get("/users/:id");
        request.params = vec![
            Param {
                name: "id".into(),
                value: "42".into(),
                kind: ParamKind::Path,
                enabled: true,
            },
            Param {
                name: "notify".into(),
                value: "true".into(),
                kind: ParamKind::Query,
                enabled: false,
            },
        ];

        let resolved = resolve(Chain::new(&collection, &[], &request));
        let names: Vec<_> = resolved.params().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["id"]);
    }

    #[test]
    fn variables_follow_the_documented_precedence() {
        let mut collection = Collection::new("c");
        collection.defaults.variables = vec![
            Variable::text("a", "col"),
            Variable::text("b", "col"),
            Variable::text("c", "col"),
        ];
        let env = Environment::new(
            "prod",
            vec![Variable::text("b", "env"), Variable::text("c", "env")],
        );
        let mut folder = Folder::new("f");
        folder.defaults.variables = vec![Variable::text("c", "folder")];
        let folders = [folder];
        let request = get("/");

        let chain = Chain::new(&collection, &folders, &request);
        let vars = resolve_variables(chain.variable_layers(&[&env]));
        assert_eq!(vars.get("a").unwrap().origin, Origin::Collection);
        assert_eq!(
            vars.get("b").unwrap().origin,
            Origin::Environment("prod".into())
        );
        assert_eq!(vars.get("c").unwrap().origin, Origin::Folder(0));
        assert_eq!(
            vars.get("c").unwrap().overridden,
            [Origin::Collection, Origin::Environment("prod".into())]
        );
    }
}
