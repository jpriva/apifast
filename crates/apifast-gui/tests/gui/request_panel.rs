//! The request panel edits the request in place and shows what it
//! inherits. Typing goes through real key events, so a field that loses its
//! focus while a row is added would fail these tests.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{
    AuthScheme, BearerAuth, Body, Header, HttpRequest, Method, MultipartValue, Origin, ParamKind,
    Resolved, Setting,
};
use apifast_gui::atoms::VariableHints;
use apifast_gui::core::TabContext;
use apifast_gui::organisms::{RequestChange, RequestPanel, RequestTab};
use apifast_gui::strings::EN;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    request: HttpRequest,
    panel: RequestPanel,
    context: TabContext,
    changes: Vec<RequestChange>,
}

fn harness(tab: RequestTab, request: HttpRequest) -> Harness<'static, State> {
    let context = TabContext {
        inherited_headers: vec![Resolved::new(
            Header::new("accept", "application/json"),
            Origin::Collection,
        )],
        inherited_auth: Resolved::new(
            AuthScheme::Bearer(BearerAuth {
                token: "{{token}}".into(),
            }),
            Origin::Folder(0),
        ),
        folders: vec!["Users".into()],
        ..TabContext::default()
    };
    let state = State {
        request,
        panel: RequestPanel {
            tab,
            ..RequestPanel::default()
        },
        context,
        changes: Vec::new(),
    };
    let hints = VariableHints::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, 500.0))
        .build_ui_state(
            move |ui, state: &mut State| {
                if let Some(change) =
                    state
                        .panel
                        .show(ui, &mut state.request, &state.context, &hints, &EN)
                {
                    state.changes.push(change);
                }
            },
            state,
        );
    harness.run();
    harness
}

fn request() -> HttpRequest {
    HttpRequest::new("Users", Method::Get, "{{host}}/users")
}

/// Types `text` one key at a time into the focused field.
fn type_slowly(harness: &mut Harness<'_, State>, text: &str) {
    for c in text.chars() {
        harness
            .input_mut()
            .events
            .push(egui::Event::Text(c.to_string()));
        harness.run();
    }
}

#[test]
fn typing_in_the_empty_row_adds_a_row_and_keeps_the_focus() {
    let mut harness = harness(RequestTab::Headers, request());
    harness
        .get_by_role_and_label(Role::TextInput, EN.name)
        .focus();
    harness.run();
    type_slowly(&mut harness, "x-trace");
    assert_eq!(
        harness.state().request.headers,
        [Header::new("x-trace", "")]
    );
    assert!(
        harness
            .state()
            .changes
            .iter()
            .all(|c| *c == RequestChange::Other)
    );

    harness.get_by_label(EN.remove_row).click();
    harness.run();
    assert_eq!(harness.state().request.headers, []);
}

#[test]
fn inherited_headers_show_where_they_come_from_and_when_replaced() {
    let mut own = request();
    own.headers.push(Header::new("Accept", "text/plain"));
    let harness = harness(RequestTab::Headers, own);
    harness.get_by_label(EN.inherited);
    harness.get_by_label("application/json");
    harness.get_by_label("Collection");
}

#[test]
fn query_parameters_are_a_params_change() {
    let mut harness = harness(RequestTab::Params, request());
    harness.get_by_label(EN.path_params_hint);
    harness
        .get_all_by_role_and_label(Role::TextInput, EN.name)
        .next()
        .unwrap()
        .focus();
    harness.run();
    type_slowly(&mut harness, "page");
    let param = &harness.state().request.params[0];
    assert_eq!(
        (param.name.as_str(), param.kind),
        ("page", ParamKind::Query)
    );
    assert_eq!(harness.state().changes.last(), Some(&RequestChange::Params));
}

#[test]
fn a_json_body_can_be_typed_and_kept_when_switching_to_text() {
    let mut harness = harness(RequestTab::Body, request());
    harness.get_by_label(EN.body_type).click();
    harness.run();
    harness.get_by_label("JSON").click();
    harness.run();
    harness.get_by_role(Role::MultilineTextInput).focus();
    harness.run();
    type_slowly(&mut harness, "{}");
    assert_eq!(harness.state().request.body, Body::Json("{}".into()));

    harness.get_by_label(EN.body_type).click();
    harness.run();
    harness.get_by_label("Text").click();
    harness.run();
    assert_eq!(harness.state().request.body, Body::Text("{}".into()));
}

#[test]
fn inherited_auth_says_from_where_and_a_bearer_token_can_be_set() {
    let mut harness = harness(RequestTab::Auth, request());
    harness.get_by_label("Inherited from Folder Users: Bearer token");

    harness.get_by_label(EN.auth_type).click();
    harness.run();
    harness.get_by_label(EN.auth_bearer).click();
    harness.run();
    harness.get_by_role(Role::PasswordInput).focus();
    harness.run();
    type_slowly(&mut harness, "abc");
    assert_eq!(
        harness.state().request.auth,
        Setting::Set(AuthScheme::Bearer(BearerAuth {
            token: "abc".into()
        }))
    );
}

