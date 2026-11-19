//! Domain → YAML.
//!
//! `update_*` writes into an existing document only the fields that differ
//! between `old` (what the document holds) and `new`, so everything else, such
//! as keys apifast does not model, stays exactly as it was. List items are
//! aligned first, so an item keeps its own unknown keys (such as
//! `description`) even when items before it are added or removed.
//!
//! New files are built the same way, from a minimal document.

use apifast_domain::{
    ApiKeyPlacement, Assertion, Auth, AuthScheme, AwsV4Auth, Body, Capture, Collection,
    Environment, Folder, FormField, GrpcCall, GrpcMessage, Header, HttpRequest, MultipartPart,
    MultipartValue, Param, ParamKind, RequestDefaults, Setting, ValueKind, Variable, VariableValue,
    WsMessage,
};
use apifast_domain::{
    CredentialsPlacement, OAuth2Auth, OAuth2Flow, OAuth2Request, ParameterPlacement, TokenPlacement,
};
use oneyaml::Value;

use crate::diff::{Step, align};
use crate::read;
use crate::yaml::{
    child_mut, get, items, mapping, number, prune_empty, remove, rename_key, set, set_or_remove,
    string,
};

/// Top-level keys Bruno separates with a blank line.
const BLOCK_KEYS: &[&str] = &[
    "info",
    "http",
    "graphql",
    "grpc",
    "websocket",
    "runtime",
    "settings",
    "examples",
    "docs",
    "items",
    "request",
    "config",
    "extensions",
    "variables",
];

/// How apifast writes YAML. Bruno quotes with double quotes; matching it
/// avoids churn when Bruno saves the file later.
pub(crate) fn settings() -> oneyaml::Settings {
    oneyaml::Settings::new().emit(oneyaml::Emit::default().quote_style(oneyaml::QuoteStyle::Double))
}

