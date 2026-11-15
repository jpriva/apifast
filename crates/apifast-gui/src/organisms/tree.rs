//! The collection tree of the sidebar.

use std::collections::HashSet;

use apifast_app::{CollectionTree, RequestPath, TreeItem};
use apifast_domain::Method;
use eframe::egui::{self, RichText, Sense};

use crate::atoms::MethodBadge;
use crate::strings::Strings;
use crate::theme;

/// What the user did in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEvent {
    /// The user clicked a request.
    RequestSelected(RequestPath),
}

/// The tree's own state: which folders are open, and the visible rows
/// flattened so that only the rows on screen are drawn.
#[derive(Debug, Default)]
pub struct TreeView {
    expanded: HashSet<String>,
    rows: Vec<Row>,
    /// The tree version the rows were built from.
    built_from: Option<u64>,
}

#[derive(Debug)]
struct Row {
    depth: u16,
    kind: RowKind,
}

#[derive(Debug)]
enum RowKind {
    Folder {
        path: String,
        name: String,
        open: bool,
    },
    Request {
        path: RequestPath,
        name: String,
        method: Method,
    },
    /// `name (kind)`.
    Other { label: String },
    /// The path, and why it could not be read.
    Invalid { label: String, message: String },
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
        let mut event = None;
        let mut toggled = None;
        let row_height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, self.rows.len(), |ui, range| {
                for row in &self.rows[range] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(row_height);
                        ui.add_space(f32::from(row.depth) * theme::space::XL);
                        match &row.kind {
                            RowKind::Folder { path, name, open } => {
                                // Painted, not a glyph: egui's fonts have no
                                // arrows.
                                let size = egui::Vec2::splat(ui.spacing().icon_width);
                                let (_, arrow) = ui.allocate_exact_size(size, Sense::click());
                                egui::collapsing_header::paint_default_icon(
                                    ui,
                                    if *open { 1.0 } else { 0.0 },
                                    &arrow,
                                );
                                let label = ui.selectable_label(false, name.as_str());
                                if arrow.clicked() || label.clicked() {
                                    toggled = Some(path.clone());
                                }
                            }
                            RowKind::Request { path, name, method } => {
                                ui.add(MethodBadge::new(method));
                                let is_selected = selected == Some(path);
                                if ui.selectable_label(is_selected, name.as_str()).clicked() {
                                    event = Some(TreeEvent::RequestSelected(path.clone()));
                                }
                            }
                            RowKind::Other { label } => {
                                let muted = theme::palette(ui).text_muted;
                                ui.label(RichText::new(label.as_str()).color(muted))
                                    .on_hover_text(strings.unsupported_kind);
                            }
                            RowKind::Invalid { label, message } => {
                                let danger = theme::palette(ui).danger;
                                ui.label(RichText::new(label.as_str()).color(danger))
                                    .on_hover_text(message.as_str());
                            }
                        }
                    });
                }
            });
        if let Some(path) = toggled {
            if !self.expanded.remove(&path) {
                self.expanded.insert(path);
            }
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
    for item in items {
        let kind = match item {
            TreeItem::Folder {
                path,
                folder,
                children,
            } => {
                let open = expanded.contains(path);
                rows.push(Row {
                    depth,
                    kind: RowKind::Folder {
                        path: path.clone(),
                        name: folder.name.clone(),
                        open,
                    },
                });
                if open {
                    push_rows(rows, expanded, children, depth.saturating_add(1));
                }
                continue;
            }
            TreeItem::Request {
                path, name, method, ..
            } => RowKind::Request {
                path: path.clone(),
                name: name.clone(),
                method: method.clone(),
            },
            TreeItem::Other { name, kind, .. } => RowKind::Other {
                label: format!("{name} ({kind})"),
            },
            TreeItem::Invalid { path, message } => RowKind::Invalid {
                label: path.to_string(),
                message: message.clone(),
            },
        };
        rows.push(Row { depth, kind });
    }
}
