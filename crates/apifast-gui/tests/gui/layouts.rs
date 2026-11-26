//! Editor layouts: saved with a name from the Layouts window, applied,
//! renamed, deleted and set back to the default; the one on screen and the
//! saved ones handed over for their file when they change, and taken back
//! from it.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_gui::core::{Command, Snapshot};
use apifast_gui::layout::{EditorLayout, LinearDir};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui::{self, Key};
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

fn direction(harness: &Harness<'_, State>) -> Option<LinearDir> {
    harness.state().screen.layout.direction()
}

/// The names of the saved layouts, as handed over for their file, if they
/// changed.
fn saved_names(harness: &mut Harness<'_, State>) -> Option<Vec<String>> {
    let (_, saved) = harness.state_mut().screen.take_layouts_change()?;
    Some(saved.into_iter().map(|(name, _)| name).collect())
}

#[test]
fn layouts_are_saved_applied_renamed_and_deleted() {
    let mut harness = harness();
    harness.run();
    // Nothing changed yet past the first look.
    let _ = harness.state_mut().screen.take_layouts_change();
    assert_eq!(saved_names(&mut harness), None);

    harness.get_by_label(EN.menu).click();
    harness.run();
    harness.get_by_label(EN.action_layouts).click();
    harness.run();
    harness.get_by_label(EN.layouts_none);

    // The layout on screen, stacked, saved with a name.
    harness.state_mut().screen.layout.toggle();
    harness.run();
    assert_eq!(
        saved_names(&mut harness),
        Some(Vec::new()),
        "the change on screen"
    );
    harness.get_by_label(EN.layout_name_hint).click();
    harness.run();
    harness
        .get_by_label(EN.layout_name_hint)
        .type_text("Stacked");
    harness.run();
    harness.get_by_label(EN.layout_save).click();
    harness.run();
    harness.get_by_label("Stacked");
    assert_eq!(saved_names(&mut harness), Some(vec!["Stacked".to_owned()]));

    harness.get_by_label(EN.layout_reset).click();
    harness.run();
    assert_eq!(direction(&harness), Some(LinearDir::Horizontal));
    harness.get_by_label(EN.layout_apply).click();
    harness.run();
    assert_eq!(direction(&harness), Some(LinearDir::Vertical));

    harness.get_by_label(EN.rename).click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    harness.run();
    harness
        .get_all_by_label(EN.layout_name_hint)
        .next()
        .unwrap()
        .type_text("Tall");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    harness.get_by_label("Tall");
    assert_eq!(saved_names(&mut harness), Some(vec!["Tall".to_owned()]));

    harness.get_by_label(EN.delete).click();
    harness.run();
    harness.get_by_label(EN.layouts_none);
    assert_eq!(saved_names(&mut harness), Some(Vec::new()));
}

#[test]
fn the_layout_left_on_screen_comes_back_and_a_bad_one_is_told() {
    let mut screen = MainScreen::default();
    let stacked = EditorLayout::stacked().to_json();
    let saved = vec![("Stacked".to_owned(), stacked.clone())];
    assert!(!screen.set_layouts((Some(stacked), saved)));
    assert_eq!(screen.layout.direction(), Some(LinearDir::Vertical));
    // Taken from the file: nothing to write back.
    assert_eq!(screen.take_layouts_change(), None);

    let mut screen = MainScreen::default();
    assert!(screen.set_layouts((Some(serde_json::json!({"root": 9})), Vec::new())));
    assert_eq!(
        screen.layout.direction(),
        Some(LinearDir::Horizontal),
        "the default"
    );
}
