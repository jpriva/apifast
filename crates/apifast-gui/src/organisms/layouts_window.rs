//! The Layouts window: the layouts saved with a name, to apply, rename or
//! delete; the layout on screen, to save with a name; and the default, to
//! go back to (ADR 0015).

use eframe::egui::{self, RichText};

use crate::strings::Strings;
use crate::theme;

/// What the user did in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutEvent {
    /// Put saved layout number `.0` on screen.
    Apply(usize),
    /// Save the layout on screen as `.0`, replacing one of that name.
    SaveAs(String),
    /// Name saved layout number `.0` `.1`.
    Rename(usize, String),
    /// Delete saved layout number `.0`.
    Delete(usize),
    /// Put the default layout on screen.
    Reset,
}

/// The window's state.
#[derive(Debug, Default)]
pub struct LayoutsWindow {
    /// Whether it is open.
    pub open: bool,
    /// The name typed to save the layout on screen.
    name: String,
    /// The layout being renamed, and the name typed so far.
    renaming: Option<(usize, String)>,
}

impl LayoutsWindow {
    /// Draws the window, if open, with the saved layouts' `names`. Returns
    /// what the user did.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        names: &[String],
        strings: &Strings,
    ) -> Option<LayoutEvent> {
        if !self.open {
            self.renaming = None;
            return None;
        }
        let mut open = self.open;
        let window = egui::Window::new(strings.layouts_title)
            .id(egui::Id::unique("layouts-window"))
            .default_width(380.0)
            .resizable(false);
        let title = (strings.layouts_title, strings.close_window);
        let event = crate::molecules::window(ctx, window, title, &mut open, |ui| {
            let muted = theme::palette(ui).text_muted;
            ui.label(RichText::new(strings.layouts_hint).color(muted).small());
            ui.add_space(theme::space::S);
            let mut event = self.list(ui, names, strings);
            ui.separator();
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text(strings.layout_name_hint)
                        .desired_width(200.0),
                );
                let field = field.accessible_name(strings.layout_name_hint);
                let name = self.name.trim().to_owned();
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let save = ui.add_enabled(!name.is_empty(), egui::Button::new(strings.layout_save));
                if !name.is_empty() && (save.clicked() || enter) {
                    self.name.clear();
                    event = Some(LayoutEvent::SaveAs(name));
                }
            });
            if ui.button(strings.layout_reset).clicked() {
                event = Some(LayoutEvent::Reset);
            }
            event
        });
        self.open = open;
        event.flatten()
    }

    /// One row per saved layout: its name, then Apply, Rename and Delete;
    /// or, while it is renamed, the field for its name.
    fn list(
        &mut self,
        ui: &mut egui::Ui,
        names: &[String],
        strings: &Strings,
    ) -> Option<LayoutEvent> {
        if names.is_empty() {
            let muted = theme::palette(ui).text_muted;
            ui.label(RichText::new(strings.layouts_none).color(muted));
            return None;
        }
        let mut event = None;
        egui::Grid::new("layouts-grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (index, name) in names.iter().enumerate() {
                    match &mut self.renaming {
                        Some((at, typed)) if *at == index => {
                            let field = ui.add(egui::TextEdit::singleline(typed));
                            let field = field.accessible_name(strings.layout_name_hint);
                            if !field.has_focus() && !field.lost_focus() {
                                field.request_focus();
                            }
                            if field.lost_focus() {
                                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                let typed = typed.trim().to_owned();
                                if !cancelled && !typed.is_empty() {
                                    event = Some(LayoutEvent::Rename(index, typed));
                                }
                                self.renaming = None;
                            }
                        }
                        _ => {
                            ui.label(name.as_str());
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button(strings.layout_apply).clicked() {
                            event = Some(LayoutEvent::Apply(index));
                        }
                        if ui.button(strings.rename).clicked() {
                            self.renaming = Some((index, name.clone()));
                        }
                        if ui.button(strings.delete).clicked() {
                            event = Some(LayoutEvent::Delete(index));
                        }
                    });
                    ui.end_row();
                }
            });
        event
    }
}
