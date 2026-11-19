//! The sidebar tree with several collections: a root per collection, its
//! menu, and dragging that stays inside a collection.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use std::path::PathBuf;

use apifast_app::{CollectionTree, ItemPath, Placement, RequestPath, TreeItem};
use apifast_domain::{Collection, Method};
use apifast_gui::core::{CollectionEntry, TabKey};
use apifast_gui::organisms::{TreeEvent, TreeView};
use apifast_gui::strings::EN;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    collections: Vec<CollectionEntry>,
    view: TreeView,
    events: Vec<(PathBuf, TreeEvent)>,
}

fn request(path: &str, name: &str) -> TreeItem {
    TreeItem::Request {
        path: RequestPath::new(path),
        name: name.into(),
        method: Method::Get,
        kind: apifast_domain::RequestKind::Http,
        seq: None,
    }
}

fn loaded(root: &str, name: &str, items: Vec<TreeItem>) -> CollectionEntry {
    CollectionEntry::loaded(
        root,
        CollectionTree {
            collection: Collection::new(name),
            items,
        },
    )
}

/// Shop[Login Health], Billing[Charge], and Legacy, which cannot load.
fn harness() -> Harness<'static, State> {
    let collections = vec![
        loaded(
            "/shop",
            "Shop",
            vec![
                request("login.yml", "Login"),
                request("health.yml", "Health"),
            ],
        ),
        loaded("/billing", "Billing", vec![request("charge.yml", "Charge")]),
        CollectionEntry {
            root: "/legacy".into(),
            name: "Legacy".into(),
            expanded: false,
            tree: None,
            error: Some("folder not found".into()),
            needs_key: false,
        },
    ];
    let state = State {
        collections,
        view: TreeView::default(),
        events: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 500.0))
        .build_ui_state(
            |ui, state: &mut State| {
                let selected = TabKey::new("/billing", RequestPath::new("charge.yml"));
                let focus = (Some(&selected), Some(std::path::Path::new("/billing")));
                if let Some(event) = state.view.show(ui, &state.collections, 1, focus, &EN) {
                    state.events.push(event);
                }
            },
            state,
        );
    harness.run();
    harness
}

fn root(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[test]
fn each_collection_is_a_root_and_clicking_it_expands_or_collapses() {
    let mut harness = harness();
    for label in ["Shop", "Login", "Health", "Billing", "Charge", "Legacy"] {
        harness.get_by_label(label);
    }
    harness.get_by_label("Shop").click();
    harness.run();
    harness.get_by_label("Legacy").click();
    harness.run();
    harness.get_by_label("Charge").click();
    harness.run();
    assert_eq!(
        harness.state().events,
        [
            (root("/shop"), TreeEvent::Expand(false)),
            (root("/legacy"), TreeEvent::Expand(true)),
            (
                root("/billing"),
                TreeEvent::RequestSelected(RequestPath::new("charge.yml"))
            ),
        ]
    );
}

/// Opens the menu of the row labelled `row` and picks `entry`.
fn menu(harness: &mut Harness<'_, State>, row: &str, entry: &str) {
    harness.get_by_label(row).click_secondary();
    harness.run();
    harness.get_all_by_label(entry).last().unwrap().click();
    harness.run();
}

#[test]
fn the_root_menu_works_on_its_collection() {
    let mut harness = harness();
    menu(&mut harness, "Shop", EN.environments);
    menu(&mut harness, "Shop", EN.collection_key);
    menu(&mut harness, "Billing", EN.close_collection);
    menu(&mut harness, "Billing", EN.remove_collection);
    // An unloaded collection cannot be closed, only removed.
    harness.get_by_label("Legacy").click_secondary();
    harness.run();
    assert!(harness.query_by_label(EN.close_collection).is_none());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert_eq!(
        harness.state().events,
        [
            (root("/shop"), TreeEvent::Environments),
            (root("/shop"), TreeEvent::CollectionKey),
            (root("/billing"), TreeEvent::Close),
            (root("/billing"), TreeEvent::Remove),
        ]
    );

    // A request created from a root goes to that collection's top.
    menu(&mut harness, "Shop", EN.new_request);
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("Ping".into()));
    harness.run();
    harness.get_by_label(EN.create).click();
    harness.run();
    assert_eq!(
        harness.state().events.last(),
        Some(&(
            root("/shop"),
            TreeEvent::CreateRequest {
                folder: String::new(),
                name: "Ping".into()
            }
        ))
    );
}

fn drag(harness: &mut Harness<'_, State>, from: &str, to: &str, fraction: f32) {
    let start = harness.get_by_label(from).rect().center();
    let target = harness.get_by_label(to).rect();
    let end = egui::pos2(target.center().x, target.top() + target.height() * fraction);
    let send = |harness: &mut Harness<'_, State>, event| {
        harness.input_mut().events.push(event);
        harness.step();
    };
    send(harness, egui::Event::PointerMoved(start));
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    send(harness, button(start, true));
    for t in [0.2, 0.4, 0.6, 0.8, 1.0] {
        send(
            harness,
            egui::Event::PointerMoved(start + (end - start) * t),
        );
    }
    send(harness, button(end, false));
    harness.run();
}

#[test]
fn dragging_stays_inside_a_collection() {
    let mut harness = harness();
    // Onto another collection's request or root: nothing.
    drag(&mut harness, "Health", "Charge", 0.2);
    drag(&mut harness, "Health", "Billing", 0.5);
    // Onto its own root: to the top folder of its collection.
    drag(&mut harness, "Health", "Shop", 0.5);
    assert_eq!(
        harness.state().events,
        [(
            root("/shop"),
            TreeEvent::Move {
                item: ItemPath::Request(RequestPath::new("health.yml")),
                placement: Placement::Inside(String::new()),
            }
        )]
    );
}
