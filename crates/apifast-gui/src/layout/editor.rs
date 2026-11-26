//! The editor's panes: the request and its response, as tiles with a tab
//! each (ADR 0015). Dragging a tab puts its pane beside the other, under it
//! or as a tab of the same place; the line between them resizes them. By
//! default they stand side by side, the same width, as in Bruno.
//!
//! A pane can go out to a window of its own: it is hidden in the tree,
//! which keeps its place, and shows again there when it comes back.

use eframe::egui;
use egui_tiles::{
    Behavior, Container, LinearDir, SimplificationOptions, Tile, TileId, Tiles, Tree, UiResponse,
};

use crate::atoms::{Icon, IconButton};
use crate::theme;

/// A pane of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    /// The request: its tabs (params, headers, body…).
    Request,
    /// Its response.
    Response,
}

/// The size a pane's window opens at the first time, in points.
const WINDOW_SIZE: egui::Vec2 = egui::vec2(640.0, 560.0);

/// Where the editor's panes are.
#[derive(Debug)]
pub struct EditorLayout {
    tree: Tree<Pane>,
    /// The panes in windows of their own, with the size of each window.
    out: Vec<(Pane, egui::Vec2)>,
}

impl Default for EditorLayout {
    fn default() -> Self {
        Self::side_by_side()
    }
}

impl EditorLayout {
    /// The request on the left and the response on the right.
    pub fn side_by_side() -> Self {
        Self::split(LinearDir::Horizontal)
    }

    /// The request above the response.
    pub fn stacked() -> Self {
        Self::split(LinearDir::Vertical)
    }

    fn split(dir: LinearDir) -> Self {
        let mut tiles = Tiles::default();
        let request = tiles.insert_pane(Pane::Request);
        let response = tiles.insert_pane(Pane::Response);
        let request = tiles.insert_tab_tile(vec![request]);
        let response = tiles.insert_tab_tile(vec![response]);
        let root = match dir {
            LinearDir::Horizontal => tiles.insert_horizontal_tile(vec![request, response]),
            LinearDir::Vertical => tiles.insert_vertical_tile(vec![request, response]),
        };
        Self {
            tree: Tree::new(egui::Id::unique("editor-panes"), root, tiles),
            out: Vec::new(),
        }
    }

    /// How the panes are split at the top: side by side (horizontal), one
    /// under the other (vertical), or not at all (tabs of one place).
    pub fn direction(&self) -> Option<LinearDir> {
        match self.tree.tiles.get(self.tree.root()?)? {
            Tile::Container(Container::Linear(linear)) => Some(linear.dir),
            _ => None,
        }
    }

    /// Side by side becomes one under the other, and the other way round,
    /// keeping the sizes; panes that are tabs of one place go side by side.
    pub fn toggle(&mut self) {
        let root = self.tree.root();
        let linear = root.and_then(|root| match self.tree.tiles.get_mut(root) {
            Some(Tile::Container(Container::Linear(linear))) => Some(linear),
            _ => None,
        });
        match linear {
            Some(linear) => {
                linear.dir = match linear.dir {
                    LinearDir::Horizontal => LinearDir::Vertical,
                    LinearDir::Vertical => LinearDir::Horizontal,
                };
            }
            None => *self = Self::side_by_side(),
        }
    }

    /// The layout as data, to keep it in a file: the tree, and the panes
    /// out in windows with their size.
    pub fn to_json(&self) -> serde_json::Value {
        let out: Vec<serde_json::Value> = (self.out.iter())
            .map(|(pane, size)| serde_json::json!({"pane": pane, "size": [size.x, size.y]}))
            .collect();
        serde_json::json!({
            "tree": serde_json::to_value(&self.tree).unwrap_or_default(),
            "out": out,
        })
    }

