//! The keyboard: the command palette, the editor of shortcuts, and the
//! actions the main screen runs from either.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::Arc;

use apifast_app::{CollectionTree, RequestPath, TreeItem};
use apifast_domain::{Collection, HttpRequest, Method};
use apifast_gui::actions::{Action, parse_shortcut};
use apifast_gui::core::{CollectionView, Command, ResponseState, Snapshot, Tab, TabKey};
use apifast_gui::organisms::{PaletteMode, fuzzy};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn request(path: &str, name: &str) -> TreeItem {
    TreeItem::Request {
        path: RequestPath::new(path),
        name: name.into(),
        method: Method::Get,
        kind: apifast_domain::RequestKind::Http,
        seq: None,
    }
}

fn tab(path: &str, name: &str) -> Tab {
    Tab {
        root: "/c".into(),
        path: RequestPath::new(path),
        saved: HttpRequest::new(name, Method::Get, "{{host}}/x"),
        draft: None,
        response: ResponseState::Idle,
        revision: 1,
        context: Arc::default(),
        conflict: false,
    }
}

fn snapshot() -> Snapshot {
    let tree = CollectionTree {
        collection: Collection::new("C"),
        items: vec![
            request("health.yml", "Health"),
            request("users/list.yml", "List users"),
            request("users/create.yml", "Create user"),
        ],
    };
    Snapshot {
        collections: vec![apifast_gui::core::CollectionEntry::loaded(
            "/c",
            tree.clone(),
        )],
        collection: Some(CollectionView {
            root: "/c".into(),
            tree: std::sync::Arc::new(tree),
        }),
        environments: vec!["local".into(), "prod".into()],
        tabs: vec![
            tab("health.yml", "Health"),
            tab("users/list.yml", "List users"),
        ],
        active: Some(TabKey::new("/c", RequestPath::new("health.yml"))),
        ..Snapshot::default()
    }
}

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
                // As the app does, every frame.
                state.screen.apply_appearance(ui.ctx(), &EN);
                let commands = state.screen.show(ui, &state.snapshot, &EN);
                state.commands.extend(commands);
            },
            State {
                screen: MainScreen::default(),
                snapshot: snapshot(),
                commands: Vec::new(),
            },
        )
}

fn press(harness: &mut Harness<'_, State>, modifiers: Modifiers, key: Key) {
    let events = &mut harness.input_mut().events;
    events.push(egui::Event::ModifiersChanged(modifiers));
    events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
}

fn type_text(harness: &mut Harness<'_, State>, text: &str) {
    harness
        .input_mut()
        .events
        .push(egui::Event::Text(text.into()));
    harness.run();
}

fn take(harness: &mut Harness<'_, State>) -> Vec<Command> {
    std::mem::take(&mut harness.state_mut().commands)
}

#[test]
fn fuzzy_matching_prefers_word_starts_and_runs() {
    assert!(fuzzy("lu", "List users").is_some());
    assert!(fuzzy("xyz", "List users").is_none());
    assert!(fuzzy("", "anything").is_some());
    // `cu` starts both words of "Create user", only one of "Cancel".
    assert!(fuzzy("cu", "Create user") > fuzzy("cu", "Focus URL"));
    assert!(fuzzy("save", "Save") > fuzzy("save", "Save all"));
}

#[test]
fn the_last_pick_comes_first_when_nothing_is_typed() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::K);
    type_text(&mut harness, "save all");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(take(&mut harness), [Command::SaveAll]);
    press(&mut harness, Modifiers::COMMAND, Key::K);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(take(&mut harness), [Command::SaveAll]);
}

#[test]
fn ctrl_k_opens_the_palette_and_enter_runs_the_selection() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::K);
    assert_eq!(harness.state().screen.palette.open, Some(PaletteMode::All));
    type_text(&mut harness, "list users");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(
        take(&mut harness),
        [Command::OpenRequest(TabKey::new(
            "/c",
            RequestPath::new("users/list.yml")
        ))]
    );
    assert_eq!(harness.state().screen.palette.open, None);
}

#[test]
fn the_palette_switches_environments_and_shows_shortcuts() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::K);
    // Each action shows its shortcut, as egui writes them on this system.
    let ctx = egui::Context::default();
    let shortcut = ctx.format_shortcut(&parse_shortcut("Ctrl+Shift+S").unwrap());
    harness.get_by_label(&shortcut);
    type_text(&mut harness, "prod");
    harness.get_by_label("prod").click();
    harness.run();
    assert_eq!(
        take(&mut harness),
        [Command::SelectEnvironment(Some("prod".into()))]
    );
}

#[test]
fn ctrl_p_searches_requests_only() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::P);
    assert_eq!(
        harness.state().screen.palette.open,
        Some(PaletteMode::Requests)
    );
    assert!(harness.query_by_label(EN.action_save_all).is_none());
    // In the tree and in the palette.
    assert_eq!(harness.get_all_by_label("Create user").count(), 2);
}