#[test]
fn a_setting_inherits_until_it_is_set_and_starts_from_the_inherited_value() {
    let mut harness = harness(RequestTab::Settings, request());
    harness.get_by_label("30000 ms (Default)");
    harness.get_all_by_label(EN.inherit).next().unwrap().click();
    harness.run();
    assert_eq!(
        harness.state().request.settings.timeout_ms,
        Setting::Set(30_000)
    );
    assert!(harness.query_by_label("30000 ms (Default)").is_none());
}

#[test]
fn assertions_and_captures_are_added_by_typing_and_operators_change_the_value() {
    use apifast_domain::{Capture, CaptureScope};
    let mut harness = harness(RequestTab::Tests, request());
    harness
        .get_all_by_role_and_label(Role::TextInput, EN.expression)
        .next()
        .unwrap()
        .focus();
    harness.run();
    type_slowly(&mut harness, "res.status");
    let assertion = &harness.state().request.assertions[0];
    assert_eq!(
        (
            assertion.expression.as_str(),
            assertion.operator.as_str(),
            assertion.value.as_deref()
        ),
        ("res.status", "eq", Some(""))
    );
    harness.get_by_label(EN.expected).focus();
    harness.run();
    type_slowly(&mut harness, "200");
    assert_eq!(
        harness.state().request.assertions[0].value.as_deref(),
        Some("200")
    );

    // An operator without a value drops it.
    harness.get_by_role(Role::ComboBox).click();
    harness.run();
    harness.get_by_label("isEmpty").click();
    harness.run();
    let assertion = &harness.state().request.assertions[0];
    assert_eq!(
        (assertion.operator.as_str(), assertion.value.as_deref()),
        ("isEmpty", None)
    );
    assert!(harness.query_by_label(EN.no_expected).is_some());

    // A capture from its variable.
    harness.get_by_label(EN.capture_variable).focus();
    harness.run();
    type_slowly(&mut harness, "id");
    let capture = &harness.state().request.captures[0];
    assert_eq!(
        *capture,
        Capture {
            scope: CaptureScope::Runtime,
            ..Capture::new("", "id")
        }
    );
    assert!(harness.query_by_label("Tests (2)").is_some());
}

#[test]
fn oauth2_shows_the_fields_of_its_grant() {
    use apifast_domain::{AuthScheme, OAuth2Flow};
    let mut harness = harness(RequestTab::Auth, request());
    harness.get_by_label(EN.auth_type).click();
    harness.run();
    harness.get_by_label(EN.auth_oauth2).click();
    harness.run();
    // Client credentials by default: no user, no authorization URL.
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, EN.oauth2_access_token_url)
            .is_some()
    );
    assert!(
        harness
            .query_by_label(EN.oauth2_authorization_url)
            .is_none()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, EN.username)
            .is_none()
    );

    harness
        .get_by_role_and_label(Role::ComboBox, EN.oauth2_flow)
        .click();
    harness.run();
    harness.get_by_label(EN.oauth2_password).click();
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, EN.username)
            .is_some()
    );

    harness
        .get_by_role_and_label(Role::TextInput, EN.oauth2_scope)
        .focus();
    harness.run();
    type_slowly(&mut harness, "read");
    let Setting::Set(AuthScheme::OAuth2(oauth)) = &harness.state().request.auth else {
        panic!("{:?}", harness.state().request.auth);
    };
    assert_eq!(oauth.flow, OAuth2Flow::Password);
    assert_eq!(oauth.scope, "read");
}

fn pick_body(harness: &mut Harness<'_, State>, label: &str) {
    harness.get_by_label(EN.body_type).click();
    harness.run();
    harness.get_by_label(label).click();
    harness.run();
}

