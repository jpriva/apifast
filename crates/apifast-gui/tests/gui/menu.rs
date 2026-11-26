//! The application's menu, the About window, quitting, and the title bar
//! apifast draws: its buttons, moving and maximizing by it, resizing by the
//! window's edges, and its search.

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

/// The viewport commands of the last frame.
fn viewport_commands(harness: &Harness<'_, State>) -> Vec<egui::ViewportCommand> {
    harness.output().viewport_output[&egui::ViewportId::ROOT]
        .commands
        .clone()
}

fn press_at(harness: &mut Harness<'_, State>, pos: egui::Pos2, pressed: bool) {
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

/// Presses at `from`, then moves to `to`, a frame each: a drag starting.
fn drag(
    harness: &mut Harness<'_, State>,
    from: egui::Pos2,
    to: egui::Pos2,
) -> Vec<egui::ViewportCommand> {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.step();
    press_at(harness, from, true);
    harness.step();
    let mut seen = viewport_commands(harness);
    for step in 1..=4 {
        let at = from + (to - from) * (step as f32 / 4.0);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.step();
        seen.extend(viewport_commands(harness));
    }
    press_at(harness, to, false);
    harness.step();
    seen
}

#[cfg(target_os = "linux")]
#[test]
fn the_title_bar_has_the_window_buttons_and_the_setting_takes_them_away() {
    let mut harness = harness();
    harness.run();
    for (name, command) in [
        (EN.window_minimize, egui::ViewportCommand::Minimized(true)),
        (EN.window_maximize, egui::ViewportCommand::Maximized(true)),
        (EN.window_close, egui::ViewportCommand::Close),
    ] {
        harness.get_by_label(name).click();
        harness.step();
        assert!(viewport_commands(&harness).contains(&command), "{name}");
    }

    harness.state_mut().screen.settings.own_title_bar = false;
    harness.step();
    assert!(viewport_commands(&harness).contains(&egui::ViewportCommand::Decorations(true)));
    harness.run();
    assert!(harness.query_by_label(EN.window_close).is_none());
    harness.state_mut().screen.settings.own_title_bar = true;
    harness.step();
    assert!(viewport_commands(&harness).contains(&egui::ViewportCommand::Decorations(false)));
}

#[cfg(target_os = "linux")]
#[test]
fn dragging_the_title_bar_moves_the_window_and_its_edges_resize_it() {
    let mut harness = harness();
    harness.run();
    // Empty space in the title bar, between the search and the menu.
    let seen = drag(
        &mut harness,
        egui::pos2(600.0, 24.0),
        egui::pos2(640.0, 40.0),
    );
    assert!(seen.contains(&egui::ViewportCommand::StartDrag), "{seen:?}");

    let seen = drag(
        &mut harness,
        egui::pos2(2.0, 400.0),
        egui::pos2(-20.0, 400.0),
    );
    assert!(
        seen.contains(&egui::ViewportCommand::BeginResize(
            egui::ResizeDirection::West
        )),
        "{seen:?}"
    );
    let seen = drag(
        &mut harness,
        egui::pos2(1097.0, 697.0),
        egui::pos2(1120.0, 720.0),
    );
    assert!(
        seen.contains(&egui::ViewportCommand::BeginResize(
            egui::ResizeDirection::SouthEast
        )),
        "{seen:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_double_click_on_the_title_bar_maximizes() {
    let mut harness = harness();
    harness.run();
    let at = egui::pos2(600.0, 24.0);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(at));
    harness.step();
    let mut seen = Vec::new();
    for _ in 0..2 {
        press_at(&mut harness, at, true);
        press_at(&mut harness, at, false);
        harness.step();
        seen.extend(viewport_commands(&harness));
    }
    assert!(
        seen.contains(&egui::ViewportCommand::Maximized(true)),
        "{seen:?}"
    );
}

#[test]
fn the_search_in_the_title_bar_opens_the_palette() {
    let mut harness = harness();
    harness.get_by_label(EN.title_search).click();
    harness.run();
    assert!(harness.state().screen.palette.open.is_some());
}
