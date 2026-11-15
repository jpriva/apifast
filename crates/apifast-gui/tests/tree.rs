//! The sidebar tree: context menu, dialogs, moving and dragging. Only the
//! events it returns are checked; the core tests cover what they do.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::{CollectionTree, ItemPath, Placement, RequestPath, TreeItem};
use apifast_domain::{Collection, Folder, Method};
use apifast_gui::organisms::{TreeEvent, TreeView};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    tree: CollectionTree,
    view: TreeView,
    events: Vec<TreeEvent>,
}

fn request(path: &str, name: &str) -> TreeItem {
    TreeItem::Request {
        path: RequestPath::new(path),
        name: name.into(),
        method: Method::Get,
        seq: None,
    }
}

/// `Users[List users] Login Health`.
fn harness() -> Harness<'static, State> {
    let tree = CollectionTree {
        collection: Collection::new("API"),
        items: vec![
            TreeItem::Folder {
                path: "users".into(),
                folder: Folder::new("Users"),
                children: vec![request("users/list.yml", "List users")],
            },
            request("login.yml", "Login"),
            request("health.yml", "Health"),
        ],
    };
    let state = State {
        tree,
        view: TreeView::default(),
        events: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 500.0))
        .build_ui_state(
            |ui, state: &mut State| {
                if let Some(event) = state.view.show(ui, &state.tree, 1, None, &EN) {
                    state.events.push(event);
                }
            },
            state,
        );
    harness.run();
    harness
}

fn req(path: &str) -> ItemPath {
    ItemPath::Request(RequestPath::new(path))
}

/// Opens the context menu of the row labelled `row` and picks `entry`.
fn menu(harness: &mut Harness<'_, State>, row: &str, entry: &str) {
    harness.get_by_label(row).click_secondary();
    harness.run();
    // The toolbar has "New request" too: the menu's entry comes last.
    harness.get_all_by_label(entry).last().unwrap().click();
    harness.run();
}

#[test]
fn new_request_in_a_folder_asks_for_its_name() {
    let mut harness = harness();
    menu(&mut harness, "Users", "New request");
    harness.get_by_label("Create");
    // The field has the focus: typing goes straight into it.
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .type_text("Delete user");
    harness.run();
    harness.get_by_label("Create").click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [TreeEvent::CreateRequest {
            folder: "users".into(),
            name: "Delete user".into()
        }]
    );
    // The dialog closed, and the folder opened to show the new request.
    assert!(harness.query_by_label("Create").is_none());
    harness.get_by_label("List users");
}

#[test]
fn the_toolbar_creates_at_the_root_and_enter_confirms() {
    let mut harness = harness();
    harness.get_by_label("New folder").click();
    harness.run();
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .type_text("Admin");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().events,
        [TreeEvent::CreateFolder {
            parent: String::new(),
            name: "Admin".into()
        }]
    );
}

#[test]
fn rename_starts_with_the_current_name() {
    let mut harness = harness();
    menu(&mut harness, "Login", "Rename");
    let field = harness.get_by_role(egui::accesskit::Role::TextInput);
    assert_eq!(field.value().as_deref(), Some("Login"));
    field.type_text(" v2");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().events,
        [TreeEvent::Rename {
            item: req("login.yml"),
            name: "Login v2".into()
        }]
    );
}

#[test]
fn delete_needs_a_confirmation_and_can_be_cancelled() {
    let mut harness = harness();
    menu(&mut harness, "Health", "Delete");
    harness.get_by_label(EN.delete_title);
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(harness.state().events, []);
    assert!(harness.query_by_label(EN.delete_title).is_none());

    menu(&mut harness, "Health", "Delete");
    harness.get_by_label("Delete").click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [TreeEvent::Delete(req("health.yml"))]
    );
}

#[test]
fn move_up_and_down_go_around_the_neighbours() {
    let mut harness = harness();
    menu(&mut harness, "Login", "Move up");
    menu(&mut harness, "Login", "Move down");
    assert_eq!(
        harness.state().events,
        [
            TreeEvent::Move {
                item: req("login.yml"),
                placement: Placement::Before(ItemPath::Folder("users".into())),
            },
            TreeEvent::Move {
                item: req("login.yml"),
                placement: Placement::After(req("health.yml")),
            },
        ]
    );
    // The first item cannot go up: the entry is disabled.
    menu(&mut harness, "Users", "Move up");
    assert_eq!(harness.state().events.len(), 2);
}

/// Drags the row labelled `from` and drops it at `fraction` of the height
/// of the row labelled `to`, with real pointer events.
fn drag(harness: &mut Harness<'_, State>, from: &str, to: &str, fraction: f32) {
    let start = harness.get_by_label(from).rect().center();
    let target = harness.get_by_label(to).rect();
    let end = egui::pos2(target.center().x, target.top() + target.height() * fraction);
    let send = |harness: &mut Harness<'_, State>, event| {
        harness.input_mut().events.push(event);
        harness.step();
    };
    send(harness, egui::Event::PointerMoved(start));
    send(
        harness,
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    );
    for t in [0.2, 0.4, 0.6, 0.8, 1.0] {
        send(
            harness,
            egui::Event::PointerMoved(start + (end - start) * t),
        );
    }
    send(
        harness,
        egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    );
    harness.run();
}

#[test]
fn dragging_moves_before_after_or_into_a_folder() {
    let mut harness = harness();
    drag(&mut harness, "Health", "Login", 0.2);
    drag(&mut harness, "Login", "Users", 0.5);
    drag(&mut harness, "Users", "Health", 0.9);
    assert_eq!(
        harness.state().events,
        [
            TreeEvent::Move {
                item: req("health.yml"),
                placement: Placement::Before(req("login.yml")),
            },
            TreeEvent::Move {
                item: req("login.yml"),
                placement: Placement::Inside("users".into()),
            },
            TreeEvent::Move {
                item: ItemPath::Folder("users".into()),
                placement: Placement::After(req("health.yml")),
            },
        ]
    );
}

#[test]
fn dropping_an_item_on_itself_does_nothing() {
    let mut harness = harness();
    drag(&mut harness, "Login", "Login", 0.9);
    assert_eq!(harness.state().events, []);
}
