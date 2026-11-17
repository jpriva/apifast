//! A collection's settings: for now, its proxy. Saved into
//! `opencollection.yml`, as Bruno keeps it.

use std::path::PathBuf;

use apifast_domain::{CollectionProxy, ProxyMode};
use eframe::egui::{self, RichText};

use crate::organisms::proxy_fields;
use crate::strings::Strings;
use crate::theme;

/// The window and the settings being edited.
#[derive(Debug, Default)]
pub struct CollectionSettingsWindow {
    /// The collection, its name, and its proxy as edited.
    editing: Option<(PathBuf, String, CollectionProxy)>,
}

impl CollectionSettingsWindow {
    /// Opens the window on the collection at `root`, called `name`, whose
    /// proxy is `proxy`.
    pub fn open_for(&mut self, root: PathBuf, name: String, proxy: CollectionProxy) {
        self.editing = Some((root, name, proxy));
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.editing.is_some()
    }

    /// Draws the window. Returns the collection and the proxy to save, once
    /// the user saves.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        strings: &Strings,
    ) -> Option<(PathBuf, CollectionProxy)> {
        let (root, name, proxy) = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        egui::Window::new(format!("{} · {name}", strings.collection_settings))
            .id(egui::Id::unique("collection-settings"))
            .open(&mut open)
            .default_width(460.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(strings.proxy).strong());
                ui.horizontal(|ui| {
                    for (mode, label) in [
                        (ProxyMode::Inherit, strings.proxy_inherit),
                        (ProxyMode::Off, strings.proxy_off),
                        (ProxyMode::Custom, strings.proxy_custom),
                    ] {
                        ui.radio_value(&mut proxy.mode, mode, label);
                    }
                });
                if proxy.mode == ProxyMode::Custom {
                    egui::Grid::new("collection-proxy")
                        .num_columns(2)
                        .spacing([theme::space::L, theme::space::S])
                        .show(ui, |ui| {
                            let config = &mut proxy.config;
                            let mut password = config.password.clone();
                            proxy_fields(
                                ui,
                                "collection-proxy",
                                config,
                                Some(&mut password),
                                strings,
                            );
                            config.password = password;
                        });
                    let muted = theme::palette(ui).text_muted;
                    ui.label(
                        RichText::new(strings.collection_proxy_hint)
                            .color(muted)
                            .small(),
                    );
                }
                ui.add_space(theme::space::M);
                if ui.button(RichText::new(strings.save).strong()).clicked() {
                    saved = Some((root.clone(), proxy.clone()));
                }
            });
        if !open || saved.is_some() {
            self.editing = None;
        }
        saved
    }
}
