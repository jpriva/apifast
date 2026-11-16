//! The environment editor: choosing, editing and saving an environment,
//! adding one, renaming and deleting, and the view of every variable.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{Environment, Origin, Variable, resolve_variables};
use apifast_gui::atoms::VariableHints;
use apifast_gui::core::TabContext;
use apifast_gui::organisms::{EnvironmentEditor, EnvironmentEvent, EnvironmentsView};
use apifast_gui::strings::EN;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    editor: EnvironmentEditor,
    environments: Vec<Environment>,
    context: Option<TabContext>,
    events: Vec<EnvironmentEvent>,
}

fn environments() -> Vec<Environment> {
    let mut local = Environment::new(
        "local",
        vec![
            Variable::text("host", "http://localhost"),
            Variable::secret("token"),
        ],
    );
    local.color = Some("#22aa55".into());
    vec![local, Environment::new("prod", Vec::new())]
}

fn harness(context: Option<TabContext>) -> Harness<'static, State> {
    let hints = VariableHints::new();
    let mut editor = EnvironmentEditor::default();
    editor.open = true;
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui_state(
            move |ui, state: &mut State| {
                if let Some(event) = state.editor.show(
                    ui.ctx(),
                    &state.environments,
                    state.context.as_ref(),
                    &hints,
                    &EN,
                ) {
                    state.events.push(event);
                }
            },
            State {
                editor,
                environments: environments(),
                context,
                events: Vec::new(),
            },
        )
}

fn type_text(harness: &mut Harness<'_, State>, text: &str) {
    harness
        .input_mut()
        .events
        .push(egui::Event::Text(text.into()));
    harness.run();
}

#[test]
fn the_first_environment_is_shown_with_its_variables() {
    let harness = harness(None);
    assert_eq!(harness.state().editor.selected(), Some("local"));
    // The field, not the text run inside it.
    harness.get_by(|node| {
        node.role() == Role::TextInput && node.value().as_deref() == Some("http://localhost")
    });
    // A secret's value is never shown; marking secrets waits for the vault.
    harness.get_by_label(EN.secret_in_vault);
}

#[test]
fn edits_are_saved_with_one_button_or_reverted() {
    let mut harness = harness(None);
    harness.get_by_label(EN.new_variable).focus();
    harness.run();
    type_text(&mut harness, "user");
    assert!(harness.state().editor.is_dirty());

    harness.get_by_label(EN.revert).click();
    harness.run();
    assert!(!harness.state().editor.is_dirty());

    harness.get_by_label(EN.new_variable).focus();
    harness.run();
    type_text(&mut harness, "user");
    harness.get_by_label(EN.save).click();
    harness.run();
    let [EnvironmentEvent::Save(saved)] = harness.state().events.as_slice() else {
        panic!("{:?}", harness.state().events);
    };
    let names: Vec<_> = saved.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["host", "token", "user"]);
    assert!(!harness.state().editor.is_dirty());
}

#[test]
fn a_new_environment_is_created_and_chosen() {
    let mut harness = harness(None);
    harness.get_by_label(EN.new_environment).focus();
    harness.run();
    type_text(&mut harness, "qa");
    harness.get_by_label(EN.add).click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [EnvironmentEvent::Create("qa".into())]
    );
    // Once the files have it, it is the one shown.
    harness
        .state_mut()
        .environments
        .push(Environment::new("qa", Vec::new()));
    harness.run();
    assert_eq!(harness.state().editor.selected(), Some("qa"));
}

#[test]
fn renaming_follows_the_new_name_once_the_files_have_it() {
    let mut harness = harness(None);
    harness
        .get_by_role_and_label(Role::TextInput, EN.environment_name)
        .focus();
    harness.run();
    for _ in "local".chars() {
        harness.key_press(egui::Key::Backspace);
    }
    harness.run();
    type_text(&mut harness, "dev");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().events,
        [EnvironmentEvent::Rename {
            from: "local".into(),
            to: "dev".into()
        }]
    );
    harness.state_mut().environments[0].name = "dev".into();
    harness.run();
    assert_eq!(harness.state().editor.selected(), Some("dev"));
}

#[test]
fn deleting_asks_first() {
    let mut harness = harness(None);
    harness.get_by_label(EN.delete).click();
    harness.run();
    harness.get_by_label(&format!("{} `local`?", EN.delete_environment));
    harness.get_all_by_label(EN.delete).last().unwrap().click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [EnvironmentEvent::Delete("local".into())]
    );
}

#[test]
fn every_variable_shows_where_it_comes_from_and_what_it_overrides() {
    let collection = [Variable::text("host", "http://collection")];
    let local = [Variable::text("host", "http://localhost")];
    let variables = resolve_variables([
        (Origin::Collection, &collection[..]),
        (Origin::Environment("local".into()), &local[..]),
    ]);
    let context = TabContext {
        variables,
        ..TabContext::default()
    };
    let mut harness = harness(Some(context));
    harness.state_mut().editor.view = EnvironmentsView::Variables;
    harness.run();
    harness.get_by_label("host");
    harness.get_by_label("http://localhost");
    harness.get_by_label("Environment local");
    harness.get_by_label("Collection");
}