/// Writes a new document as text, with a blank line before each top-level
/// block, like Bruno.
pub(crate) fn to_text(doc: &Value) -> Result<String, oneyaml::Error> {
    let text = oneyaml::to_string_with(doc, &settings())?;
    let mut out = String::with_capacity(text.len() + 64);
    for (i, line) in text.lines().enumerate() {
        let key = line.split(':').next().unwrap_or_default();
        if i > 0
            && !line.starts_with([' ', '-'])
            && BLOCK_KEYS.contains(&key)
            && !out.ends_with("\n\n")
        {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

// --- Requests ---------------------------------------------------------------

/// The blocks a request may live in, as its `info.type`.
const REQUEST_BLOCKS: [&str; 4] = ["http", "graphql", "websocket", "grpc"];

/// The file's block, and `info.type`, of `request`, as Bruno writes it:
/// `graphql` for a GraphQL body, `websocket` for WebSocket messages,
/// `grpc` for a gRPC call, `http` for any other.
pub(crate) fn block_of(request: &HttpRequest) -> &'static str {
    match request.body {
        Body::GraphQl { .. } => "graphql",
        Body::WebSocket(_) => "websocket",
        Body::Grpc(_) => "grpc",
        _ => "http",
    }
}

/// `text` with its request block, and `info.type`, renamed to `block`
/// where they are, if the file has another of [`REQUEST_BLOCKS`]. Only the
/// top-level block line and the `type:` line under `info:` are touched.
pub(crate) fn retype_request_text(text: String, block: &str) -> String {
    let found = REQUEST_BLOCKS
        .into_iter()
        .filter(|other| *other != block)
        .find(|other| {
            let key = format!("{other}:");
            text.lines().any(|line| line.trim_end() == key)
        });
    let Some(other) = found else {
        return text;
    };
    let key = format!("{other}:");
    let mut in_info = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end();
        if !line.starts_with([' ', '\t', '#']) && !bare.is_empty() {
            in_info = bare == "info:";
        }
        let the_type = in_info && bare.trim_start() == format!("type: {other}");
        if bare == key || the_type {
            out.push_str(&line.replacen(other, block, 1));
        } else {
            out.push_str(line);
        }
    }
    out
}

pub(crate) fn new_request(request: &HttpRequest) -> Value {
    let block = block_of(request);
    // A WebSocket block has no method: the handshake is always a GET. A
    // gRPC block's is the method it calls, none yet.
    let method = match block {
        "graphql" => Some(string("POST")),
        "websocket" => None,
        "grpc" => Some(string("")),
        _ => Some(string("GET")),
    };
    let mut doc = mapping([
        (
            "info",
            Some(mapping([
                ("name", Some(string(&request.name))),
                ("type", Some(string(block))),
            ])),
        ),
        (
            block,
            // Bruno writes a gRPC block's URL before its method.
            Some(if block == "grpc" {
                mapping([("url", Some(string(""))), ("method", method)])
            } else {
                mapping([("method", method), ("url", Some(string("")))])
            }),
        ),
    ]);
    let read::Item::Http(blank) = read::item(&doc) else {
        unreachable!("the document has `type: http` or `type: graphql`")
    };
    update_request(&mut doc, &blank, request);
    doc
}

pub(crate) fn update_request(doc: &mut Value, old: &HttpRequest, new: &HttpRequest) {
    // Containers are only touched (or created) when something in them changes,
    // so saving an unchanged request never alters its file.
    update_info(doc, old, new);
    // A body that becomes or stops being GraphQL moves the request to the
    // other block, in its place.
    let (old_block, block) = (block_of(old), block_of(new));
    if old_block != block {
        rename_key(doc, old_block, block);
        set(child_mut(doc, "info"), "type", string(block));
    }

    let http_changed = (
        &old.method,
        &old.url,
        &old.headers,
        &old.params,
        &old.body,
        &old.auth,
    ) != (
        &new.method,
        &new.url,
        &new.headers,
        &new.params,
        &new.body,
        &new.auth,
    );
    if http_changed {
        update_block(child_mut(doc, block), block, old, new);
    }
    if old.auth != new.auth {
        // The auth now lives in `http.auth`; drop the rc1 location if present.
        if let Some(runtime) = doc
            .as_mapping_mut()
            .and_then(|m| m.get_mut(&string("runtime")))
        {
            remove(runtime, "auth");
        }
        prune_empty(doc, "runtime");
    }

    if old.variables != new.variables {
        let runtime = child_mut(doc, "runtime");
        update_list(
            runtime,
            "variables",
            &old.variables,
            &new.variables,
            variable_value_item,
            merge_variable,
        );
        prune_empty(doc, "runtime");
    }

    if old.assertions != new.assertions {
        let runtime = child_mut(doc, "runtime");
        update_list(
            runtime,
            "assertions",
            &old.assertions,
            &new.assertions,
            assertion_item,
            merge_assertion,
        );
        prune_empty(doc, "runtime");
    }

    if old.captures != new.captures {
        update_captures(doc, &old.captures, &new.captures);
    }

    if old.settings != new.settings {
        update_settings(doc, old, new);
    }

    if old.docs != new.docs {
        set_or_remove(doc, "docs", new.docs.as_deref().map(string));
    }

    if old.examples != new.examples {
        crate::examples::update_examples(doc, &old.examples, &new.examples);
    }
}

/// The request's block (`http`, `graphql`, `websocket` or `grpc`): method,
/// URL, headers, parameters, body and auth. A WebSocket block has no
/// method and no parameters (they are those of its URL), and its body is
/// its messages. A gRPC block has no parameters either; its headers are
/// its `metadata`, and its body, its method and messages.
fn update_block(http: &mut Value, block: &str, old: &HttpRequest, new: &HttpRequest) {
    let (websocket, grpc) = (block == "websocket", block == "grpc");
    if old.method != new.method && !websocket && !grpc {
        set(http, "method", string(new.method.as_str()));
    }
    if old.url != new.url {
        set(http, "url", string(&new.url));
    }
    update_list(
        http,
        if grpc { "metadata" } else { "headers" },
        &old.headers,
        &new.headers,
        header_value,
        merge_header,
    );
    // A WebSocket or gRPC request's parameters are those of its URL.
    if !websocket && !grpc {
        update_list(
            http,
            "params",
            &old.params,
            &new.params,
            param_value,
            merge_param,
        );
    }
    match (&old.body, &new.body) {
        (Body::WebSocket(o), Body::WebSocket(n)) if o == n => {}
        (Body::WebSocket(o), Body::WebSocket(n)) => update_ws_messages(http, o, n),
        (_, Body::WebSocket(messages)) => {
            set_or_remove(http, "message", ws_message_value(messages));
        }
        (Body::Grpc(o), Body::Grpc(n)) => update_grpc_call(http, o, n),
        (_, Body::Grpc(n)) => update_grpc_call(http, &GrpcCall::default(), n),
        (old_body, new_body) => update_body(http, old_body, new_body),
    }
    update_auth(http, "auth", &old.auth, &new.auth, AuthLevel::Item);
}

/// A list of `items` as values, or `None` when empty.
pub(crate) fn list_value<T>(items: &[T], value: impl Fn(&T) -> Value) -> Option<Value> {
    (!items.is_empty()).then(|| items.iter().map(value).collect::<Vec<_>>().into())
}

/// The `info` block: name, position and tags.
fn update_info(doc: &mut Value, old: &HttpRequest, new: &HttpRequest) {
    if (&old.name, old.seq, &old.tags) == (&new.name, new.seq, &new.tags) {
        return;
    }
    let info = child_mut(doc, "info");
    if old.name != new.name {
        set(info, "name", string(&new.name));
    }
    if old.seq != new.seq {
        set_or_remove(info, "seq", new.seq.map(number));
    }
    if old.tags != new.tags {
        let tags =
            (!new.tags.is_empty()).then(|| new.tags.iter().map(string).collect::<Vec<_>>().into());
        set_or_remove(info, "tags", tags);
    }
}

fn update_settings(doc: &mut Value, old: &HttpRequest, new: &HttpRequest) {
    let (o, n) = (&old.settings, &new.settings);
    let settings = child_mut(doc, "settings");
    if o.encode_url != n.encode_url {
        set_or_remove(
            settings,
            "encodeUrl",
            n.encode_url.as_set().map(|b| Value::Bool(*b)),
        );
    }
    if o.timeout_ms != n.timeout_ms {
        set(
            settings,
            "timeout",
            n.timeout_ms
                .as_set()
                .map_or_else(|| string("inherit"), |t| number(*t)),
        );
    }
    if o.follow_redirects != n.follow_redirects {
        set_or_remove(
            settings,
            "followRedirects",
            n.follow_redirects.as_set().map(|b| Value::Bool(*b)),
        );
    }
    if o.max_redirects != n.max_redirects {
        set_or_remove(
            settings,
            "maxRedirects",
            n.max_redirects.as_set().map(|m| number(*m)),
        );
    }

    let everything_inherits = *n == apifast_domain::Settings::default();
    let only_known_keys = settings.as_mapping().is_some_and(|m| {
        m.keys().all(|k| {
            matches!(
                k.as_str(),
                Some("encodeUrl" | "timeout" | "followRedirects" | "maxRedirects")
            )
        })
    });
    if everything_inherits && only_known_keys {
        remove(doc, "settings");
    } else if n.timeout_ms == Setting::Inherit {
        // With a `settings` block, a missing timeout means 0 (no limit) to
        // Bruno, so inheriting must be written out.
        set(settings, "timeout", string("inherit"));
    }
}

pub(crate) fn header_value(header: &Header) -> Value {
    mapping([
        ("name", Some(string(&header.name))),
        ("value", Some(string(&header.value))),
        ("disabled", (!header.enabled).then_some(Value::Bool(true))),
    ])
}

fn merge_header(item: &mut Value, old: &Header, new: &Header) {
    merge_named(
        item,
        (&old.name, &old.value, old.enabled),
        (&new.name, &new.value, new.enabled),
    );
}

/// Writes the name, value and `disabled` flag that changed.
fn merge_named(item: &mut Value, old: (&str, &str, bool), new: (&str, &str, bool)) {
    if old.0 != new.0 {
        set(item, "name", string(new.0));
    }
    if old.1 != new.1 {
        set(item, "value", string(new.1));
    }
    if old.2 != new.2 {
        set_or_remove(item, "disabled", (!new.2).then_some(Value::Bool(true)));
    }
}

fn param_kind(kind: ParamKind) -> Value {
    string(match kind {
        ParamKind::Query => "query",
        ParamKind::Path => "path",
    })
}

pub(crate) fn param_value(param: &Param) -> Value {
    mapping([
        ("name", Some(string(&param.name))),
        ("value", Some(string(&param.value))),
        ("type", Some(param_kind(param.kind))),
        ("disabled", (!param.enabled).then_some(Value::Bool(true))),
    ])
}

fn merge_param(item: &mut Value, old: &Param, new: &Param) {
    merge_named(
        item,
        (&old.name, &old.value, old.enabled),
        (&new.name, &new.value, new.enabled),
    );
    if old.kind != new.kind {
        set(item, "type", param_kind(new.kind));
    }
}

fn variable_value_item(variable: &Variable) -> Value {
    mapping([
        ("name", Some(string(&variable.name))),
        ("value", Some(typed_value(variable.value.as_ref()))),
        ("disabled", (!variable.enabled).then_some(Value::Bool(true))),
    ])
}

/// A plain string for text values, `{type, data}` for the others.
fn typed_value(value: Option<&VariableValue>) -> Value {
    let Some(value) = value else {
        return string("");
    };
    let kind = match value.kind {
        ValueKind::String => return string(&value.data),
        ValueKind::Number => "number",
        ValueKind::Bool => "boolean",
        ValueKind::Object => "object",
    };
    mapping([
        ("type", Some(string(kind))),
        ("data", Some(string(&value.data))),
    ])
}

fn merge_variable(item: &mut Value, old: &Variable, new: &Variable) {
    if old.name != new.name {
        set(item, "name", string(&new.name));
    }
    if old.value != new.value {
        set(item, "value", typed_value(new.value.as_ref()));
    }
    if old.enabled != new.enabled {
        set_or_remove(
            item,
            "disabled",
            (!new.enabled).then_some(Value::Bool(true)),
        );
    }
}

// --- Assertions and captures -------------------------------------------------

fn assertion_item(assertion: &Assertion) -> Value {
    mapping([
        ("expression", Some(string(&assertion.expression))),
        ("operator", Some(string(assertion.operator.as_str()))),
        ("value", assertion.value.as_deref().map(string)),
        (
            "disabled",
            (!assertion.enabled).then_some(Value::Bool(true)),
        ),
    ])
}

fn merge_assertion(item: &mut Value, old: &Assertion, new: &Assertion) {
    if old.expression != new.expression {
        set(item, "expression", string(&new.expression));
    }
    if old.operator != new.operator {
        set(item, "operator", string(new.operator.as_str()));
    }
    if old.value != new.value {
        set_or_remove(item, "value", new.value.as_deref().map(string));
    }
    if old.enabled != new.enabled {
        set_or_remove(
            item,
            "disabled",
            (!new.enabled).then_some(Value::Bool(true)),
        );
    }
}

fn capture_item(capture: &Capture) -> Value {
    mapping([
        ("type", Some(string("set-variable"))),
        ("phase", Some(string(&capture.phase))),
        (
            "selector",
            Some(mapping([
                ("expression", Some(string(&capture.expression))),
                ("method", Some(string(&capture.method))),
            ])),
        ),
        (
            "variable",
            Some(mapping([
                ("name", Some(string(&capture.variable))),
                ("scope", Some(string(capture.scope.as_str()))),
            ])),
        ),
        ("disabled", (!capture.enabled).then_some(Value::Bool(true))),
    ])
}

fn merge_capture(item: &mut Value, old: &Capture, new: &Capture) {
    if old.phase != new.phase {
        set(item, "phase", string(&new.phase));
    }
    if (&old.expression, &old.method) != (&new.expression, &new.method) {
        let selector = child_mut(item, "selector");
        set(selector, "expression", string(&new.expression));
        set(selector, "method", string(&new.method));
    }
    if (&old.variable, &old.scope) != (&new.variable, &new.scope) {
        let variable = child_mut(item, "variable");
        set(variable, "name", string(&new.variable));
        set(variable, "scope", string(new.scope.as_str()));
    }
    if old.enabled != new.enabled {
        set_or_remove(
            item,
            "disabled",
            (!new.enabled).then_some(Value::Bool(true)),
        );
    }
}

/// Rewrites the captures in `runtime.actions`, keeping the actions of other
/// kinds (which are not modeled) after them, as they were.
fn update_captures(doc: &mut Value, old: &[Capture], new: &[Capture]) {
    let runtime = child_mut(doc, "runtime");
    let (captures, others): (Vec<Value>, Vec<Value>) = items(get(runtime, "actions"))
        .iter()
        .cloned()
        .partition(read::is_capture);
    let mut scratch = mapping([(
        "actions",
        (!captures.is_empty()).then(|| Value::from(captures)),
    )]);
    update_list(
        &mut scratch,
        "actions",
        old,
        new,
        capture_item,
        merge_capture,
    );
    let mut actions = items(get(&scratch, "actions")).to_vec();
    actions.extend(others);
    set_or_remove(
        runtime,
        "actions",
        (!actions.is_empty()).then(|| actions.into()),
    );
    prune_empty(doc, "runtime");
}

/// Rewrites the list at `parent[key]` so it holds `new`, keeping the original
/// item (and its unknown keys) for every item that is kept or changed.
pub(crate) fn update_list<T: PartialEq>(
    parent: &mut Value,
    key: &str,
    old: &[T],
    new: &[T],
    fresh: impl Fn(&T) -> Value,
    merge: impl Fn(&mut Value, &T, &T),
) {
    if old == new {
        return;
    }
    let originals = items(get(parent, key));
    // `old` was read from these items one to one; if not, start afresh.
    let usable = originals.len() == old.len();
    let mut out = Vec::with_capacity(new.len());
    for step in align(old, new) {
        match step {
            Step::Keep(i, j) => out.push(if usable {
                originals[i].clone()
            } else {
                fresh(&new[j])
            }),
            Step::Change(i, j) => {
                if usable {
                    let mut item = originals[i].clone();
                    merge(&mut item, &old[i], &new[j]);
                    out.push(item);
                } else {
                    out.push(fresh(&new[j]));
                }
            }
            Step::Insert(j) => out.push(fresh(&new[j])),
            Step::Remove(_) => {}
        }
    }
    set_or_remove(parent, key, (!out.is_empty()).then(|| out.into()));
}

// --- Auth -------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthLevel {
    /// Requests and folders: a missing auth means none.
    Item,
    /// The collection: a missing auth means the default (none).
    Collection,
}

