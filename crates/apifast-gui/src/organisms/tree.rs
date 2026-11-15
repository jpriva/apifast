//! The collection tree of the sidebar: only the visible rows are drawn. A
//! context menu creates, renames, moves and deletes; rows can be dragged to
//! reorder them or move them to another folder.

use std::collections::HashSet;

use apifast_app::{CollectionTree, ItemPath, Placement, RequestPath, TreeItem};
use apifast_domain::Method;
use eframe::egui::{
    self, Align2, Id, Rect, RichText, Sense, Shape, Stroke, TextStyle, TextWrapMode, UiBuilder,
    pos2, vec2,
};

use crate::atoms::MethodBadge;
use crate::strings::Strings;
use crate::theme;

/// Width of the name field in the dialogs, in points.
const DIALOG_FIELD_WIDTH: f32 = 280.0;

/// What the user did in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEvent {
    /// The user clicked a request.
    RequestSelected(RequestPath),
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
    expanded: HashSet<String>,
    rows: Vec<Row>,
    /// The tree version the rows were built from.
    built_from: Option<u64>,
    dialog: Option<Dialog>,
}

#[derive(Debug)]
struct Row {
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
    Toggle(String),
    Event(TreeEvent),
    Open(Dialog),
}

impl TreeView {
    /// Draws `tree`. `version` changes whenever `tree` does, so the rows are
    /// rebuilt only then. `selected` is highlighted.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        tree: &CollectionTree,
        version: u64,
        selected: Option<&RequestPath>,
        strings: &Strings,
    ) -> Option<TreeEvent> {
        if self.built_from != Some(version) {
            self.rebuild(tree);
            self.built_from = Some(version);
        }
        let mut action = None;
        ui.horizontal(|ui| {
            if ui.small_button(strings.new_request).clicked() {
                action = Some(RowAction::Open(name_dialog(
                    NameAction::NewRequest(String::new()),
                    "",
                )));
            }
            if ui.small_button(strings.new_folder).clicked() {
                action = Some(RowAction::Open(name_dialog(
                    NameAction::NewFolder(String::new()),
                    "",
                )));
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
            Some(RowAction::Toggle(path)) => {
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
                self.rebuild(tree);
            }
            Some(RowAction::Event(e)) => event = Some(e),
            Some(RowAction::Open(dialog)) => self.dialog = Some(dialog),
            None => {}
        }
        if let Some(dialog) = &mut self.dialog {
            match show_dialog(ui, dialog, strings) {
                DialogOutcome::Open => {}
                DialogOutcome::Closed => self.dialog = None,
                DialogOutcome::Done(e) => {
                    self.dialog = None;
                    event = Some(e);
                }
            }
        }
        // Show what is created inside a closed folder.
        if let Some(
            TreeEvent::CreateRequest { folder, .. }
            | TreeEvent::CreateFolder { parent: folder, .. },
        ) = &event
            && !folder.is_empty()
            && self.expanded.insert(folder.clone())
        {
            self.rebuild(tree);
        }
        event
    }

    fn rebuild(&mut self, tree: &CollectionTree) {
        self.rows.clear();
        push_rows(&mut self.rows, &self.expanded, &tree.items, 0);
    }
}

fn push_rows(rows: &mut Vec<Row>, expanded: &HashSet<String>, items: &[TreeItem], depth: u16) {
    let paths: Vec<ItemPath> = items.iter().map(item_path).collect();
    for (index, item) in items.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| paths[i].clone());
        let next = paths.get(index + 1).cloned();
        let (label, kind) = match item {
            TreeItem::Folder { path, folder, .. } => (
                folder.name.clone(),
                RowKind::Folder {
                    open: expanded.contains(path),
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
            depth,
            item: paths[index].clone(),
            label,
            kind,
            previous,
            next,
        });
        if let (true, TreeItem::Folder { children, .. }) = (open, item) {
            push_rows(rows, expanded, children, depth.saturating_add(1));
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
    selected: Option<&RequestPath>,
    strings: &Strings,
) -> Option<RowAction> {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), row_height),
        Sense::click_and_drag(),
    );
    let is_selected = matches!(&row.item, ItemPath::Request(p) if Some(p) == selected);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::TreeItem, true, is_selected, &row.label)
    });
    if ui.is_rect_visible(rect) {
        paint_row(ui, row, rect, is_selected, response.hovered());
    }

    let mut action = None;
    if response.clicked() {
        action = match (&row.kind, &row.item) {
            (RowKind::Folder { .. }, item) => Some(RowAction::Toggle(item.as_str().to_owned())),
            (RowKind::Request { .. }, ItemPath::Request(path)) => {
                Some(RowAction::Event(TreeEvent::RequestSelected(path.clone())))
            }
            _ => None,
        };
    }

    if !matches!(row.kind, RowKind::Invalid { .. }) {
        response.dnd_set_drag_payload(row.item.clone());
    }
    let pointer_y = ui.ctx().pointer_interact_pos().map(|p| p.y);
    if let (Some(dragged), Some(y)) = (response.dnd_hover_payload::<ItemPath>(), pointer_y)
        && let Some(placement) = drop_placement(row, &dragged, rect, y)
    {
        paint_drop(ui, rect, &placement);
    }
    if let (Some(dragged), Some(y)) = (response.dnd_release_payload::<ItemPath>(), pointer_y)
        && let Some(placement) = drop_placement(row, &dragged, rect, y)
    {
        action = Some(RowAction::Event(TreeEvent::Move {
            item: (*dragged).clone(),
            placement,
        }));
    }

    response.context_menu(|ui| {
        if let Some(a) = context_menu(ui, row, strings) {
            action = Some(a);
            ui.close();
        }
    });
    match &row.kind {
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
        RowKind::Invalid { .. } => palette.danger,
        _ => palette.text,
    };
    match &row.kind {
        RowKind::Folder { open } => {
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
    let galley = egui::WidgetText::from(RichText::new(row.label.as_str()).color(color))
        .into_galley(ui, Some(TextWrapMode::Truncate), width, TextStyle::Body);
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
fn drop_placement(row: &Row, dragged: &ItemPath, rect: Rect, y: f32) -> Option<Placement> {
    if row.item == *dragged
        || (matches!(dragged, ItemPath::Folder(_)) && row.item.is_within(dragged))
    {
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
            action = Some(RowAction::Open(name_dialog(
                NameAction::NewRequest(folder.to_owned()),
                "",
            )));
        }
        if ui.button(strings.new_folder).clicked() {
            action = Some(RowAction::Open(name_dialog(
                NameAction::NewFolder(folder.to_owned()),
                "",
            )));
        }
        ui.separator();
    }
    if matches!(row.kind, RowKind::Folder { .. } | RowKind::Request { .. })
        && ui.button(strings.rename).clicked()
    {
        action = Some(RowAction::Open(name_dialog(
            NameAction::Rename(row.item.clone()),
            &row.label,
        )));
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
                action = Some(RowAction::Event(TreeEvent::Move {
                    item: row.item.clone(),
                    placement,
                }));
            }
        }
        ui.separator();
    }
    if ui.button(strings.delete).clicked() {
        action = Some(RowAction::Open(Dialog::Delete {
            item: row.item.clone(),
            label: row.label.clone(),
        }));
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
