//! Random requests saved over a real, commented file read back exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;

use apifast_app::{CollectionRepository, RequestPath};
use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, BasicAuth, BearerAuth, Body, FormField, Header,
    HttpRequest, Method, Param, ParamKind, Setting, Settings, ValueKind, Variable, VariableValue,
};
use apifast_storage_fs::FsCollection;
use proptest::prelude::*;

const BASE: &str = "\
# Top comment
info:
  name: Base
  type: http
  seq: 1

http:
  method: GET
  url: \"{{host}}/base\"
  headers:
    - name: accept # keep me
      value: application/json
      description: unknown key
  auth: inherit

runtime:
  scripts:
    - type: before-request
      code: console.log(1)

docs: Base docs
";

fn text() -> impl Strategy<Value = String> {
    // Plain words, templates, YAML-special strings and Unicode.
    prop_oneof![
        "[a-z]{0,8}",
        "\\{\\{[a-z_]{1,6}\\}\\}",
        Just("yes".to_owned()),
        Just("null".to_owned()),
        Just("0777".to_owned()),
        Just("- dash".to_owned()),
        Just("a: b # c".to_owned()),
        Just("línea\nsegunda ñ 日本".to_owned()),
        Just("  spaced  ".to_owned()),
        Just("'\"quotes\"'".to_owned()),
    ]
}

fn header() -> impl Strategy<Value = Header> {
    (text(), text(), any::<bool>()).prop_map(|(name, value, enabled)| Header {
        name,
        value,
        enabled,
    })
}

fn param() -> impl Strategy<Value = Param> {
    (text(), text(), any::<bool>(), any::<bool>()).prop_map(|(name, value, path, enabled)| Param {
        name,
        value,
        kind: if path {
            ParamKind::Path
        } else {
            ParamKind::Query
        },
        enabled,
    })
}

fn auth() -> impl Strategy<Value = Setting<AuthScheme>> {
    prop_oneof![
        Just(Setting::Inherit),
        Just(Setting::Set(AuthScheme::None)),
        (text(), text()).prop_map(|(username, password)| Setting::Set(AuthScheme::Basic(
            BasicAuth { username, password }
        ))),
        text().prop_map(|token| Setting::Set(AuthScheme::Bearer(BearerAuth { token }))),
        (text(), text(), any::<bool>()).prop_map(|(key, value, query)| Setting::Set(
            AuthScheme::ApiKey(ApiKeyAuth {
                key,
                value,
                placement: if query {
                    ApiKeyPlacement::Query
                } else {
                    ApiKeyPlacement::Header
                },
            })
        )),
    ]
}

fn body() -> impl Strategy<Value = Body> {
    prop_oneof![
        Just(Body::None),
        text().prop_map(Body::Json),
        text().prop_map(Body::Text),
        text().prop_map(Body::Xml),
        prop::collection::vec((text(), text(), any::<bool>()), 0..3).prop_map(|fields| {
            Body::FormUrlEncoded(
                fields
                    .into_iter()
                    .map(|(name, value, enabled)| FormField {
                        name,
                        value,
                        enabled,
                    })
                    .collect(),
            )
        }),
        (
            "[a-z]{1,6}\\.bin",
            prop::option::of(Just("application/pdf".to_owned()))
        )
            .prop_map(|(path, content_type)| Body::File { path, content_type }),
    ]
}

fn setting<T: std::fmt::Debug + Clone + 'static>(
    value: impl Strategy<Value = T> + 'static,
) -> BoxedStrategy<Setting<T>> {
    prop_oneof![Just(Setting::Inherit), value.prop_map(Setting::Set)].boxed()
}

fn variable() -> impl Strategy<Value = Variable> {
    (text(), text(), 0u8..4, any::<bool>()).prop_map(|(name, data, kind, enabled)| Variable {
        name,
        value: Some(VariableValue {
            kind: [
                ValueKind::String,
                ValueKind::Number,
                ValueKind::Bool,
                ValueKind::Object,
            ][usize::from(kind)],
            data,
        }),
        enabled,
        secret: false,
    })
}

/// A random request. The parts are boxed in groups: one tuple of fifteen
/// strategies builds a type too big for a test thread's stack in debug builds.
fn request() -> BoxedStrategy<HttpRequest> {
    let head = (
        "[A-Za-z ]{1,12}",
        prop::option::of(1u32..20),
        prop::collection::vec("[a-z]{1,5}", 0..3),
        prop_oneof![
            Just(Method::Get),
            Just(Method::Post),
            Just(Method::Custom("PURGE".into()))
        ],
        text(),
    )
        .boxed();
    let parts = (
        prop::collection::vec(param(), 0..4),
        prop::collection::vec(header(), 0..4),
        body(),
        auth(),
        prop::collection::vec(variable(), 0..3),
    )
        .boxed();
    let settings = (
        setting(0u64..100_000),
        setting(any::<bool>()),
        setting(0u32..20),
        setting(any::<bool>()),
        prop::option::of(text()),
    )
        .boxed();
    (head, parts, settings)
        .prop_map(
            |(
                (name, seq, tags, method, url),
                (params, headers, body, auth, variables),
                (timeout, follow, max, encode, docs),
            )| {
                HttpRequest {
                    name,
                    seq,
                    tags,
                    method,
                    url,
                    params,
                    headers,
                    body,
                    auth,
                    variables,
                    settings: Settings {
                        timeout_ms: timeout,
                        follow_redirects: follow,
                        max_redirects: max,
                        encode_url: encode,
                    },
                    docs,
                }
            },
        )
        .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn any_request_saved_over_a_file_reads_back_exactly(first in request(), second in request()) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\ninfo:\n  name: P\n").unwrap();
        fs::write(dir.path().join("r.yml"), BASE).unwrap();
        let repo = FsCollection::open(dir.path()).unwrap();
        let path = RequestPath::new("r.yml");

        // Two saves in a row: the second edits a file apifast itself wrote.
        for request in [&first, &second] {
            repo.save_request(&path, request).unwrap();
            prop_assert_eq!(&repo.request(&path).unwrap().request, request);
            let text = fs::read_to_string(dir.path().join("r.yml")).unwrap();
            prop_assert!(text.contains("# Top comment"), "{}", text);
            prop_assert!(text.contains("console.log(1)"), "{}", text);
        }
    }

    #[test]
    fn any_new_request_reads_back_exactly(request in request()) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\ninfo:\n  name: P\n").unwrap();
        let repo = FsCollection::open(dir.path()).unwrap();
        let path = RequestPath::new("new/r.yml");
        repo.save_request(&path, &request).unwrap();
        prop_assert_eq!(repo.request(&path).unwrap().request, request);
    }
}
