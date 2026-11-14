use crate::{
    AuthScheme, Body, Collection, Environment, Folder, Header, HttpRequest, Method, Origin, Param,
    RequestDefaults, Resolved, ResolvedSettings, Setting, Settings, Variable,
};

/// Cadena de herencia de una request: colección → carpetas → request.
#[derive(Debug, Clone, Copy)]
pub struct Chain<'a> {
    /// La colección.
    pub collection: &'a Collection,
    /// Carpetas que contienen la request, **de la más externa a la más interna**.
    pub folders: &'a [Folder],
    /// La request.
    pub request: &'a HttpRequest,
}

impl<'a> Chain<'a> {
    /// Crea la cadena. `folders` va de la carpeta más externa a la más interna.
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

    /// Ámbitos de variables de esta request, de menor a mayor prioridad:
    /// colección, entorno (`environment`, del más general al más específico),
    /// carpetas y request.
    ///
    /// El global va antes y el runtime y las preguntas después; los añade quien
    /// llama, porque no forman parte de la colección.
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

    /// Niveles que pueden definir valores por defecto, del más externo al más interno.
    fn defaults(&self) -> impl DoubleEndedIterator<Item = (Origin, &'a RequestDefaults)> {
        std::iter::once((Origin::Collection, &self.collection.defaults)).chain(
            self.folders
                .iter()
                .enumerate()
                .map(|(i, f)| (Origin::Folder(i), &f.defaults)),
        )
    }

    /// Valor efectivo de un ajuste: el de la request, si lo define; si no, el del
    /// padre más cercano; si nadie lo define, `default`.
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

/// Request con la herencia ya aplicada.
///
/// Solo se obtiene con [`resolve`], así que no se puede enviar una request sin
/// resolver su herencia. Los textos siguen siendo plantillas sin renderizar.
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
    /// Método.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Parámetros habilitados.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// Headers habilitados de todos los niveles, con su procedencia.
    pub fn headers(&self) -> &[Resolved<Header>] {
        &self.headers
    }

    /// Cuerpo.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Autenticación efectiva y el nivel que la define.
    pub fn auth(&self) -> &Resolved<AuthScheme> {
        &self.auth
    }

    /// Ajustes de envío efectivos.
    pub fn settings(&self) -> &ResolvedSettings {
        &self.settings
    }
}

/// Aplica la herencia de headers, auth y ajustes a una request.
///
/// - **Headers:** se acumulan de la colección a la request. Un header sustituye a
///   los de niveles anteriores con el mismo nombre, sin distinguir mayúsculas.
///   Dentro de un mismo nivel se conservan los repetidos. Los deshabilitados se
///   ignoran.
/// - **Auth y ajustes:** se toma el valor del nivel más cercano que lo defina; si
///   nadie lo hace, el valor por defecto (sin auth, y los de [`Settings`]).
/// - **Parámetros y body:** son propios de la request; no se heredan.
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
    fn sin_valores_heredados_usa_los_valores_por_defecto() {
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
    fn la_auth_heredada_viene_de_la_carpeta_mas_cercana_que_la_define() {
        let mut collection = Collection::new("c");
        collection.defaults.auth = bearer("col");
        let mut outer = Folder::new("outer");
        outer.defaults.auth = bearer("outer");
        let inner = Folder::new("inner"); // hereda
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
    fn la_auth_propia_de_la_request_gana_aunque_sea_none() {
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
    fn cada_ajuste_se_hereda_por_separado() {
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
    fn los_headers_se_acumulan_y_se_sobrescriben_sin_distinguir_mayusculas() {
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
    fn los_headers_repetidos_en_un_nivel_se_conservan() {
        let collection = Collection::new("c");
        let mut request = get("/");
        request.headers = vec![Header::new("Accept", "a"), Header::new("accept", "b")];

        let resolved = resolve(Chain::new(&collection, &[], &request));
        assert_eq!(resolved.headers().len(), 2);
    }

    #[test]
    fn un_header_deshabilitado_ni_se_envia_ni_sobrescribe() {
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
    fn solo_se_conservan_los_parametros_habilitados() {
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
    fn las_variables_siguen_la_precedencia_de_la_documentacion() {
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
