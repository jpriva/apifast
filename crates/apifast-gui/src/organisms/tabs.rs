//! The tab bar: one tab per open request, a dot on those with unsaved
//! changes, and a confirmation before closing one of them.

use crate::core::TabKey;
use eframe::egui::{self, Id, RichText};

use crate::atoms::CloseButton;
use crate::core::Tab;
use crate::strings::Strings;
use crate::theme;

/// What the user did in the tab bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabEvent {
    /// Show this tab.
    Activate(TabKey),
    /// Close this tab, discarding its unsaved changes (confirmed if it had
    /// any).
    Close(TabKey),
    /// Save this tab's changes, then close it.
    SaveAndClose(TabKey),
}

/// The tab bar and its confirmation dialog.
#[derive(Debug, Default)]
pub struct TabBar {
    /// A dirty tab the user asked to close: its path and name.
    confirming: Option<(TabKey, String)>,
    /// The name of each collection, by folder: shown in a tab's tooltip,
    /// and before its name when another tab has the same one.
    pub names: Vec<(std::path::PathBuf, String)>,
}

impl TabBar {
    /// Draws `tabs` with `active` highlighted.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        tabs: &[Tab],
        active: Option<&TabKey>,
        strings: &Strings,
    ) -> Option<TabEvent> {
        let mut event = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in tabs {
                        let is_active = active.is_some_and(|key| tab.is(key));
                        let clash = tabs.iter().any(|other| {
                            other.root != tab.root && other.request().name == tab.request().name
                        });
                        if let Some(e) = self.tab(ui, tab, (is_active, clash), strings) {
                            event = Some(e);
                        }
                    }
                });
            });
        if let Some(e) = self.confirm(ui, strings) {
            event = Some(e);
        }
        event
    }

    /// Asks to close `tab`: at once if it has no unsaved changes, otherwise
    /// after the user confirms. For the close shortcut.
    pub fn request_close(&mut self, tab: &Tab) -> Option<TabEvent> {
        if tab.is_dirty() {
            self.confirming = Some((tab.key(), tab.request().name.clone()));
            None
        } else {
            Some(TabEvent::Close(tab.key()))
        }
    }

    fn tab(
        &mut self,
        ui: &mut egui::Ui,
        tab: &Tab,
        (active, clash): (bool, bool),
        strings: &Strings,
    ) -> Option<TabEvent> {
        let mut event = None;
        let collection = (self.names.iter())
            .find(|(root, _)| *root == tab.root)
            .map(|(_, name)| name.as_str());
        let name = match (clash, collection) {
            (true, Some(collection)) => format!("{collection} / {}", tab.request().name),
            _ => tab.request().name.clone(),
        };
        let hover = match collection {
            Some(collection) => format!("{collection} / {}", tab.path.as_str()),
            None => tab.path.as_str().to_owned(),
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::XS;
            if tab.is_dirty() {
                unsaved_dot(ui, strings);
            }
            let response = ui.selectable_label(active, name).on_hover_text(hover);
            if response.clicked() {
                event = Some(TabEvent::Activate(tab.key()));
            }
            let close = ui.add(CloseButton::new(strings.close_tab));
            if close.clicked() || response.middle_clicked() {
                event = self.request_close(tab);
            }
        });
        ui.add_space(theme::space::S);
        event
    }

    fn confirm(&mut self, ui: &egui::Ui, strings: &Strings) -> Option<TabEvent> {
        let (path, name) = self.confirming.as_ref()?;
        let modal = egui::Modal::new(Id::unique("close-tab")).show(ui.ctx(), |ui| {
            ui.label(
                RichText::new(strings.unsaved_title)
                    .size(theme::text::HEADING)
                    .strong(),
            );
            ui.add_space(theme::space::M);
            ui.label(RichText::new(name.as_str()).strong());
            ui.label(strings.unsaved_warning);
            ui.add_space(theme::space::M);
            ui.horizontal(|ui| {
                if ui.button(strings.save).clicked() {
                    Some(Some(TabEvent::SaveAndClose(path.clone())))
                } else if ui.button(strings.discard).clicked() {
                    Some(Some(TabEvent::Close(path.clone())))
                } else if ui.button(strings.cancel).clicked() {
                    Some(None)
                } else {
                    None
                }
            })
            .inner
        });
        let outcome = match modal.inner {
            None if modal.should_close() => Some(None),
            outcome => outcome,
        };
        let event = outcome?;
        self.confirming = None;
        event
    }
}

/// A dot for unsaved changes. Painted (egui's fonts have no dot glyph), and
/// named for screen readers.
fn unsaved_dot(ui: &mut egui::Ui, strings: &Strings) {
    let size = ui.spacing().icon_width * 0.5;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, strings.unsaved));
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, theme::palette(ui).accent);
    response.on_hover_text(strings.unsaved);
}