fn update_auth(parent: &mut Value, key: &str, old: &Auth, new: &Auth, level: AuthLevel) {
    if old == new {
        return;
    }
    // OAuth 2.0 nests its fields: merge what changed into the block, so
    // keys apifast does not model stay.
    if let (Setting::Set(AuthScheme::OAuth2(o)), Setting::Set(AuthScheme::OAuth2(n))) = (old, new)
        && let Some(existing) = parent
            .as_mapping_mut()
            .and_then(|m| m.get_mut(&string(key)))
        && existing.as_mapping().is_some()
    {
        merge(existing, &oauth2_value(o), &oauth2_value(n));
        return;
    }
    // Same scheme: write only the fields, keeping any the domain does not model.
    if let (Setting::Set(o), Setting::Set(n)) = (old, new)
        && let Some(existing) = parent
            .as_mapping_mut()
            .and_then(|m| m.get_mut(&string(key)))
        && existing.as_mapping().is_some()
    {
        let mut pairs: Vec<(&str, String, String)> = Vec::new();
        match (o, n) {
            (AuthScheme::Basic(a), AuthScheme::Basic(b)) => {
                pairs.push(("username", a.username.clone(), b.username.clone()));
                pairs.push(("password", a.password.clone(), b.password.clone()));
            }
            (AuthScheme::Bearer(a), AuthScheme::Bearer(b)) => {
                pairs.push(("token", a.token.clone(), b.token.clone()));
            }
            (AuthScheme::ApiKey(a), AuthScheme::ApiKey(b)) => {
                pairs.push(("key", a.key.clone(), b.key.clone()));
                pairs.push(("value", a.value.clone(), b.value.clone()));
                pairs.push((
                    "placement",
                    placement(a.placement).into(),
                    placement(b.placement).into(),
                ));
            }
            (AuthScheme::Digest(a), AuthScheme::Digest(b)) => {
                pairs.push(("username", a.username.clone(), b.username.clone()));
                pairs.push(("password", a.password.clone(), b.password.clone()));
            }
            (AuthScheme::Ntlm(a), AuthScheme::Ntlm(b)) => {
                pairs.push(("username", a.username.clone(), b.username.clone()));
                pairs.push(("password", a.password.clone(), b.password.clone()));
                pairs.push(("domain", a.domain.clone(), b.domain.clone()));
            }
            (AuthScheme::AwsV4(a), AuthScheme::AwsV4(b)) => {
                for ((field, before), (_, after)) in aws_fields(a).into_iter().zip(aws_fields(b)) {
                    pairs.push((field, before, after));
                }
            }
            _ => {}
        }
        if !pairs.is_empty() {
            for (field, before, after) in pairs {
                if before != after {
                    set(existing, field, string(after));
                }
            }
            return;
        }
    }
    let value = match new {
        Setting::Inherit if level == AuthLevel::Collection => None,
        Setting::Inherit => Some(string("inherit")),
        Setting::Set(scheme) => auth_value(scheme),
    };
    set_or_remove(parent, key, value);
}

