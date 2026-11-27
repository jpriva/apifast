//! The settings window and what its sections change: the editor's Tab
//! and line numbers, the shortcuts, the folders; and asking before closing
//! with unsaved changes.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::Arc;

use apifast_app::RequestPath;
use apifast_domain::{HttpRequest, Method};
use apifast_gui::atoms::{CodeEditor, EditorOptions, TemplateFieldTexts, VariableHints};
use apifast_gui::code::Language;
use apifast_gui::core::{Command, ResponseState, Snapshot, Tab};
use apifast_gui::organisms::{Folders, SettingsSection};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui::{self, Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn tab(path: &str, dirty: bool) -> Tab {
    let saved = HttpRequest::new(path, Method::Get, "http://localhost/");
    Tab {
        root: "/c".into(),
        path: RequestPath::new(path),
        draft: dirty.then(|| HttpRequest::new(path, Method::Post, "http://localhost/")),
        saved,
        response: ResponseState::Idle,
        revision: 1,
        context: Arc::default(),
        conflict: false,
    }
}

struct State {
    screen: MainScreen,
    snapshot: Snapshot,
    commands: Vec<Command>,
}

fn harness(tabs: Vec<Tab>) -> Harness<'static, State> {
    let active = tabs.first().map(Tab::key);
    let mut screen = MainScreen::default();
    screen.settings_window.folders = Folders {
        data: Some("/home/ana/.local/share/apifast".into()),
        config: Some("/home/ana/.config/apifast".into()),
    };
    Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                state.screen.apply_appearance(ui.ctx(), &EN);
                let commands = state.screen.show(ui, &state.snapshot, &EN);
                state.commands.extend(commands);
            },
            State {
                screen,
                snapshot: Snapshot {
                    tabs,
                    active,
                    ..Snapshot::default()
                },
                commands: Vec::new(),
            },
        )
}

fn open_settings(harness: &mut Harness<'_, State>) {
    harness.state_mut().screen.settings_window.open = true;
    harness.run();
}

#[test]
fn each_section_shows_its_settings_and_changes_them() {
    let mut harness = harness(Vec::new());
    open_settings(&mut harness);
    // General first.
    harness.get_by_role_and_label(Role::ComboBox, EN.language);
    harness.get_by_role_and_label(Role::CheckBox, EN.settings_reopen_workspace);

    harness.get_by_label(EN.settings_editor).click();
    harness.run();
    assert_eq!(
        harness.state().screen.settings_window.section,
        SettingsSection::Editor
    );
    harness
        .get_by_role_and_label(Role::CheckBox, EN.settings_line_numbers)
        .click();
    harness.run();
    // On by default: the click turns them off.
    let screen = &mut harness.state_mut().screen;
    assert!(!screen.settings.line_numbers);
    assert!(screen.take_settings_change());

    harness.get_by_label(EN.settings_network).click();
    harness.run();
    assert!(harness.get_all_by_label(EN.settings_timeout).count() > 0);
    harness.get_by_label(EN.proxy_system);

    harness.get_by_label(EN.settings_shortcuts).click();
    harness.run();
    harness.get_by_label(EN.settings_edit_shortcuts).click();
    harness.run();
    assert!(harness.state().screen.shortcuts.open);

    harness.get_by_label(EN.settings_data).click();
    harness.run();
    harness.get_by_label("/home/ana/.config/apifast");
    assert_eq!(harness.get_all_by_label(EN.settings_open_folder).count(), 2);
    harness.get_by_label(EN.settings_log);
}

#[test]
fn the_window_title_bar_has_its_own_close_button() {
    let mut harness = harness(Vec::new());
    open_settings(&mut harness);
    assert!(harness.state().screen.settings_window.open);
    // One ×, apifast's drawn button: egui's, named the same, is gone.
    assert_eq!(harness.get_all_by_label(EN.close_window).count(), 1);
    harness.get_by_label(EN.close_window).click();
    harness.run();
    assert!(!harness.state().screen.settings_window.open);
}

