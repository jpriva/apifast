//! The tree of the sidebar: one root per collection of the workspace, and
//! under each its folders and requests. Only the visible rows are drawn. A
//! context menu creates, renames, moves and deletes; rows can be dragged to
//! reorder them or move them to another folder of the same collection.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use apifast_app::{ItemPath, Placement, RequestPath, TreeItem};
use apifast_domain::Method;
use eframe::egui::{
    self, Align2, Id, Rect, RichText, Sense, Shape, Stroke, TextStyle, TextWrapMode, UiBuilder,
    pos2, vec2,
};

use crate::atoms::MethodBadge;
use crate::core::{CollectionEntry, TabKey};
use crate::strings::Strings;
use crate::theme;

/// Width of the name field in the dialogs, in points.
const DIALOG_FIELD_WIDTH: f32 = 280.0;

/// What the user did in the tree, about one collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEvent {
    /// Expand or collapse the collection.
    Expand(bool),
    /// Edit the collection's environments.
    Environments,
    /// Show the collection's key.
    CollectionKey,
    /// See and edit the collection's cookies.
    Cookies,
    /// Unload the collection and close its tabs.
    Close,
    /// Remove the collection from the workspace.
    Remove,
    /// Edit the collection's settings, such as its proxy.
    Settings,
    /// Import a new version of the collection's source.
    Reimport,
    /// Export the collection to Postman.
    ExportPostman,
    /// Open the runner on the collection, or on one of its folders.
    Run {
        /// The folder; `None` for the whole collection.
        folder: Option<String>,
        /// How the window names it.
        label: String,
    },
    /// The user clicked a request.
    RequestSelected(RequestPath),
    /// Copy a request as a cURL command.
    CopyAs(RequestPath, apifast_import::CodeLanguage),
    /// Create a request called `name` in `folder` (`""` for the root).
    CreateRequest {
        /// Where.
        folder: String,
        /// Its name.
        name: String,
    },
    /// Create a folder called `name` in `parent` (`""` for the root).
    CreateFolder {
        /// Where.
        parent: String,
        /// Its name.
        name: String,
    },
    /// Rename an item.
    Rename {
        /// What.
        item: ItemPath,
        /// The new name.
        name: String,
    },
    /// Delete an item, confirmed by the user.
    Delete(ItemPath),
    /// Move an item.
    Move {
        /// What.
        item: ItemPath,
        /// Where to.
        placement: Placement,
    },
}

/// The tree's own state: open folders, the visible rows flattened so only
/// those on screen are drawn, and the open dialog.
#[derive(Debug, Default)]
pub struct TreeView {
    /// Open folders: a collection's folder and the folder's path in it.
    expanded: HashSet<(PathBuf, String)>,
    rows: Vec<Row>,
    /// The tree version the rows were built from.
    built_from: Option<u64>,
    /// The open dialog, and the collection it is about.
    dialog: Option<(PathBuf, Dialog)>,
}

/// What is dragged: an item of a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dragged {
    root: PathBuf,
    item: ItemPath,
}

#[derive(Debug)]
struct Row {
    /// The folder of its collection.
    root: PathBuf,
    depth: u16,
    item: ItemPath,
    /// The name shown, built once so frames do not format text.
    label: String,
    kind: RowKind,
    /// The siblings before and after, for "move up" and "move down".
    previous: Option<ItemPath>,
    next: Option<ItemPath>,
}

#[derive(Debug)]
enum RowKind {
    /// A collection of the workspace.
    Collection {
        open: bool,
        loaded: bool,
        /// Why it cannot be loaded.
        error: Option<String>,
        /// It shares secrets and this machine lacks its key.
        needs_key: bool,
    },
    Folder {
        open: bool,
    },
    Request {
        method: Method,
    },
    /// A request of a kind this version cannot open.
    Other,
    /// A file that could not be read, and why.
    Invalid {
        message: String,
    },
}

#[derive(Debug)]
enum Dialog {
    Name {
        action: NameAction,
        text: String,
        focused: bool,
    },
    Delete {
        item: ItemPath,
        label: String,
    },
}

