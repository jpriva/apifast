//! The tab bar: what it shows and the events it returns, with the
//! confirmation before closing a tab with unsaved changes.

use apifast_app::RequestPath;
use apifast_domain::{HttpRequest, Method};
use apifast_gui::core::{ResponseState, Tab};
use apifast_gui::organisms::{TabBar, TabEvent};
use apifast_gui::strings::EN;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    tabs: Vec<Tab>,
    bar: TabBar,
    events: Vec<TabEvent>,
}

fn tab(path: &str, name: &str, dirty: bool) -> Tab {
    let saved = HttpRequest::new(name, Method::Get, "{{host}}");
    Tab {
        path: RequestPath::new(path),
        draft: dirty.then(|| HttpRequest::new(name, Method::Post, "{{host}}")),
        saved,
        response: ResponseState::Idle,
        revision: 0,
        context: std::sync::Arc::default(),
        conflict: false,
    }
}

fn harness() -> Harness<'static, State> {
    let state = State {
        tabs: vec![
            tab("login.yml", "Login", false),
            tab("users.yml", "Users", true),
        ],
        bar: TabBar::default(),
        events: Vec::new(),
    };
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut State| {
            let active = state.tabs.first().map(|t| t.path.clone());
            if let Some(event) = state.bar.show(ui, &state.tabs, active.as_ref(), &EN) {
                state.events.push(event);
            }
        },
        state,
    );
    harness.run();
    harness
}

#[test]
fn tabs_show_their_names_and_a_dot_when_unsaved() {
    let harness = harness();
    harness.get_by_label("Login");
    harness.get_by_label("Users");
    // One dot, for the one tab with changes.
    harness.get_by_label(EN.unsaved);
}

#[test]
fn clicking_a_tab_activates_it_and_a_clean_tab_closes_at_once() {
    let mut harness = harness();
    harness.get_by_label("Users").click();
    harness.run();
    harness.get_all_by_label("×").next().unwrap().click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [
            TabEvent::Activate(RequestPath::new("users.yml")),
            TabEvent::Close(RequestPath::new("login.yml")),
        ]
    );
}

#[test]
fn closing_an_unsaved_tab_asks_first() {
    for (button, expected) in [
        (
            "Save",
            Some(TabEvent::SaveAndClose(RequestPath::new("users.yml"))),
        ),
        (
            "Don't save",
            Some(TabEvent::Close(RequestPath::new("users.yml"))),
        ),
        ("Cancel", None),
    ] {
        let mut harness = harness();
        harness.get_all_by_label("×").last().unwrap().click();
        harness.run();
        harness.get_by_label(EN.unsaved_title);
        assert!(harness.state().events.is_empty(), "closed without asking");
        harness.get_by_label(button).click();
        harness.run();
        assert_eq!(harness.state().events, Vec::from_iter(expected), "{button}");
        assert!(harness.query_by_label(EN.unsaved_title).is_none());
    }
}