fn placement(placement: ApiKeyPlacement) -> &'static str {
    match placement {
        ApiKeyPlacement::Header => "header",
        ApiKeyPlacement::Query => "query",
    }
}

/// The fields of an AWS scheme with their names in the files, in Bruno's
/// order. Empty optional fields are written as empty, as Bruno does.
fn aws_fields(auth: &AwsV4Auth) -> [(&'static str, String); 6] {
    [
        ("accessKeyId", auth.access_key_id.clone()),
        ("secretAccessKey", auth.secret_access_key.clone()),
        ("sessionToken", auth.session_token.clone()),
        ("service", auth.service.clone()),
        ("region", auth.region.clone()),
        ("profileName", auth.profile_name.clone()),
    ]
}

/// Writes into `existing` what changed from `old` to `new`, recursively in
/// mappings: keys of `existing` that neither has stay as they are.
fn merge(existing: &mut Value, old: &Value, new: &Value) {
    match (existing.as_mapping().is_some(), old, new) {
        (true, Value::Mapping(o), Value::Mapping(n)) => {
            for (key, _) in o {
                if !n.contains_key(key)
                    && let Some(key) = key.as_str()
                {
                    remove(existing, key);
                }
            }
            for (key, value) in n {
                let Some(name) = key.as_str() else { continue };
                match (o.get(key), get(existing, name)) {
                    (Some(before), _) if before == value => {}
                    (Some(before @ Value::Mapping(_)), Some(Value::Mapping(_)))
                        if matches!(value, Value::Mapping(_)) =>
                    {
                        merge(child_mut(existing, name), before, value);
                    }
                    _ => set(existing, name, value.clone()),
                }
            }
        }
        _ => {
            if old != new {
                *existing = new.clone();
            }
        }
    }
}

/// A client certificate as Bruno writes it.
fn certificate_value(certificate: &apifast_domain::ClientCertificate) -> Value {
    use apifast_domain::ClientCertificateKind;
    let s = |v: &str| Some(string(v));
    let text = |v: &str| (!v.is_empty()).then(|| string(v));
    let mut pairs = vec![("domain", s(&certificate.domain))];
    match &certificate.kind {
        ClientCertificateKind::Pem {
            certificate,
            private_key,
        } => {
            pairs.push(("type", s("pem")));
            pairs.push(("certificateFilePath", s(certificate)));
            pairs.push(("privateKeyFilePath", s(private_key)));
        }
        ClientCertificateKind::Pkcs12 { file } => {
            pairs.push(("type", s("pkcs12")));
            pairs.push(("pkcs12FilePath", s(file)));
        }
    }
    pairs.push(("passphrase", text(&certificate.passphrase)));
    pairs.push((
        "disabled",
        (!certificate.enabled).then_some(Value::Bool(true)),
    ));
    mapping(pairs)
}

/// The YAML of a collection's `config.proxy`, as Bruno writes it.
fn proxy_value(proxy: &apifast_domain::CollectionProxy) -> Value {
    use apifast_domain::ProxyMode;
    let c = &proxy.config;
    mapping([
        (
            "inherit",
            Some(Value::Bool(proxy.mode == ProxyMode::Inherit)),
        ),
        (
            "disabled",
            (proxy.mode == ProxyMode::Off).then_some(Value::Bool(true)),
        ),
        (
            "config",
            Some(mapping([
                ("protocol", Some(string(c.protocol.as_str()))),
                ("hostname", Some(string(&c.hostname))),
                ("port", Some(string(&c.port))),
                (
                    "auth",
                    Some(mapping([
                        ("username", Some(string(&c.username))),
                        ("password", Some(string(&c.password))),
                        ("disabled", (!c.auth).then_some(Value::Bool(true))),
                    ])),
                ),
                ("bypassProxy", Some(string(&c.bypass))),
            ])),
        ),
    ])
}

/// The YAML of an `oauth2` auth, in Bruno's key order; empty texts are left
/// out, as Bruno does.
fn oauth2_value(auth: &OAuth2Auth) -> Value {
    let text = |v: &str| (!v.is_empty()).then(|| string(v));
    let section = |pairs: Vec<(&str, Option<Value>)>| {
        let value = mapping(pairs);
        value
            .as_mapping()
            .is_some_and(|m| !m.is_empty())
            .then_some(value)
    };
    let parameters = |request: OAuth2Request| {
        let list: Vec<Value> = (auth.parameters.iter())
            .filter(|p| p.request == request)
            .map(|p| {
                mapping([
                    ("name", Some(string(&p.name))),
                    (
                        "placement",
                        Some(string(match p.placement {
                            ParameterPlacement::Header => "header",
                            ParameterPlacement::Query => "query",
                            ParameterPlacement::Body => "body",
                        })),
                    ),
                    ("value", text(&p.value)),
                ])
            })
            .collect();
        (!list.is_empty()).then(|| Value::from(list))
    };
    let flow = auth.flow;
    let placement = match &auth.token_placement {
        TokenPlacement::Header(prefix) => mapping([("header", Some(string(prefix)))]),
        TokenPlacement::Query(key) => mapping([("query", Some(string(key)))]),
    };
    mapping([
        ("type", Some(string("oauth2"))),
        ("flow", Some(string(flow.as_str()))),
        ("authorizationUrl", text(&auth.authorization_url)),
        ("accessTokenUrl", text(&auth.access_token_url)),
        ("refreshTokenUrl", text(&auth.refresh_token_url)),
        ("callbackUrl", text(&auth.callback_url)),
        (
            "credentials",
            section(vec![
                ("clientId", text(&auth.client_id)),
                ("clientSecret", text(&auth.client_secret)),
                (
                    "placement",
                    Some(string(match auth.credentials_placement {
                        CredentialsPlacement::Body => "body",
                        CredentialsPlacement::BasicAuthHeader => "basic_auth_header",
                    })),
                ),
            ]),
        ),
        (
            "resourceOwner",
            section(vec![
                ("username", text(&auth.username)),
                ("password", text(&auth.password)),
            ]),
        ),
        (
            "additionalParameters",
            section(vec![
                (
                    "authorizationRequest",
                    parameters(OAuth2Request::Authorization),
                ),
                ("accessTokenRequest", parameters(OAuth2Request::Token)),
                ("refreshTokenRequest", parameters(OAuth2Request::Refresh)),
            ]),
        ),
        ("scope", text(&auth.scope)),
        ("state", text(&auth.state)),
        (
            "pkce",
            (flow == OAuth2Flow::AuthorizationCode).then(|| {
                if auth.pkce {
                    mapping([])
                } else {
                    mapping([("disabled", Some(Value::Bool(true)))])
                }
            }),
        ),
        (
            "tokenConfig",
            Some(mapping([
                ("id", text(&auth.token_id)),
                ("placement", Some(placement)),
                ("source", text(&auth.token_source)),
            ])),
        ),
        (
            "settings",
            Some(mapping([
                ("autoFetchToken", Some(Value::Bool(auth.auto_fetch))),
                ("autoRefreshToken", Some(Value::Bool(auth.auto_refresh))),
            ])),
        ),
    ])
}

/// The YAML for a scheme; `None` for no auth, which Bruno writes as no key.
fn auth_value(scheme: &AuthScheme) -> Option<Value> {
    let s = |v: &str| Some(string(v));
    Some(match scheme {
        AuthScheme::Basic(b) => mapping([
            ("type", s("basic")),
            ("username", s(&b.username)),
            ("password", s(&b.password)),
        ]),
        AuthScheme::Bearer(b) => mapping([("type", s("bearer")), ("token", s(&b.token))]),
        AuthScheme::ApiKey(k) => mapping([
            ("type", s("apikey")),
            ("key", s(&k.key)),
            ("value", s(&k.value)),
            ("placement", s(placement(k.placement))),
        ]),
        AuthScheme::Digest(d) => mapping([
            ("type", s("digest")),
            ("username", s(&d.username)),
            ("password", s(&d.password)),
        ]),
        AuthScheme::Ntlm(n) => mapping([
            ("type", s("ntlm")),
            ("username", s(&n.username)),
            ("password", s(&n.password)),
            ("domain", s(&n.domain)),
        ]),
        AuthScheme::OAuth2(o) => oauth2_value(o),
        AuthScheme::AwsV4(a) => {
            let fields = aws_fields(a);
            let mut pairs = vec![("type", s("awsv4"))];
            pairs.extend(fields.iter().map(|(name, value)| (*name, s(value))));
            mapping(pairs)
        }
        AuthScheme::Unsupported(kind) => mapping([("type", s(kind))]),
        // `None`, and any scheme added later, writes no auth key.
        _ => return None,
    })
}

// --- Body -------------------------------------------------------------------

fn text_kind(body: &Body) -> Option<(&'static str, &str)> {
    match body {
        Body::Json(t) => Some(("json", t)),
        Body::Text(t) => Some(("text", t)),
        Body::Xml(t) => Some(("xml", t)),
        Body::Sparql(t) => Some(("sparql", t)),
        _ => None,
    }
}

fn update_body(http: &mut Value, old: &Body, new: &Body) {
    if old == new {
        return;
    }
    let same_kind = std::mem::discriminant(old) == std::mem::discriminant(new);
    let existing = http
        .as_mapping_mut()
        .and_then(|m| m.get_mut(&string("body")))
        .filter(|b| b.as_mapping().is_some());
    // Same kind in an existing body: write only what changed inside it.
    if same_kind && let Some(body) = existing {
        let handled = match (old, new) {
            (_, n) if text_kind(n).is_some() => {
                if let Some((_, data)) = text_kind(n) {
                    set(body, "data", string(data));
                }
                true
            }
            (Body::FormUrlEncoded(o), Body::FormUrlEncoded(n)) => {
                update_list(body, "data", o, n, form_field_value, merge_form_field);
                true
            }
            (Body::Multipart(o), Body::Multipart(n)) => {
                update_list(body, "data", o, n, multipart_value, merge_multipart);
                true
            }
            (Body::File { .. }, Body::File { path, content_type }) => {
                update_file_body(body, path, content_type.as_deref());
                true
            }
            (Body::GraphQl { .. }, Body::GraphQl { query, variables }) => {
                // An HTTP body renamed into the `graphql` block leaves its own.
                remove(body, "type");
                remove(body, "data");
                set(body, "query", string(query));
                set(body, "variables", string(variables));
                true
            }
            _ => false,
        };
        if handled {
            return;
        }
    }
    set_or_remove(http, "body", request_body_value(new));
}

/// Writes what changed of a WebSocket request's messages, in place when
/// the file keeps their shape (one flat message, or as many variants), so
/// keys Bruno left out stay out.
fn update_ws_messages(block: &mut Value, old: &[WsMessage], new: &[WsMessage]) {
    let flat = |messages: &[WsMessage]| {
        ws_message_value(messages).is_some_and(|v| v.as_mapping().is_some())
    };
    let existing = block
        .as_mapping_mut()
        .and_then(|m| m.get_mut(&string("message")));
    let patch_message = |message: &mut Value, o: &WsMessage, n: &WsMessage| {
        if o.kind != n.kind {
            set(message, "type", string(n.kind.as_str()));
        }
        if o.data != n.data {
            set(message, "data", string(&n.data));
        }
    };
    match existing {
        Some(message) if message.as_mapping().is_some() && flat(old) && flat(new) => {
            patch_message(message, &old[0], &new[0]);
        }
        Some(variants)
            if variants.as_sequence().is_some_and(|v| v.len() == new.len())
                && old.len() == new.len()
                && !flat(new) =>
        {
            let Some(variants) = variants.as_sequence_mut() else {
                return;
            };
            for ((variant, o), n) in variants.iter_mut().zip(old).zip(new) {
                if o.name != n.name {
                    set(variant, "title", string(&n.name));
                }
                if o.selected != n.selected {
                    set(variant, "selected", Value::Bool(n.selected));
                }
                if (o.kind, &o.data) != (n.kind, &n.data) {
                    patch_message(child_mut(variant, "message"), o, n);
                }
            }
        }
        _ => set_or_remove(block, "message", ws_message_value(new)),
    }
}

/// Writes what changed of a gRPC call: its `method`, `methodType`,
/// `protoFilePath` and messages, these in place when the file keeps their
/// shape (one plain message, or as many variants).
fn update_grpc_call(block: &mut Value, old: &GrpcCall, new: &GrpcCall) {
    if old.method != new.method {
        set(block, "method", string(&new.method));
    }
    if old.method_type != new.method_type {
        let kind = new.method_type.map(|t| string(t.as_str()));
        set_or_remove(block, "methodType", kind);
    }
    if old.proto_path != new.proto_path {
        set_or_remove(
            block,
            "protoFilePath",
            new.proto_path.as_deref().map(string),
        );
    }
    if old.messages == new.messages {
        return;
    }
    let plain =
        |messages: &[GrpcMessage]| matches!(messages, [only] if only.name.trim().is_empty());
    let existing = block
        .as_mapping_mut()
        .and_then(|m| m.get_mut(&string("message")));
    match existing {
        Some(message)
            if message.as_sequence().is_none() && plain(&old.messages) && plain(&new.messages) =>
        {
            *message = string(&new.messages[0].data);
        }
        Some(variants)
            if variants
                .as_sequence()
                .is_some_and(|v| v.len() == new.messages.len())
                && old.messages.len() == new.messages.len() =>
        {
            let Some(variants) = variants.as_sequence_mut() else {
                return;
            };
            for ((variant, o), n) in variants.iter_mut().zip(&old.messages).zip(&new.messages) {
                if o.name != n.name {
                    set(variant, "title", string(&n.name));
                }
                if o.data != n.data {
                    set(variant, "message", string(&n.data));
                }
            }
        }
        _ => set_or_remove(block, "message", grpc_message_value(&new.messages)),
    }
}

/// The messages of a gRPC request, as Bruno writes them: always as named
/// variants (`title`, `message`).
fn grpc_message_value(messages: &[GrpcMessage]) -> Option<Value> {
    (!messages.is_empty()).then(|| {
        messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let title = if m.name.trim().is_empty() {
                    format!("message {}", i + 1)
                } else {
                    m.name.clone()
                };
                mapping([
                    ("title", Some(string(&title))),
                    ("message", Some(string(&m.data))),
                ])
            })
            .collect::<Vec<_>>()
            .into()
    })
}

