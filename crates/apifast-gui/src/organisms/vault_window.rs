//! The collection's key: show it to give it to the team, paste the one the
//! team gave, or rotate it.

use apifast_domain::Secret;
use eframe::egui::{self, RichText};

use crate::core::{VaultCommand, VaultView};
use crate::strings::Strings;
use crate::theme;

/// The window's state.
#[derive(Debug, Default)]
pub struct VaultWindow {
    /// Whether it is open.
    pub open: bool,
    /// The key being pasted.
    pasted: String,
    /// Asking to confirm a rotation.
    confirming: bool,
}

/// A hidden field to paste the collection's key, and its button. Returns
/// the key once the button is pressed.
pub fn paste_key(ui: &mut egui::Ui, text: &mut String, strings: &Strings) -> Option<Secret> {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(text)
                .password(true)
                .hint_text("AFK-...")
                .desired_width(320.0),
        )
        .accessible_name(strings.collection_key);
        let ready = !text.trim().is_empty();
        let used = ui
            .add_enabled(ready, egui::Button::new(strings.use_key))
            .clicked();
        used.then(|| Secret::new(std::mem::take(text)))
    })
    .inner
}

impl VaultWindow {
    /// Draws the window, if open. Returns what it asks of the vault.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        vault: &VaultView,
        strings: &Strings,
    ) -> Vec<VaultCommand> {
        if !self.open {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let mut open = self.open;
        crate::molecules::window(
            ctx,
            egui::Window::new(strings.collection_key)
                .id(egui::Id::unique("vault-window"))
                .default_width(520.0),
            (strings.collection_key, strings.close_window),
            &mut open,
            |ui| {
                let muted = theme::palette(ui).text_muted;
                if !vault.shares {
                    ui.label(RichText::new(strings.no_collection_key).color(muted));
                } else if vault.has_key {
                    commands.extend(self.with_key(ui, vault, strings));
                } else {
                    ui.label(strings.key_needed);
                    if let Some(key) = paste_key(ui, &mut self.pasted, strings) {
                        commands.push(VaultCommand::ImportKey(key));
                    }
                }
            },
        );
        self.open = open;
        commands.extend(self.confirm_rotation(ctx, strings));
        commands
    }

    /// Show, copy and rotate the key this machine has.
    fn with_key(
        &mut self,
        ui: &mut egui::Ui,
        vault: &VaultView,
        strings: &Strings,
    ) -> Vec<VaultCommand> {
        let muted = theme::palette(ui).text_muted;
        let mut commands = Vec::new();
        ui.label(RichText::new(strings.share_key_hint).color(muted));
        ui.add_space(theme::space::S);
        match &vault.shareable_key {
            Some(key) => {
                ui.add(
                    egui::Label::new(RichText::new(key.expose()).monospace().strong())
                        .selectable(true),
                );
                ui.horizontal(|ui| {
                    if ui.button(strings.copy).clicked() {
                        ui.ctx().copy_text(key.expose().to_owned());
                    }
                    if ui.button(strings.hide).clicked() {
                        commands.push(VaultCommand::Hide);
                    }
                });
            }
            None => {
                if ui.button(strings.show_key).clicked() {
                    commands.push(VaultCommand::ShowKey);
                }
            }
        }
        ui.add_space(theme::space::M);
        ui.separator();
        ui.label(RichText::new(strings.rotate_hint).color(muted).small());
        if ui.button(strings.rotate_key).clicked() {
            self.confirming = true;
        }
        commands
    }

    fn confirm_rotation(&mut self, ctx: &egui::Context, strings: &Strings) -> Option<VaultCommand> {
        if !self.confirming {
            return None;
        }
        let mut command = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm-rotation")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(strings.rotate_confirm);
            ui.horizontal(|ui| {
                if ui.button(strings.rotate_key).clicked() {
                    command = Some(VaultCommand::RotateKey);
                    self.confirming = false;
                }
                if ui.button(strings.cancel).clicked() {
                    self.confirming = false;
                }
            });
        });
        if modal.should_close() {
            self.confirming = false;
        }
        command
    }
}