#[derive(Debug)]
enum NameAction {
    NewRequest(String),
    NewFolder(String),
    Rename(ItemPath),
}

/// What a row asks for in this frame.
enum RowAction {
    Toggle(PathBuf, String),
    Event(PathBuf, TreeEvent),
    Open(PathBuf, Dialog),
}

impl TreeView {
    /// Draws the `collections`. `version` changes whenever they do, so the
    /// rows are rebuilt only then. `selected` is highlighted; the toolbar
    /// creates in the `focused` collection. Returns what was done, and the
    /// folder of the collection it is about.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        collections: &[CollectionEntry],
        version: u64,
        (selected, focused): (Option<&TabKey>, Option<&Path>),
        strings: &Strings,
    ) -> Option<(PathBuf, TreeEvent)> {
        if self.built_from != Some(version) {
            self.rebuild(collections);
            self.built_from = Some(version);
        }
        let mut action = None;
        ui.horizontal(|ui| {
            let new_request = ui.add_enabled(
                focused.is_some(),
                egui::Button::new(strings.new_request).small(),
            );
            let new_folder = ui.add_enabled(
                focused.is_some(),
                egui::Button::new(strings.new_folder).small(),
            );
            let dialog = if new_request.clicked() {
                Some(NameAction::NewRequest(String::new()))
            } else {
                new_folder
                    .clicked()
                    .then(|| NameAction::NewFolder(String::new()))
            };
            if let (Some(dialog), Some(root)) = (dialog, focused) {
                action = Some(RowAction::Open(root.to_path_buf(), name_dialog(dialog, "")));
            }
        });
        ui.add_space(theme::space::S);

        let row_height = ui.spacing().interact_size.y;
        let rows = &self.rows;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for row in &rows[range] {
                    if let Some(a) = draw_row(ui, row, row_height, selected, strings) {
                        action = Some(a);
                    }
                }
            });

        let mut event = None;
        match action {
            Some(RowAction::Toggle(root, path)) => {
                let key = (root, path);
                if !self.expanded.remove(&key) {
                    self.expanded.insert(key);
                }
                self.rebuild(collections);
            }
            Some(RowAction::Event(root, e)) => event = Some((root, e)),
            Some(RowAction::Open(root, dialog)) => self.dialog = Some((root, dialog)),
            None => {}
        }
        if let Some((root, dialog)) = &mut self.dialog {
            match show_dialog(ui, dialog, strings) {
                DialogOutcome::Open => {}
                DialogOutcome::Closed => self.dialog = None,
                DialogOutcome::Done(e) => {
                    event = Some((root.clone(), e));
                    self.dialog = None;
                }
            }
        }
        // Show what is created inside a closed folder.
        if let Some((
            root,
            TreeEvent::CreateRequest { folder, .. }
            | TreeEvent::CreateFolder { parent: folder, .. },
        )) = &event
            && !folder.is_empty()
            && self.expanded.insert((root.clone(), folder.clone()))
        {
            self.rebuild(collections);
        }
        event
    }

    fn rebuild(&mut self, collections: &[CollectionEntry]) {
        self.rows.clear();
        for collection in collections {
            self.rows.push(Row {
                root: collection.root.clone(),
                depth: 0,
                item: ItemPath::Folder(String::new()),
                label: collection.name.clone(),
                kind: RowKind::Collection {
                    open: collection.expanded,
                    loaded: collection.tree.is_some(),
                    error: collection.error.clone(),
                    needs_key: collection.needs_key,
                },
                previous: None,
                next: None,
            });
            if let (true, Some(tree)) = (collection.expanded, &collection.tree) {
                let place = (&collection.root, &self.expanded);
                push_rows(&mut self.rows, place, &tree.items, 1);
            }
        }
    }
}