#[test]
fn tabs_are_walked_numbered_closed_and_reopened() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::Tab);
    assert_eq!(
        take(&mut harness),
        [Command::ActivateTab(TabKey::new(
            "/c",
            RequestPath::new("users/list.yml")
        ))]
    );
    press(
        &mut harness,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Tab,
    );
    assert_eq!(
        take(&mut harness),
        [Command::ActivateTab(TabKey::new(
            "/c",
            RequestPath::new("users/list.yml")
        ))],
        "going back from the first wraps to the last"
    );
    press(&mut harness, Modifiers::COMMAND, Key::Num2);
    assert_eq!(
        take(&mut harness),
        [Command::ActivateTab(TabKey::new(
            "/c",
            RequestPath::new("users/list.yml")
        ))]
    );

    press(&mut harness, Modifiers::COMMAND, Key::W);
    assert_eq!(
        take(&mut harness),
        [Command::CloseTab(TabKey::new(
            "/c",
            RequestPath::new("health.yml")
        ))]
    );
    // The core closed it.
    harness.state_mut().snapshot.tabs.remove(0);
    harness.state_mut().snapshot.active =
        Some(TabKey::new("/c", RequestPath::new("users/list.yml")));
    harness.run();
    press(
        &mut harness,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::T,
    );
    assert_eq!(
        take(&mut harness),
        [Command::OpenRequest(TabKey::new(
            "/c",
            RequestPath::new("health.yml")
        ))]
    );
}

#[test]
fn a_rebound_shortcut_runs_its_new_action_and_not_the_old_one() {
    let mut harness = harness();
    harness.state_mut().screen.shortcuts.open = true;
    harness.run();
    // The first "Change" belongs to the first action: Send.
    harness
        .get_all_by_label(EN.shortcut_change)
        .next()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label(EN.shortcut_press);
    press(&mut harness, Modifiers::COMMAND, Key::R);
    assert!(harness.state_mut().screen.take_keymap_change());
    let keymap = &harness.state().screen.keymap;
    assert_eq!(
        keymap.shortcuts(Action::Send),
        [parse_shortcut("Ctrl+R").unwrap()]
    );

    harness.state_mut().screen.shortcuts.open = false;
    harness.run();
    take(&mut harness);
    press(&mut harness, Modifiers::COMMAND, Key::R);
    assert_eq!(take(&mut harness), [Command::Send]);
    press(&mut harness, Modifiers::COMMAND, Key::Enter);
    assert_eq!(take(&mut harness), []);
}

#[test]
fn taking_a_shortcut_from_another_action_says_so_and_escape_cancels() {
    let mut harness = harness();
    harness.state_mut().screen.shortcuts.open = true;
    harness.run();
    harness
        .get_all_by_label(EN.shortcut_change)
        .next()
        .unwrap()
        .click();
    harness.run();
    press(&mut harness, Modifiers::COMMAND, Key::S);
    harness.get_by_label(&format!("{} {}.", EN.shortcut_moved, EN.action_save));

    harness
        .get_all_by_label(EN.shortcut_change)
        .next()
        .unwrap()
        .click();
    harness.run();
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(!harness.state().screen.shortcuts.capturing());
    let keymap = &harness.state().screen.keymap;
    assert_eq!(
        keymap.shortcuts(Action::Send),
        [parse_shortcut("Ctrl+S").unwrap()]
    );
}

#[test]
fn the_sidebar_hides_and_the_history_shows() {
    let mut harness = harness();
    harness.get_by_label(EN.sidebar_history);
    press(&mut harness, Modifiers::COMMAND, Key::B);
    assert!(harness.query_by_label(EN.sidebar_history).is_none());
    assert!(harness.state().screen.sidebar_hidden);
    press(&mut harness, Modifiers::COMMAND, Key::B);
    harness.get_by_label(EN.sidebar_history);
}

#[test]
fn the_theme_is_chosen_from_the_palette() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::K);
    type_text(&mut harness, "theme dark");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(
        harness.state().screen.settings.theme,
        apifast_gui::theme::ThemeChoice::Dark
    );
    harness.run();
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);

    press(&mut harness, Modifiers::COMMAND, Key::K);
    type_text(&mut harness, "theme light");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.run();
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
}

#[test]
fn ctrl_comma_opens_the_settings_and_changes_are_marked_for_saving() {
    let mut harness = harness();
    press(&mut harness, Modifiers::COMMAND, Key::Comma);
    assert!(harness.state().screen.settings_window.open);
    harness.get_by_label(EN.settings_appearance);
    harness.state_mut().screen.settings.theme = apifast_gui::theme::ThemeChoice::Dark;
    harness.get_by_label(EN.settings_reset).click();
    harness.run();
    let screen = &mut harness.state_mut().screen;
    assert_eq!(
        screen.settings.theme,
        apifast_gui::theme::ThemeChoice::System
    );
    assert!(screen.take_settings_change());
    assert!(!screen.take_settings_change());
}

#[test]
fn the_scale_follows_ctrl_plus_into_the_settings() {
    let mut harness = harness();
    // As `Ctrl+=` does; egui applies it at the end of the frame.
    harness.ctx.set_zoom_factor(1.5);
    harness.run();
    harness.run();
    let screen = &mut harness.state_mut().screen;
    assert!((screen.settings.ui_scale - 1.5).abs() < f32::EPSILON);
    assert!(screen.take_settings_change());
}