/// The messages of a WebSocket request, as Bruno writes them: the only
/// one, unnamed and not empty, as a single `type` and `data`; any other
/// list as named variants (`title`, `selected`, `message`).
fn ws_message_value(messages: &[WsMessage]) -> Option<Value> {
    let message = |m: &WsMessage| {
        mapping([
            ("type", Some(string(m.kind.as_str()))),
            ("data", Some(string(&m.data))),
        ])
    };
    match messages {
        [] => None,
        [only] if only.name.trim().is_empty() && !only.data.trim().is_empty() => {
            Some(message(only))
        }
        _ => Some(
            messages
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let title = if m.name.trim().is_empty() {
                        format!("message {}", i + 1)
                    } else {
                        m.name.clone()
                    };
                    mapping([
                        ("title", Some(string(&title))),
                        ("selected", Some(Value::Bool(m.selected))),
                        ("message", Some(message(m))),
                    ])
                })
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

/// A request's body: a GraphQL one as Bruno writes it in the `graphql`
/// block, `query` and `variables` (both, even empty) without a `type`.
fn request_body_value(body: &Body) -> Option<Value> {
    match body {
        Body::GraphQl { query, variables } => Some(mapping([
            ("query", Some(string(query))),
            ("variables", Some(string(variables))),
        ])),
        other => body_value(other),
    }
}

pub(crate) fn body_value(body: &Body) -> Option<Value> {
    let s = |v: &str| Some(string(v));
    if let Some((kind, data)) = text_kind(body) {
        return Some(mapping([("type", s(kind)), ("data", s(data))]));
    }
    let list = |values: Vec<Value>| (!values.is_empty()).then(|| values.into());
    Some(match body {
        Body::FormUrlEncoded(fields) => mapping([
            ("type", s("form-urlencoded")),
            ("data", list(fields.iter().map(form_field_value).collect())),
        ]),
        Body::Multipart(parts) => mapping([
            ("type", s("multipart-form")),
            ("data", list(parts.iter().map(multipart_value).collect())),
        ]),
        // Where a body says its kind, as in an example; a request's
        // `graphql` block has it without (see `request_body_value`).
        Body::GraphQl { query, variables } => mapping([
            ("type", s("graphql")),
            ("query", s(query)),
            ("variables", s(variables)),
        ]),
        Body::File { path, content_type } => mapping([
            ("type", s("file")),
            (
                "data",
                Some(vec![file_variant(path, content_type.as_deref())].into()),
            ),
        ]),
        // `None`, and any kind added later, writes no body key.
        _ => return None,
    })
}

fn form_field_value(field: &FormField) -> Value {
    mapping([
        ("name", Some(string(&field.name))),
        ("value", Some(string(&field.value))),
        ("disabled", (!field.enabled).then_some(Value::Bool(true))),
    ])
}

fn merge_form_field(item: &mut Value, old: &FormField, new: &FormField) {
    merge_named(
        item,
        (&old.name, &old.value, old.enabled),
        (&new.name, &new.value, new.enabled),
    );
}

fn multipart_content(value: &MultipartValue) -> (Value, Value) {
    match value {
        MultipartValue::Text(text) => (string("text"), string(text)),
        MultipartValue::Files(paths) => (
            string("file"),
            paths.iter().map(string).collect::<Vec<_>>().into(),
        ),
    }
}

fn multipart_value(part: &MultipartPart) -> Value {
    let (kind, value) = multipart_content(&part.value);
    mapping([
        ("name", Some(string(&part.name))),
        ("type", Some(kind)),
        ("value", Some(value)),
        ("contentType", part.content_type.as_deref().map(string)),
        ("disabled", (!part.enabled).then_some(Value::Bool(true))),
    ])
}

fn merge_multipart(item: &mut Value, old: &MultipartPart, new: &MultipartPart) {
    if old.name != new.name {
        set(item, "name", string(&new.name));
    }
    if old.value != new.value {
        let (kind, value) = multipart_content(&new.value);
        set(item, "type", kind);
        set(item, "value", value);
    }
    if old.content_type != new.content_type {
        set_or_remove(item, "contentType", new.content_type.as_deref().map(string));
    }
    if old.enabled != new.enabled {
        set_or_remove(
            item,
            "disabled",
            (!new.enabled).then_some(Value::Bool(true)),
        );
    }
}

fn file_variant(path: &str, content_type: Option<&str>) -> Value {
    mapping([
        ("filePath", Some(string(path))),
        (
            "contentType",
            Some(string(content_type.unwrap_or_default())),
        ),
        ("selected", Some(Value::Bool(true))),
    ])
}

/// Updates the selected file variant (or the first), keeping the others.
fn update_file_body(body: &mut Value, path: &str, content_type: Option<&str>) {
    let variants = items(get(body, "data"));
    let index = variants
        .iter()
        .position(|v| read_bool(get(v, "selected")))
        .or(if variants.is_empty() { None } else { Some(0) });
    let Some(index) = index else {
        set(body, "data", vec![file_variant(path, content_type)].into());
        return;
    };
    let mut list = variants.to_vec();
    let variant = &mut list[index];
    set(variant, "filePath", string(path));
    set(
        variant,
        "contentType",
        string(content_type.unwrap_or_default()),
    );
    set(body, "data", list.into());
}

fn read_bool(value: Option<&Value>) -> bool {
    crate::yaml::boolean(value) == Some(true)
}

// --- Folders and the collection ------------------------------------------------

pub(crate) fn new_folder(folder: &Folder) -> Value {
    let mut doc = mapping([(
        "info",
        Some(mapping([
            ("name", Some(string(&folder.name))),
            ("type", Some(string("folder"))),
        ])),
    )]);
    let blank = read::folder(&doc, &folder.name);
    update_folder(&mut doc, &blank, folder);
    doc
}

pub(crate) fn update_folder(doc: &mut Value, old: &Folder, new: &Folder) {
    if (&old.name, old.seq) != (&new.name, new.seq) {
        let info = child_mut(doc, "info");
        if old.name != new.name {
            set(info, "name", string(&new.name));
        }
        if old.seq != new.seq {
            set_or_remove(info, "seq", new.seq.map(number));
        }
    }
    update_defaults(doc, &old.defaults, &new.defaults, AuthLevel::Item);
}

pub(crate) fn new_collection(collection: &Collection) -> Value {
    let mut doc = mapping([
        ("opencollection", Some(string("1.0.0"))),
        (
            "info",
            Some(mapping([("name", Some(string(&collection.name)))])),
        ),
    ]);
    let blank = read::collection(&doc);
    update_collection(&mut doc, &blank, collection);
    doc
}

pub(crate) fn update_collection(doc: &mut Value, old: &Collection, new: &Collection) {
    if old.name != new.name {
        set(child_mut(doc, "info"), "name", string(&new.name));
    }
    if old.client_certificates != new.client_certificates {
        let list: Vec<Value> = new
            .client_certificates
            .iter()
            .map(certificate_value)
            .collect();
        let config = child_mut(doc, "config");
        set_or_remove(
            config,
            "clientCertificates",
            (!list.is_empty()).then(|| Value::from(list)),
        );
        prune_empty(doc, "config");
    }
    if old.proxy != new.proxy {
        let config = child_mut(doc, "config");
        let (before, after) = (proxy_value(&old.proxy), proxy_value(&new.proxy));
        match get(config, "proxy") {
            Some(Value::Mapping(_)) => merge(child_mut(config, "proxy"), &before, &after),
            _ => set(config, "proxy", after),
        }
    }
    update_defaults(doc, &old.defaults, &new.defaults, AuthLevel::Collection);
}

fn update_defaults(
    doc: &mut Value,
    old: &RequestDefaults,
    new: &RequestDefaults,
    level: AuthLevel,
) {
    if old.headers == new.headers && old.auth == new.auth && old.variables == new.variables {
        return;
    }
    let request = child_mut(doc, "request");
    update_list(
        request,
        "headers",
        &old.headers,
        &new.headers,
        header_value,
        merge_header,
    );
    update_auth(request, "auth", &old.auth, &new.auth, level);
    update_list(
        request,
        "variables",
        &old.variables,
        &new.variables,
        variable_value_item,
        merge_variable,
    );
    prune_empty(doc, "request");
}

/// Adds `pattern` to `extensions.bruno.ignore` unless it is already there.
pub(crate) fn add_ignore(doc: &mut Value, pattern: &str) {
    if read::ignore_patterns(doc).iter().any(|p| p == pattern) {
        return;
    }
    let bruno = child_mut(child_mut(doc, "extensions"), "bruno");
    let mut list = items(get(bruno, "ignore")).to_vec();
    if list.is_empty() {
        // Bruno's own defaults, so adding ours does not drop them.
        list.extend(["node_modules", ".git"].map(string));
    }
    list.push(string(pattern));
    set(bruno, "ignore", list.into());
}

// --- Environments -------------------------------------------------------------

pub(crate) fn new_environment(environment: &Environment) -> Value {
    let mut doc = mapping([("name", Some(string(&environment.name)))]);
    let blank = read::environment(&doc, &environment.name);
    update_environment(&mut doc, &blank, environment);
    doc
}

pub(crate) fn update_environment(doc: &mut Value, old: &Environment, new: &Environment) {
    if old.name != new.name {
        set(doc, "name", string(&new.name));
    }
    if old.color != new.color {
        set_or_remove(doc, "color", new.color.as_deref().map(string));
    }
    if old.extends != new.extends {
        set_or_remove(doc, "extends", new.extends.as_deref().map(string));
    }
    update_list(
        doc,
        "variables",
        &old.variables,
        &new.variables,
        env_variable_value,
        merge_env_variable,
    );
}

fn env_variable_value(variable: &Variable) -> Value {
    if variable.secret {
        mapping([
            ("name", Some(string(&variable.name))),
            ("secret", Some(Value::Bool(true))),
            ("disabled", (!variable.enabled).then_some(Value::Bool(true))),
        ])
    } else {
        variable_value_item(variable)
    }
}

fn merge_env_variable(item: &mut Value, old: &Variable, new: &Variable) {
    if old.secret != new.secret {
        if new.secret {
            remove(item, "value");
            set(item, "secret", Value::Bool(true));
        } else {
            remove(item, "secret");
            set(item, "value", typed_value(new.value.as_ref()));
        }
    } else if !new.secret && old.value != new.value {
        set(item, "value", typed_value(new.value.as_ref()));
    }
    if old.name != new.name {
        set(item, "name", string(&new.name));
    }
    if old.enabled != new.enabled {
        set_or_remove(
            item,
            "disabled",
            (!new.enabled).then_some(Value::Bool(true)),
        );
    }
}

// --- .apifast/collection.yml ----------------------------------------------------

pub(crate) fn apifast_collection(id: &str) -> Value {
    mapping([("format", Some(number(1u32))), ("id", Some(string(id)))])
}

/// The `dns` list of `.apifast/collection.yml`, or `None` for no entries.
pub(crate) fn dns_value(overrides: &[apifast_domain::DnsOverride]) -> Option<Value> {
    (!overrides.is_empty()).then(|| {
        Value::Sequence(
            overrides
                .iter()
                .map(|o| {
                    mapping([
                        ("host", Some(string(o.host.as_str()))),
                        ("address", Some(string(o.address.as_str()))),
                        ("enabled", (!o.enabled).then_some(Value::Bool(false))),
                    ])
                })
                .collect(),
        )
    })
}

/// A new `.apifast/collection.yml` with `overrides` and no id yet.
pub(crate) fn apifast_with_dns(overrides: &[apifast_domain::DnsOverride]) -> Value {
    mapping([
        ("format", Some(number(1u32))),
        ("dns", dns_value(overrides)),
    ])
}
