//! The request panel edits the request in place and shows what it
//! inherits. Typing goes through real key events, so a field that loses its
//! focus while a row is added would fail these tests.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{
    AuthScheme, BearerAuth, Body, Header, HttpRequest, Method, Origin, ParamKind, Resolved, Setting,
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
        panel: RequestPanel { tab },
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