fn push_rows(
    rows: &mut Vec<Row>,
    (root, expanded): (&PathBuf, &HashSet<(PathBuf, String)>),
    items: &[TreeItem],
    depth: u16,
) {
    let paths: Vec<ItemPath> = items.iter().map(item_path).collect();
    for (index, item) in items.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| paths[i].clone());
        let next = paths.get(index + 1).cloned();
        let (label, kind) = match item {
            TreeItem::Folder { path, folder, .. } => (
                folder.name.clone(),
                RowKind::Folder {
                    open: expanded.contains(&(root.clone(), path.clone())),
                },
            ),
            TreeItem::Request { name, method, .. } => (
                name.clone(),
                RowKind::Request {
                    method: method.clone(),
                },
            ),
            TreeItem::Other { name, kind, .. } => (format!("{name} ({kind})"), RowKind::Other),
            TreeItem::Invalid { path, message } => (
                path.to_string(),
                RowKind::Invalid {
                    message: message.clone(),
                },
            ),
        };
        let open = matches!(kind, RowKind::Folder { open: true });
        rows.push(Row {
            root: root.clone(),
            depth,
            item: paths[index].clone(),
            label,
            kind,
            previous,
            next,
        });
        if let (true, TreeItem::Folder { children, .. }) = (open, item) {
            push_rows(rows, (root, expanded), children, depth.saturating_add(1));
        }
    }
}

fn item_path(item: &TreeItem) -> ItemPath {
    match item {
        TreeItem::Folder { path, .. } => ItemPath::Folder(path.clone()),
        TreeItem::Request { path, .. }
        | TreeItem::Other { path, .. }
        | TreeItem::Invalid { path, .. } => ItemPath::Request(path.clone()),
    }
}

/// Draws one row as a single widget: one node for screen readers, one
/// target for clicks, drags and drops.
fn draw_row(
    ui: &mut egui::Ui,
    row: &Row,
    row_height: f32,
    selected: Option<&TabKey>,
    strings: &Strings,
) -> Option<RowAction> {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), row_height),
        Sense::click_and_drag(),
    );
    let is_selected = selected.is_some_and(|key| {
        key.root == row.root && matches!(&row.item, ItemPath::Request(p) if *p == key.path)
    });
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::TreeItem, true, is_selected, &row.label)
    });
    if ui.is_rect_visible(rect) {
        paint_row(ui, row, rect, is_selected, response.hovered());
    }

    let mut action = None;
    if response.clicked() {
        let root = row.root.clone();
        action = match (&row.kind, &row.item) {
            (RowKind::Collection { open, .. }, _) => {
                Some(RowAction::Event(root, TreeEvent::Expand(!open)))
            }
            (RowKind::Folder { .. }, item) => {
                Some(RowAction::Toggle(root, item.as_str().to_owned()))
            }
            (RowKind::Request { .. }, ItemPath::Request(path)) => Some(RowAction::Event(
                root,
                TreeEvent::RequestSelected(path.clone()),
            )),
            _ => None,
        };
    }

    if !matches!(
        row.kind,
        RowKind::Invalid { .. } | RowKind::Collection { .. }
    ) {
        response.dnd_set_drag_payload(Dragged {
            root: row.root.clone(),
            item: row.item.clone(),
        });
    }
    let pointer_y = ui.ctx().pointer_interact_pos().map(|p| p.y);
    if let (Some(dragged), Some(y)) = (response.dnd_hover_payload::<Dragged>(), pointer_y)
        && let Some(placement) = drop_placement(row, &dragged, rect, y)
    {
        paint_drop(ui, rect, &placement);
    }
    if let (Some(dragged), Some(y)) = (response.dnd_release_payload::<Dragged>(), pointer_y)
        && let Some(placement) = drop_placement(row, &dragged, rect, y)
    {
        action = Some(RowAction::Event(
            row.root.clone(),
            TreeEvent::Move {
                item: dragged.item.clone(),
                placement,
            },
        ));
    }

    response.context_menu(|ui| {
        let chosen = match &row.kind {
            RowKind::Collection { loaded, .. } => collection_menu(ui, row, *loaded, strings),
            _ => context_menu(ui, row, strings),
        };
        if let Some(a) = chosen {
            action = Some(a);
            ui.close();
        }
    });
    match &row.kind {
        RowKind::Collection { error, .. } => {
            let path = row.root.display().to_string();
            match error {
                Some(error) => response.on_hover_text(format!("{path}\n{error}")),
                None => response.on_hover_text(path),
            };
        }
        RowKind::Other => {
            response.on_hover_text(strings.unsupported_kind);
        }
        RowKind::Invalid { message } => {
            response.on_hover_text(message.as_str());
        }
        _ => {}
    }
    action
}