#[test]
fn a_form_becomes_multipart_with_its_fields_and_parts_can_hold_files() {
    let mut form = request();
    form.body = Body::FormUrlEncoded(vec![apifast_domain::FormField {
        name: "user".into(),
        value: "{{name}}".into(),
        enabled: false,
    }]);
    let mut harness = harness(RequestTab::Body, form);
    pick_body(&mut harness, EN.body_multipart);
    let Body::Multipart(parts) = &harness.state().request.body else {
        panic!("{:?}", harness.state().request.body);
    };
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "user");
    assert_eq!(parts[0].value, MultipartValue::Text("{{name}}".into()));
    assert!(!parts[0].enabled);

    harness.get_by_label(EN.add_part).click();
    harness.run();
    harness
        .get_all_by_role_and_label(Role::TextInput, EN.name)
        .nth(1)
        .unwrap()
        .focus();
    harness.run();
    type_slowly(&mut harness, "doc");
    // The second part's kind selector, then Files.
    harness
        .get_all_by_label(EN.part_kind)
        .nth(1)
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label(EN.part_files).click();
    harness.run();
    assert!(harness.query_by_label(EN.no_files).is_some());
    assert!(harness.query_by_label(EN.choose_files).is_some());
    let Body::Multipart(parts) = &harness.state().request.body else {
        unreachable!()
    };
    assert_eq!(parts[1].name, "doc");
    assert_eq!(parts[1].value, MultipartValue::Files(Vec::new()));
    assert!(parts[1].enabled && parts[1].content_type.is_none());

    // And back to a form: the text parts stay, the files go.
    pick_body(&mut harness, EN.body_form);
    let Body::FormUrlEncoded(fields) = &harness.state().request.body else {
        unreachable!()
    };
    assert_eq!(fields.len(), 1);
}

#[test]
fn a_file_body_takes_its_path_and_content_type() {
    let mut harness = harness(RequestTab::Body, request());
    pick_body(&mut harness, EN.body_file);
    assert_eq!(
        harness.state().request.body,
        Body::File {
            path: String::new(),
            content_type: None
        }
    );
    harness
        .get_by_role_and_label(Role::TextInput, EN.body_file_path)
        .focus();
    harness.run();
    type_slowly(&mut harness, "a.bin");
    harness
        .get_by_role_and_label(Role::TextInput, EN.part_content_type)
        .focus();
    harness.run();
    type_slowly(&mut harness, "image/png");
    assert_eq!(
        harness.state().request.body,
        Body::File {
            path: "a.bin".into(),
            content_type: Some("image/png".into())
        }
    );
}

#[test]
fn picked_files_are_kept_relative_inside_the_collection() {
    use apifast_gui::organisms::written_path;
    use std::path::Path;
    let root = Path::new("/work/shop");
    assert_eq!(
        written_path(Path::new("/work/shop/files/a.png"), Some(root)),
        "files/a.png"
    );
    assert_eq!(
        written_path(Path::new("/tmp/b.png"), Some(root)),
        "/tmp/b.png"
    );
    assert_eq!(written_path(Path::new("/tmp/b.png"), None), "/tmp/b.png");
}

#[test]
fn examples_are_listed_renamed_and_deleted() {
    use apifast_domain::{Example, ExampleBody, ExampleResponse};
    let example = |name: &str, status| Example {
        name: name.into(),
        description: None,
        request: None,
        response: Some(ExampleResponse {
            status: Some(status),
            body: Some(ExampleBody {
                kind: "json".into(),
                data: "{}".into(),
            }),
            ..ExampleResponse::default()
        }),
    };
    let mut with = request();
    with.examples = vec![example("Found", 200), example("Missing", 404)];
    let mut harness = harness(RequestTab::Examples, with);
    harness.get_by_label("Examples (2)");
    harness
        .get_all_by_label(EN.example_delete)
        .next()
        .unwrap()
        .click();
    harness.run();
    let names: Vec<String> = (harness.state().request.examples.iter())
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, ["Missing"]);
    harness
        .get_by_role_and_label(Role::TextInput, EN.example_name)
        .focus();
    harness.run();
    harness.key_press(egui::Key::End);
    type_slowly(&mut harness, " user");
    assert_eq!(harness.state().request.examples[0].name, "Missing user");
    assert_eq!(harness.state().changes.last(), Some(&RequestChange::Other));

    let mut empty = self::harness(RequestTab::Examples, request());
    empty.run();
    assert!(empty.query_by_label(EN.examples_empty).is_some());
}

