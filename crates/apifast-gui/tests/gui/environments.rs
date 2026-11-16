//! The environment editor: choosing, editing and saving an environment,
//! adding one, renaming and deleting, and the view of every variable.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_domain::{Environment, Origin, Variable, resolve_variables};
use apifast_gui::atoms::VariableHints;
use apifast_gui::core::{Revealed, SecretState, SecretStatus, TabContext, VaultView};
use apifast_gui::organisms::{EnvironmentEditor, EnvironmentEvent, EnvironmentsView};
use apifast_gui::strings::EN;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    editor: EnvironmentEditor,
    environments: Vec<Environment>,
    context: Option<TabContext>,
    vault: VaultView,
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
                let events = state.editor.show(
                    ui.ctx(),
                    (&state.environments, &state.vault),
                    state.context.as_ref(),
                    &hints,
                    &EN,
                );
                state.events.extend(events);
            },
            State {
                editor,
                environments: environments(),
                context,
                vault: VaultView::default(),
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
    // A secret's value is never in a field: its field is hidden.
    harness.get_by_role(Role::PasswordInput);
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

fn status(name: &str, status: SecretStatus) -> SecretState {
    SecretState {
        environment: "local".into(),
        name: name.into(),
        status,
    }
}

#[test]
fn marking_a_variable_secret_moves_its_value_to_the_vault_on_save() {
    let mut harness = harness(None);
    // `host` is the first variable; its secret box is the first one.
    harness
        .get_all_by_role_and_label(Role::CheckBox, EN.secret)
        .next()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label(EN.save).click();
    harness.run();
    let events = &harness.state().events;
    let [
        EnvironmentEvent::Save(saved),
        EnvironmentEvent::Secret {
            name,
            value,
            shared,
            ..
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    let host = saved.variables.iter().find(|v| v.name == "host").unwrap();
    assert!(
        host.secret && host.value.is_none(),
        "the file keeps only the name"
    );
    assert_eq!(
        (name.as_str(), value.expose(), *shared),
        ("host", "http://localhost", false)
    );
}

#[test]
fn a_value_typed_for_a_secret_is_stored_shared_on_save() {
    let mut harness = harness(None);
    harness.get_by_role(Role::PasswordInput).focus();
    harness.run();
    type_text(&mut harness, "t0k");
    harness.get_by_label(EN.shared).click();
    harness.run();
    assert!(harness.state().editor.is_dirty());
    harness.get_by_label(EN.save).click();
    harness.run();
    let [
        EnvironmentEvent::Secret {
            environment,
            name,
            value,
            shared,
        },
    ] = harness.state().events.as_slice()
    else {
        panic!("{:?}", harness.state().events);
    };
    assert_eq!(
        (environment.as_str(), name.as_str(), value.expose(), *shared),
        ("local", "token", "t0k", true)
    );
    assert!(!harness.state().editor.is_dirty());
}

#[test]
fn a_stored_value_is_revealed_on_request_and_its_status_shown() {
    let mut harness = harness(None);
    harness.state_mut().vault.secrets = vec![status("token", SecretStatus::Local)];
    harness.run();
    harness.get_by_label(&format!("{}: {}", EN.secret_value, EN.secret_local));
    harness.get_by_label(EN.reveal).click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [EnvironmentEvent::Reveal {
            environment: "local".into(),
            name: "token".into()
        }]
    );
    harness.state_mut().vault.revealed = Some(Revealed {
        environment: "local".into(),
        name: "token".into(),
        value: apifast_domain::Secret::new("t0k-shown"),
    });
    harness.run();
    harness.get_by_label("t0k-shown");
    harness.get_by_label(EN.hide).click();
    harness.run();
    assert_eq!(harness.state().events.last(), Some(&EnvironmentEvent::Hide));
}

#[test]
fn nothing_to_reveal_without_a_stored_value_and_orphans_can_be_forgotten() {
    let mut harness = harness(None);
    harness.get_by_label(&format!("{}: {}", EN.secret_value, EN.secret_missing));
    // Disabled: clicking it asks for nothing.
    harness.get_by_label(EN.reveal).click();
    harness.run();
    assert_eq!(harness.state().events, []);

    let orphan = apifast_vault::Orphan {
        environment: "old".into(),
        name: "key".into(),
        kind: apifast_vault::SecretKind::Shared,
    };
    harness.state_mut().vault.orphans = vec![orphan.clone()];
    harness.run();
    harness.get_by_label(EN.orphans_title);
    harness.get_by_label("old / key");
    harness.get_by_label(EN.forget).click();
    harness.run();
    assert_eq!(harness.state().events, [EnvironmentEvent::Forget(orphan)]);
}