fn paint_row(ui: &mut egui::Ui, row: &Row, rect: Rect, selected: bool, hovered: bool) {
    let visuals = ui.visuals();
    let palette = theme::palette(ui);
    if selected {
        ui.painter()
            .rect_filled(rect, theme::radius::SMALL, visuals.selection.bg_fill);
    } else if hovered {
        ui.painter().rect_filled(
            rect,
            theme::radius::SMALL,
            visuals.widgets.hovered.weak_bg_fill,
        );
    }
    let mut x = rect.left() + theme::space::S + f32::from(row.depth) * theme::space::XL;
    let color = match row.kind {
        RowKind::Other => palette.text_muted,
        RowKind::Invalid { .. } | RowKind::Collection { error: Some(_), .. } => palette.danger,
        RowKind::Collection {
            needs_key: true, ..
        } => palette.warning,
        _ => palette.text,
    };
    match &row.kind {
        RowKind::Folder { open } | RowKind::Collection { open, .. } => {
            let size = ui.spacing().icon_width;
            let icon =
                Rect::from_center_size(pos2(x + size / 2.0, rect.center().y), vec2(size, size));
            paint_arrow(ui, icon, *open, palette.text_muted);
            x += size + theme::space::S;
        }
        RowKind::Request { method } => {
            // A child that does not take space in the list: the badge sits
            // inside the row's rectangle.
            let badge = Rect::from_min_max(
                pos2(x, rect.top()),
                pos2(x + theme::METHOD_BADGE_WIDTH, rect.bottom()),
            );
            ui.new_child(UiBuilder::new().max_rect(badge))
                .add(MethodBadge::new(method));
            x += theme::METHOD_BADGE_WIDTH + theme::space::S;
        }
        RowKind::Other | RowKind::Invalid { .. } => {}
    }
    let width = (rect.right() - x - theme::space::S).max(0.0);
    let mut text = RichText::new(row.label.as_str()).color(color);
    if matches!(row.kind, RowKind::Collection { .. }) {
        text = text.strong();
    }
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        width,
        TextStyle::Body,
    );
    let pos = Align2::LEFT_CENTER
        .align_size_within_rect(
            galley.size(),
            Rect::from_x_y_ranges(x..=rect.right(), rect.y_range()),
        )
        .min;
    ui.painter().galley(pos, galley, color);
}

