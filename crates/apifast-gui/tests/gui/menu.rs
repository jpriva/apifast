//! The application's menu, the About window, and quitting.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_gui::core::{Command, Snapshot};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    screen: MainScreen,
    snapshot: Snapshot,
    commands: Vec<Command>,
}

fn harness() -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                state.screen.apply_appearance(ui.ctx(), &EN);
                let commands = state.screen.show(ui, &state.snapshot, &EN);
                state.commands.extend(commands);
            },
            State {
                screen: MainScreen::default(),
                snapshot: Snapshot::default(),
                commands: Vec::new(),
            },
        )
}

fn open_menu(harness: &mut Harness<'_, State>) {
    harness.get_by_label(EN.menu).click();
    harness.run();
}

#[test]
fn the_menu_opens_settings_shortcuts_and_import() {
    let mut harness = harness();
    open_menu(&mut harness);
    harness.get_by_label(EN.action_settings).click();
    harness.run();
    assert!(harness.state().screen.settings_window.open);

    open_menu(&mut harness);
    harness.get_by_label(EN.action_shortcuts).click();
    harness.run();
    assert!(harness.state().screen.shortcuts.open);
    // The shortcuts' window lists every action, Import… and Quit among them.
    harness.state_mut().screen.shortcuts.open = false;
    harness.run();

    open_menu(&mut harness);
    // The menu closes once something is picked.
    harness.get_by_label(EN.action_quit);
    // The sidebar has an Import… too; the menu's comes first.
    harness
        .get_all_by_label(EN.import_collection_action)
        .next()
        .unwrap()
        .click();
    harness.run();
    assert!(harness.query_by_label(EN.action_quit).is_none());
    assert!(harness.state().screen.import_window.open);
}

#[test]
fn about_shows_the_version_license_and_dependencies_and_copies_a_report() {
    let mut harness = harness();
    harness.state_mut().screen.about.graphics = Some("Vulkan · Test GPU".into());
    open_menu(&mut harness);
    harness.get_by_label(EN.action_about).click();
    harness.run();
    assert!(harness.state().screen.about.open);
    harness.get_by_label(apifast_gui::VERSION);
    harness.get_by_label(apifast_gui::licenses::LICENSE);
    harness.get_by_label("Vulkan · Test GPU");
    let count = apifast_gui::licenses::DEPENDENCIES.len();
    harness.get_by_label(&format!("{count} dependencies"));

    harness.get_by_label(EN.about_copy).click();
    harness.run();
    harness.get_by_label(EN.about_copied);
    let report = harness.state_mut().screen.about.report();
    assert!(report.starts_with(&format!("apifast {}\n", apifast_gui::VERSION)));
    assert!(report.contains("Graphics: Vulkan · Test GPU"));
}

#[test]
fn ctrl_q_asks_the_window_to_close() {
    let mut harness = harness();
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::Key {
        key: Key::Q,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    harness.step();
    let commands = &harness.output().viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        commands.contains(&egui::ViewportCommand::Close),
        "{commands:?}"
    );
}
