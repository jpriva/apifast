//! The vault on the main screen: the banner that asks for the collection's
//! key, the note about a master key in a file, and the key window.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::sync::Arc;

use apifast_app::{CollectionTree, RequestPath};
use apifast_domain::{Collection, HttpRequest, Method, Secret};
use apifast_gui::core::{
    CollectionView, Command, ResponseState, Snapshot, Tab, TabKey, VaultCommand, VaultView,
};
use apifast_gui::screens::MainScreen;
use apifast_gui::strings::EN;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn snapshot(vault: VaultView) -> Snapshot {
    let path = RequestPath::new("health.yml");
    Snapshot {
        collection: Some(CollectionView {
            root: "/c".into(),
            tree: std::sync::Arc::new(CollectionTree {
                collection: Collection::new("C"),
                items: Vec::new(),
            }),
        }),
        tabs: vec![Tab {
            root: "/c".into(),
            path: path.clone(),
            saved: HttpRequest::new("Health", Method::Get, "{{host}}/health"),
            draft: None,
            response: ResponseState::Idle,
            revision: 1,
            context: Arc::default(),
            conflict: false,
        }],
        active: Some(TabKey::new("/c", path)),
        vault,
        ..Snapshot::default()
    }
}

struct State {
    screen: MainScreen,
    snapshot: Snapshot,
    commands: Vec<Command>,
}

fn harness(vault: VaultView) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let commands = state.screen.show(ui, &state.snapshot, &EN);
                state.commands.extend(commands);
            },
            State {
                screen: MainScreen::default(),
                snapshot: snapshot(vault),
                commands: Vec::new(),
            },
        )
}

fn vault_commands(harness: &Harness<'_, State>) -> Vec<VaultCommand> {
    (harness.state().commands.iter())
        .filter_map(|c| match c {
            Command::Vault(v) => Some(v.clone()),
            _ => None,
        })
        .collect()
}

fn locked() -> VaultView {
    VaultView {
        shares: true,
        has_key: false,
        ..VaultView::default()
    }
}

#[test]
fn a_collection_whose_key_is_missing_asks_for_it() {
    let mut harness = harness(locked());
    harness.get_by_label(EN.vault_banner);
    harness
        .get_by_role_and_label(Role::PasswordInput, EN.collection_key)
        .focus();
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("AFK-123".into()));
    harness.run();
    harness.get_by_label(EN.use_key).click();
    harness.run();
    assert_eq!(
        vault_commands(&harness),
        [VaultCommand::ImportKey(Secret::new("AFK-123"))]
    );
}

#[test]
fn with_the_key_there_is_no_banner_and_a_file_key_is_noted() {
    let harness = harness(VaultView {
        shares: true,
        has_key: true,
        master_in_file: true,
        ..VaultView::default()
    });
    assert!(harness.query_by_label(EN.vault_banner).is_none());
    harness.get_by_label(EN.master_in_file);
}

#[test]
fn the_key_window_shows_copies_and_rotates_after_confirming() {
    let mut harness = harness(VaultView {
        shares: true,
        has_key: true,
        ..VaultView::default()
    });
    harness.state_mut().screen.vault_window.open = true;
    harness.run();
    harness.get_by_label(EN.show_key).click();
    harness.run();
    assert_eq!(vault_commands(&harness), [VaultCommand::ShowKey]);

    // The core answers with the key.
    harness.state_mut().snapshot.vault.shareable_key = Some(Secret::new("AFK-ABCDEF-123456"));
    harness.run();
    harness.get_by_label("AFK-ABCDEF-123456");
    harness.get_by_label(EN.copy).click();
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "AFK-ABCDEF-123456"));
    assert!(copied);

    harness.get_by_label(EN.rotate_key).click();
    harness.run();
    harness.get_by_label(EN.rotate_confirm);
    harness
        .get_all_by_label(EN.rotate_key)
        .last()
        .unwrap()
        .click();
    harness.run();
    assert_eq!(
        vault_commands(&harness).last(),
        Some(&VaultCommand::RotateKey)
    );
}

#[test]
fn without_shared_secrets_the_window_explains_where_the_key_comes_from() {
    let mut harness = harness(VaultView::default());
    harness.state_mut().screen.vault_window.open = true;
    harness.run();
    harness.get_by_label(EN.no_collection_key);
    assert!(harness.query_by_label(EN.vault_banner).is_none());
}