    /// A layout kept with [`Self::to_json`], if it is one: it reads as a
    /// tree, and has the request and the response once each. A bare tree,
    /// as the first version kept it, has no pane out.
    pub fn from_json(mut value: serde_json::Value) -> Option<Self> {
        let (tree, out) = match value.get_mut("tree") {
            Some(tree) => (tree.take(), value.get("out").cloned()),
            None => (value, None),
        };
        let tree: Tree<Pane> = serde_json::from_value(tree).ok()?;
        let out: Vec<(Pane, egui::Vec2)> = (out
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|entry| {
            let pane: Pane = serde_json::from_value(entry.get("pane")?.clone()).ok()?;
            let size = entry.get("size")?.as_array()?;
            let number = |i: usize| size.get(i)?.as_f64();
            #[expect(clippy::cast_possible_truncation, reason = "a window's size")]
            let size = egui::vec2(number(0)? as f32, number(1)? as f32);
            Some((pane, size))
        })
        .collect();
        let mut layout = Self {
            tree,
            out: Vec::new(),
        };
        let mut panes: Vec<Pane> = layout.panes().into_iter().map(|(_, pane)| pane).collect();
        panes.sort_by_key(|pane| *pane == Pane::Response);
        let root = layout.tree.root()?;
        if panes != [Pane::Request, Pane::Response] || layout.tree.tiles.get(root).is_none() {
            return None;
        }
        for (pane, size) in out {
            layout.take_out(pane);
            layout.resized(pane, size);
        }
        Some(layout)
    }

    /// The panes in windows of their own, with the size of each window.
    pub fn out(&self) -> &[(Pane, egui::Vec2)] {
        &self.out
    }

    /// Takes `pane` out to a window of its own; its place stays, empty.
    pub fn take_out(&mut self, pane: Pane) {
        if self.out.iter().any(|(out, _)| *out == pane) {
            return;
        }
        self.out.push((pane, WINDOW_SIZE));
        self.set_shown(pane, false);
    }

    /// Puts `pane` back in its place.
    pub fn bring_back(&mut self, pane: Pane) {
        self.out.retain(|(out, _)| *out != pane);
        self.set_shown(pane, true);
    }

    /// Remembers the size of `pane`'s window.
    pub fn resized(&mut self, pane: Pane, size: egui::Vec2) {
        if let Some((_, kept)) = self.out.iter_mut().find(|(out, _)| *out == pane) {
            *kept = size.round();
        }
    }

    /// Shows or hides `pane` in the tree; with it, its tab container when
    /// it holds only that pane, so the other pane takes the whole place.
    fn set_shown(&mut self, pane: Pane, shown: bool) {
        let Some((tile, _)) = self.panes().into_iter().find(|(_, p)| *p == pane) else {
            return;
        };
        self.tree.set_visible(tile, shown);
        let tiles = &self.tree.tiles;
        let parent = tiles
            .parent_of(tile)
            .filter(|parent| match tiles.get(*parent) {
                Some(Tile::Container(Container::Tabs(tabs))) => tabs.children.len() == 1,
                _ => false,
            });
        if let Some(parent) = parent {
            self.tree.set_visible(parent, shown);
        }
    }

    /// The pane each tile shows, to find them.
    pub fn panes(&self) -> Vec<(TileId, Pane)> {
        (self.tree.tiles.iter())
            .filter_map(|(id, tile)| match tile {
                Tile::Pane(pane) => Some((*id, *pane)),
                Tile::Container(_) => None,
            })
            .collect()
    }

    /// Draws the panes in `ui`, titled by `titles` (the request's, the
    /// response's), with a button that takes each out to a window, named
    /// `take_out`; `draw` fills each. A pane taken out leaves on the next
    /// frame.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        (titles, take_out): ((&str, &str), &str),
        mut draw: impl FnMut(&mut egui::Ui, Pane),
    ) {
        let mut behavior = Panes {
            titles,
            take_out,
            draw: &mut draw,
            taken: None,
        };
        self.tree.ui(&mut behavior, ui);
        if let Some(pane) = behavior.taken {
            self.take_out(pane);
        }
    }
}

/// How the panes are drawn: their content by the screen, their tabs with
/// apifast's theme.
struct Panes<'a> {
    titles: (&'a str, &'a str),
    /// The name of the button that takes a pane out to a window.
    take_out: &'a str,
    draw: &'a mut dyn FnMut(&mut egui::Ui, Pane),
    /// The pane whose button was pressed.
    taken: Option<Pane>,
}