/// A small triangle pointing right (closed) or down (open). Painted: egui's
/// fonts have no arrow glyphs.
fn paint_arrow(ui: &egui::Ui, rect: Rect, open: bool, color: egui::Color32) {
    let r = rect.shrink(rect.width() * 0.25);
    let points = if open {
        vec![r.left_top(), r.right_top(), r.center_bottom()]
    } else {
        vec![r.left_top(), r.right_center(), r.left_bottom()]
    };
    ui.painter()
        .add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Where `dragged` goes if dropped at height `y` of this row, if it can go
/// there at all.
fn drop_placement(row: &Row, dragged: &Dragged, rect: Rect, y: f32) -> Option<Placement> {
    let item = &dragged.item;
    // Only inside its own collection.
    if row.root != dragged.root {
        return None;
    }
    if let RowKind::Collection { .. } = row.kind {
        return Some(Placement::Inside(String::new()));
    }
    if row.item == *item || (matches!(item, ItemPath::Folder(_)) && row.item.is_within(item)) {
        return None;
    }
    let fraction = (y - rect.top()) / rect.height();
    Some(match row.kind {
        RowKind::Folder { open } => {
            if fraction < 0.25 {
                Placement::Before(row.item.clone())
            } else if fraction > 0.75 && !open {
                Placement::After(row.item.clone())
            } else {
                Placement::Inside(row.item.as_str().to_owned())
            }
        }
        _ if fraction < 0.5 => Placement::Before(row.item.clone()),
        _ => Placement::After(row.item.clone()),
    })
}

fn paint_drop(ui: &egui::Ui, rect: Rect, placement: &Placement) {
    let stroke = Stroke::new(2.0, theme::palette(ui).accent);
    let painter = ui.painter();
    match placement {
        Placement::Before(_) => {
            painter.hline(rect.x_range(), rect.top(), stroke);
        }
        Placement::After(_) => {
            painter.hline(rect.x_range(), rect.bottom(), stroke);
        }
        Placement::Inside(_) => {
            painter.rect_stroke(rect, theme::radius::SMALL, stroke, egui::StrokeKind::Inside);
        }
    }
}

fn context_menu(ui: &mut egui::Ui, row: &Row, strings: &Strings) -> Option<RowAction> {
    let mut action = None;
    if let RowKind::Folder { .. } = row.kind {
        let folder = row.item.as_str();
        if ui.button(strings.new_request).clicked() {
            action = Some(RowAction::Open(
                row.root.clone(),
                name_dialog(NameAction::NewRequest(folder.to_owned()), ""),
            ));
        }
        if ui.button(strings.new_folder).clicked() {
            action = Some(RowAction::Open(
                row.root.clone(),
                name_dialog(NameAction::NewFolder(folder.to_owned()), ""),
            ));
        }
        if ui.button(strings.run_menu).clicked() {
            action = Some(RowAction::Event(
                row.root.clone(),
                TreeEvent::Run {
                    folder: Some(folder.to_owned()),
                    label: row.label.clone(),
                },
            ));
        }
        ui.separator();
    }
    if let ItemPath::Request(path) = &row.item {
        ui.menu_button(strings.copy_as, |ui| {
            for language in apifast_import::CodeLanguage::ALL {
                if ui.button(language.label()).clicked() {
                    let event = TreeEvent::CopyAs(path.clone(), language);
                    action = Some(RowAction::Event(row.root.clone(), event));
                }
            }
        });
    }
    if matches!(row.kind, RowKind::Folder { .. } | RowKind::Request { .. })
        && ui.button(strings.rename).clicked()
    {
        action = Some(RowAction::Open(
            row.root.clone(),
            name_dialog(NameAction::Rename(row.item.clone()), &row.label),
        ));
    }
    if !matches!(row.kind, RowKind::Invalid { .. }) {
        let moves = [
            (strings.move_up, row.previous.clone().map(Placement::Before)),
            (strings.move_down, row.next.clone().map(Placement::After)),
        ];
        for (label, placement) in moves {
            let clicked = ui
                .add_enabled(placement.is_some(), egui::Button::new(label))
                .clicked();
            if let (true, Some(placement)) = (clicked, placement) {
                action = Some(RowAction::Event(
                    row.root.clone(),
                    TreeEvent::Move {
                        item: row.item.clone(),
                        placement,
                    },
                ));
            }
        }
        ui.separator();
    }
    if ui.button(strings.delete).clicked() {
        action = Some(RowAction::Open(
            row.root.clone(),
            Dialog::Delete {
                item: row.item.clone(),
                label: row.label.clone(),
            },
        ));
    }
    action
}

/// The menu of a collection's root row.
fn collection_menu(
    ui: &mut egui::Ui,
    row: &Row,
    loaded: bool,
    strings: &Strings,
) -> Option<RowAction> {
    let root = || row.root.clone();
    let mut action = None;
    if loaded {
        if ui.button(strings.new_request).clicked() {
            let dialog = name_dialog(NameAction::NewRequest(String::new()), "");
            action = Some(RowAction::Open(root(), dialog));
        }
        if ui.button(strings.new_folder).clicked() {
            let dialog = name_dialog(NameAction::NewFolder(String::new()), "");
            action = Some(RowAction::Open(root(), dialog));
        }
        ui.separator();
    }
    if ui.button(strings.run_menu).clicked() {
        let event = TreeEvent::Run {
            folder: None,
            label: row.label.clone(),
        };
        action = Some(RowAction::Event(root(), event));
    }
    let events = [
        (strings.environments, TreeEvent::Environments, true),
        (
            strings.collection_settings_menu,
            TreeEvent::Settings,
            loaded,
        ),
        (strings.collection_key, TreeEvent::CollectionKey, true),
        (strings.cookies_menu, TreeEvent::Cookies, loaded),
        (strings.reimport_menu, TreeEvent::Reimport, loaded),
        (
            strings.export_postman_menu,
            TreeEvent::ExportPostman,
            loaded,
        ),
        (strings.close_collection, TreeEvent::Close, loaded),
        (strings.remove_collection, TreeEvent::Remove, true),
    ];
    for (label, event, shown) in events {
        if shown && ui.button(label).clicked() {
            action = Some(RowAction::Event(root(), event));
        }
    }
    if ui.button(strings.copy_folder_path).clicked() {
        ui.ctx().copy_text(row.root.display().to_string());
        ui.close();
    }
    action
}

fn name_dialog(action: NameAction, text: &str) -> Dialog {
    Dialog::Name {
        action,
        text: text.to_owned(),
        focused: false,
    }
}

enum DialogOutcome {
    Open,
    Closed,
    Done(TreeEvent),
}

fn show_dialog(ui: &egui::Ui, dialog: &mut Dialog, strings: &Strings) -> DialogOutcome {
    let modal = egui::Modal::new(Id::unique("tree-dialog")).show(ui.ctx(), |ui| match dialog {
        Dialog::Name {
            action,
            text,
            focused,
        } => {
            let (title, confirm) = match action {
                NameAction::NewRequest(_) => (strings.new_request, strings.create),
                NameAction::NewFolder(_) => (strings.new_folder, strings.create),
                NameAction::Rename(_) => (strings.rename, strings.rename),
            };
            ui.label(RichText::new(title).size(theme::text::HEADING).strong());
            ui.add_space(theme::space::M);
            let field = ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(strings.name_placeholder)
                    .desired_width(DIALOG_FIELD_WIDTH),
            );
            if !*focused {
                field.request_focus();
                *focused = true;
            }
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(theme::space::M);
            let filled = !text.trim().is_empty();
            let (confirmed, cancelled) = buttons(ui, confirm, filled, strings);
            if (confirmed || entered) && filled {
                let name = text.trim().to_owned();
                DialogOutcome::Done(match action {
                    NameAction::NewRequest(folder) => TreeEvent::CreateRequest {
                        folder: folder.clone(),
                        name,
                    },
                    NameAction::NewFolder(parent) => TreeEvent::CreateFolder {
                        parent: parent.clone(),
                        name,
                    },
                    NameAction::Rename(item) => TreeEvent::Rename {
                        item: item.clone(),
                        name,
                    },
                })
            } else if cancelled {
                DialogOutcome::Closed
            } else {
                DialogOutcome::Open
            }
        }
        Dialog::Delete { item, label } => {
            ui.label(
                RichText::new(strings.delete_title)
                    .size(theme::text::HEADING)
                    .strong(),
            );
            ui.add_space(theme::space::M);
            ui.label(RichText::new(label.as_str()).strong());
            ui.label(strings.delete_warning);
            ui.add_space(theme::space::M);
            let (confirmed, cancelled) = buttons(ui, strings.delete, true, strings);
            if confirmed {
                DialogOutcome::Done(TreeEvent::Delete(item.clone()))
            } else if cancelled {
                DialogOutcome::Closed
            } else {
                DialogOutcome::Open
            }
        }
    });
    match modal.inner {
        DialogOutcome::Open if modal.should_close() => DialogOutcome::Closed,
        outcome => outcome,
    }
}

/// The confirm and cancel buttons of a dialog: whether each was clicked.
fn buttons(ui: &mut egui::Ui, confirm: &str, enabled: bool, strings: &Strings) -> (bool, bool) {
    ui.horizontal(|ui| {
        let confirmed = ui
            .add_enabled(enabled, egui::Button::new(confirm))
            .clicked();
        let cancelled = ui.button(strings.cancel).clicked();
        (confirmed, cancelled)
    })
    .inner
}