#[test]
fn the_editor_options_apply_once_set() {
    let mut harness = harness(Vec::new());
    harness.state_mut().screen.settings.tab_size = 4;
    harness.state_mut().screen.settings.wrap_lines = false;
    harness.state_mut().screen.settings.line_numbers = false;
    harness.run();
    let options = EditorOptions::of(&harness.ctx);
    assert_eq!(
        options,
        EditorOptions {
            tab_size: 4,
            wrap: false,
            line_numbers: false
        }
    );
}

fn editor(options: EditorOptions) -> Harness<'static, String> {
    let hints = VariableHints::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(600.0, 300.0))
        .build_ui_state(
            move |ui, text: &mut String| {
                options.set(ui.ctx());
                let texts = TemplateFieldTexts {
                    placeholder: "",
                    missing: "missing",
                    generated: "generated",
                    edit: "edit",
                };
                CodeEditor::new(text, Language::Json, &hints, texts)
                    .accessible_name("Body")
                    .show(ui);
            },
            "{\n}".to_owned(),
        );
    harness.run();
    harness.get_by_role(Role::MultilineTextInput).focus();
    harness.run();
    harness
}

fn press_tab(harness: &mut Harness<'_, String>) {
    harness.input_mut().events.push(egui::Event::Key {
        key: Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn tab_inserts_the_spaces_of_the_settings_or_a_tab() {
    let mut harness = editor(EditorOptions {
        tab_size: 4,
        ..EditorOptions::default()
    });
    press_tab(&mut harness);
    assert!(harness.state().contains("    "), "{:?}", harness.state());
    assert!(!harness.state().contains('\t'));
    // Still in the editor: Tab did not move the focus away.
    press_tab(&mut harness);
    assert!(
        harness.state().contains("        "),
        "{:?}",
        harness.state()
    );

    let mut harness = editor(EditorOptions {
        tab_size: 0,
        ..EditorOptions::default()
    });
    press_tab(&mut harness);
    assert!(harness.state().contains('\t'), "{:?}", harness.state());
}

#[test]
fn line_numbers_and_no_wrapping_still_edit() {
    let mut harness = editor(EditorOptions {
        tab_size: 2,
        wrap: false,
        line_numbers: true,
    });
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("x".into()));
    harness.run();
    assert!(harness.state().contains('x'), "{:?}", harness.state());
}

fn ask_to_close(harness: &mut Harness<'_, State>) {
    harness
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    harness.run();
}

fn take(harness: &mut Harness<'_, State>) -> Vec<Command> {
    std::mem::take(&mut harness.state_mut().commands)
}

#[test]
fn closing_with_unsaved_changes_asks_only_when_the_settings_say() {
    let mut harness = harness(vec![tab("a.yml", true), tab("b.yml", false)]);
    ask_to_close(&mut harness);
    assert!(harness.query_by_label(EN.close_save_all).is_none(), "off");

    harness.state_mut().screen.settings.confirm_close = true;
    ask_to_close(&mut harness);
    harness.get_by_label(EN.close_save_all);
    harness.get_by_label("1 tab has unsaved changes.");
    harness.get_by_label(EN.cancel).click();
    harness.run();
    assert!(harness.query_by_label(EN.close_save_all).is_none());

    // Save all: once nothing is unsaved, the window closes.
    ask_to_close(&mut harness);
    harness.get_by_label(EN.close_save_all).click();
    harness.run();
    assert!(
        take(&mut harness)
            .iter()
            .any(|c| matches!(c, Command::SaveAll))
    );
    assert!(harness.query_by_label(EN.close_save_all).is_none());
    harness.state_mut().snapshot.tabs = vec![tab("a.yml", false), tab("b.yml", false)];
    harness.run();
    // Closing now goes through without asking.
    harness.state_mut().snapshot.tabs = vec![tab("a.yml", true)];
    ask_to_close(&mut harness);
    assert!(harness.query_by_label(EN.close_save_all).is_none());
}

#[test]
fn closing_without_saving_does_not_ask_again() {
    let mut harness = harness(vec![tab("a.yml", true)]);
    harness.state_mut().screen.settings.confirm_close = true;
    ask_to_close(&mut harness);
    harness.get_by_label(EN.close_anyway).click();
    harness.run();
    assert!(harness.query_by_label(EN.close_save_all).is_none());
    ask_to_close(&mut harness);
    assert!(harness.query_by_label(EN.close_save_all).is_none());
}