impl Behavior<Pane> for Panes<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        egui::Frame::NONE
            .inner_margin(theme::space::S)
            .show(ui, |ui| (self.draw)(ui, *pane));
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::Request => self.titles.0,
            Pane::Response => self.titles.1,
        }
        .into()
    }

    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut egui::Ui,
        _tile: TileId,
        tabs: &egui_tiles::Tabs,
        _scroll_offset: &mut f32,
    ) {
        let Some(Tile::Pane(pane)) = tabs.active.and_then(|active| tiles.get(active)) else {
            return;
        };
        let side = ui.spacing().interact_size.y * 0.8;
        let button = IconButton::new(Icon::TakeOut, self.take_out).side(side);
        if ui.add(button).clicked() {
            self.taken = Some(*pane);
        }
    }

    fn tab_bar_height(&self, style: &egui::Style) -> f32 {
        style.spacing.interact_size.y + theme::space::XS
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        theme::space::XS
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            // Every pane keeps its tab: it is how it is dragged.
            all_panes_must_have_tabs: true,
            ..SimplificationOptions::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panes_start_side_by_side_each_with_its_tab() {
        let layout = EditorLayout::default();
        assert_eq!(layout.direction(), Some(LinearDir::Horizontal));
        let mut panes: Vec<Pane> = layout.panes().into_iter().map(|(_, pane)| pane).collect();
        panes.sort_by_key(|pane| *pane == Pane::Response);
        assert_eq!(panes, [Pane::Request, Pane::Response]);
        // Each pane is the only child of a tab container: its tab drags it.
        for (id, _) in layout.panes() {
            let parent = layout.tree.tiles.parent_of(id).unwrap();
            assert!(matches!(
                layout.tree.tiles.get(parent),
                Some(Tile::Container(Container::Tabs(_)))
            ));
        }
    }

    #[test]
    fn toggling_flips_the_split_and_tabs_of_one_place_go_side_by_side() {
        let mut layout = EditorLayout::stacked();
        assert_eq!(layout.direction(), Some(LinearDir::Vertical));
        layout.toggle();
        assert_eq!(layout.direction(), Some(LinearDir::Horizontal));

        // Both panes dragged into one tab container: no split at the top.
        let mut together = EditorLayout {
            tree: Tree::new_tabs(egui::Id::unique("t"), vec![Pane::Request, Pane::Response]),
            out: Vec::new(),
        };
        assert_eq!(together.direction(), None);
        together.toggle();
        assert_eq!(together.direction(), Some(LinearDir::Horizontal));
    }

    #[test]
    fn a_layout_survives_as_data_and_bad_data_is_refused() {
        let mut stacked = EditorLayout::stacked();
        stacked.toggle();
        stacked.toggle();
        let back = EditorLayout::from_json(stacked.to_json()).unwrap();
        assert_eq!(back.direction(), Some(LinearDir::Vertical));
        assert_eq!(back.to_json(), stacked.to_json());

        assert!(EditorLayout::from_json(serde_json::json!({"root": 1})).is_none());
        // A tree that lost the response is not a layout of the editor.
        let alone = EditorLayout {
            tree: Tree::new_tabs(egui::Id::unique("t"), vec![Pane::Request]),
            out: Vec::new(),
        };
        assert!(EditorLayout::from_json(alone.to_json()).is_none());

        // A bare tree, as the first version kept it.
        let bare = serde_json::to_value(&EditorLayout::stacked().tree).unwrap();
        let back = EditorLayout::from_json(bare).unwrap();
        assert_eq!(back.direction(), Some(LinearDir::Vertical));
        assert_eq!(back.out(), []);
    }

    /// Whether `pane` shows in the tree, with what holds it.
    fn shown(layout: &EditorLayout, pane: Pane) -> bool {
        let (tile, _) = layout
            .panes()
            .into_iter()
            .find(|(_, p)| *p == pane)
            .unwrap();
        let parent = layout.tree.tiles.parent_of(tile).unwrap();
        layout.tree.is_visible(tile) && layout.tree.is_visible(parent)
    }

    #[test]
    fn a_pane_goes_out_with_its_window_size_and_comes_back_to_its_place() {
        let mut layout = EditorLayout::default();
        layout.take_out(Pane::Response);
        layout.take_out(Pane::Response);
        assert_eq!(layout.out(), [(Pane::Response, WINDOW_SIZE)], "once");
        assert!(!shown(&layout, Pane::Response));
        assert!(shown(&layout, Pane::Request));
        layout.resized(Pane::Response, egui::vec2(800.4, 600.6));

        // Kept and read back, out and with its size.
        let back = EditorLayout::from_json(layout.to_json()).unwrap();
        assert_eq!(back.out(), [(Pane::Response, egui::vec2(800.0, 601.0))]);
        assert!(!shown(&back, Pane::Response));

        layout.bring_back(Pane::Response);
        assert_eq!(layout.out(), []);
        assert!(shown(&layout, Pane::Response));
        assert_eq!(
            layout.direction(),
            Some(LinearDir::Horizontal),
            "in its place"
        );
    }
}