#[test]
fn a_graphql_body_has_a_query_and_its_variables() {
    let mut json = request();
    // Postman's converted bodies are the JSON GraphQL sends.
    json.body = Body::Json(r#"{"query": "{ me { id } }", "variables": {"a": 1}}"#.into());
    let mut harness = harness(RequestTab::Body, json);
    pick_body(&mut harness, EN.body_graphql);
    assert_eq!(
        harness.state().request.body,
        Body::GraphQl {
            query: "{ me { id } }".into(),
            variables: r#"{"a":1}"#.into()
        },
        "taken apart"
    );
    harness.get_by_role_and_label(Role::MultilineTextInput, EN.graphql_query);
    harness
        .get_by_role_and_label(Role::MultilineTextInput, EN.graphql_variables)
        .focus();
    harness.run();
    harness.key_press(egui::Key::End);
    harness.run();
    type_slowly(&mut harness, " ");
    let Body::GraphQl { variables, .. } = &harness.state().request.body else {
        panic!("{:?}", harness.state().request.body);
    };
    assert!(variables.ends_with(' '), "{variables:?}");

    // Back to text, the query is what it keeps.
    pick_body(&mut harness, EN.body_text);
    assert_eq!(
        harness.state().request.body,
        Body::Text("{ me { id } }".into())
    );
}

#[test]
fn a_graphql_body_asks_for_its_schema() {
    let mut graphql = request();
    graphql.body = Body::GraphQl {
        query: "{ me }".into(),
        variables: String::new(),
    };
    let mut harness = harness(RequestTab::Body, graphql);
    // Without a schema in the context (not worked out yet), no section.
    assert!(harness.query_by_label(EN.schema_fetch).is_none());
    harness.state_mut().context.schema = Some(apifast_gui::core::SchemaView::default());
    // Tall enough for both editors and the section.
    harness.set_size(egui::vec2(800.0, 1200.0));
    harness.run();
    assert!(harness.query_by_label(EN.schema_none).is_some());
    harness.get_by_label(EN.schema_fetch).click();
    harness.run();
    assert!(
        harness.state().panel.fetch_schema,
        "asked, for the screen to send"
    );
}

#[test]
fn the_query_is_completed_from_the_schema() {
    use apifast_domain::{GraphQlSchema, Json};
    use std::sync::Arc;

    let answer = r#"{"data": {"__schema": {"queryType": {"name": "Query"}, "types": [
        {"kind": "OBJECT", "name": "Query", "fields": [
            {"name": "user", "args": [], "type": {"kind": "OBJECT", "name": "User"}},
            {"name": "users", "args": [], "type": {"kind": "LIST", "ofType": {"kind": "OBJECT", "name": "User"}}}]},
        {"kind": "OBJECT", "name": "User", "fields": [{"name": "id", "args": [], "type": {"kind": "SCALAR", "name": "ID"}}]}]}}}"#;
    let schema = GraphQlSchema::from_introspection(&Json::parse(answer).unwrap()).unwrap();
    let mut graphql = request();
    graphql.body = Body::GraphQl {
        query: String::new(),
        variables: String::new(),
    };
    let mut harness = harness(RequestTab::Body, graphql);
    harness.state_mut().context.schema = Some(apifast_gui::core::SchemaView {
        schema: Some(Arc::new(schema)),
        ..Default::default()
    });
    harness.set_size(egui::vec2(800.0, 1200.0));
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, EN.graphql_query)
        .focus();
    harness.run();
    type_slowly(&mut harness, "{ us");
    assert!(harness.query_by_label("users").is_some(), "suggested");
    // The arrow moves in the list, not the cursor; Enter puts it in.
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.run();
    let query = |h: &Harness<'_, State>| match &h.state().request.body {
        Body::GraphQl { query, .. } => query.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(query(&harness), "{ users");
    type_slowly(&mut harness, " { i");
    assert!(harness.query_by_label("id").is_some(), "the fields of User");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("id").is_none(), "closed");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        query(&harness),
        "{ users { i\n",
        "Enter is a new line again"
    );
    // At the start of a line nothing opens alone; Ctrl+Space asks.
    assert!(harness.query_by_label("id").is_none());
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Space);
    harness.run();
    assert!(harness.query_by_label("id").is_some(), "asked for");
}

#[test]
fn a_websocket_request_edits_its_messages() {
    use apifast_domain::{WsMessage, WsMessageKind};
    let mut socket = HttpRequest::new("Socket", Method::Get, "ws://h/ws");
    socket.body = Body::WebSocket(vec![WsMessage::new("", WsMessageKind::Json, "{}")]);
    let mut harness = harness(RequestTab::Body, socket);
    // A socket has no response to check or keep.
    assert!(harness.query_by_label_contains(EN.tests).is_none());
    assert!(harness.query_by_label_contains(EN.examples).is_none());
    harness.get_by_label("Messages (1)");

    harness.get_by_label(EN.ws_add_message).click();
    harness.run();
    harness.get_by_label(EN.ws_message_name).click();
    harness.run();
    harness.get_by_label(EN.ws_message_name).type_text("-hi");
    harness.run();
    harness.get_by_label(EN.ws_messages).click();
    harness.run();
    harness.get_by_label(EN.ws_messages).type_text("ping");
    harness.run();
    let Body::WebSocket(messages) = &harness.state().request.body else {
        panic!("{:?}", harness.state().request.body);
    };
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].name, "message 2-hi");
    assert_eq!(messages[1].data, "ping");
    assert_eq!(
        messages[1].kind,
        WsMessageKind::Json,
        "the kind of the one shown"
    );
    assert_ne!(harness.state().changes.len(), 0);

    harness.get_by_label(EN.ws_remove_message).click();
    harness.run();
    harness.get_by_label(EN.ws_remove_message).click();
    harness.run();
    harness.get_by_label(EN.ws_no_messages);
    assert_eq!(harness.state().request.body, Body::WebSocket(Vec::new()));
}
