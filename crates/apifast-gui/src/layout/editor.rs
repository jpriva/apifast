//! The editor's panes: the request and its response, as tiles with a tab
//! each (ADR 0015). Dragging a tab puts its pane beside the other, under it
//! or as a tab of the same place; the line between them resizes them. By
//! default they stand side by side, the same width, as in Bruno.

use eframe::egui;
use egui_tiles::{
    Behavior, Container, LinearDir, SimplificationOptions, Tile, TileId, Tiles, Tree, UiResponse,
};

use crate::theme;

/// A pane of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// The request: its tabs (params, headers, body…).
    Request,
    /// Its response.
    Response,
}

/// Where the editor's panes are.
#[derive(Debug)]
pub struct EditorLayout {
    tree: Tree<Pane>,
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
    /// response's); `draw` fills each.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        titles: (&str, &str),
        mut draw: impl FnMut(&mut egui::Ui, Pane),
    ) {
        let mut behavior = Panes {
            titles,
            draw: &mut draw,
        };
        self.tree.ui(&mut behavior, ui);
    }
}

/// How the panes are drawn: their content by the screen, their tabs with
/// apifast's theme.
struct Panes<'a> {
    titles: (&'a str, &'a str),
    draw: &'a mut dyn FnMut(&mut egui::Ui, Pane),
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
        };
        assert_eq!(together.direction(), None);
        together.toggle();
        assert_eq!(together.direction(), Some(LinearDir::Horizontal));
    }
}
