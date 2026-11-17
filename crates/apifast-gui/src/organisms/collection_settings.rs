//! A collection's settings: its proxy and its client certificates. Saved
//! into `opencollection.yml`, as Bruno keeps them.

use std::path::PathBuf;

use apifast_domain::{ClientCertificate, ClientCertificateKind, CollectionProxy, ProxyMode};
use eframe::egui::{self, RichText};

use crate::organisms::proxy_fields;
use crate::strings::Strings;
use crate::theme;

/// A collection's settings, as edited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectionSettings {
    /// Its proxy.
    pub proxy: CollectionProxy,
    /// Its client certificates.
    pub client_certificates: Vec<ClientCertificate>,
}

/// The window and the settings being edited.
#[derive(Debug, Default)]
pub struct CollectionSettingsWindow {
    /// The collection, its name, and its settings as edited.
    editing: Option<(PathBuf, String, CollectionSettings)>,
}

impl CollectionSettingsWindow {
    /// Opens the window on the collection at `root`, called `name`, whose
    /// proxy is `proxy`.
    pub fn open_for(&mut self, root: PathBuf, name: String, settings: CollectionSettings) {
        self.editing = Some((root, name, settings));
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
    ) -> Option<(PathBuf, CollectionSettings)> {
        let (root, name, settings) = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        egui::Window::new(format!("{} · {name}", strings.collection_settings))
            .id(egui::Id::unique("collection-settings"))
            .open(&mut open)
            .default_width(460.0)
            .collapsible(false)
            .show(ctx, |ui| {
                let proxy = &mut settings.proxy;
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
                certificates(ui, &mut settings.client_certificates, strings);
                ui.add_space(theme::space::M);
                if ui.button(RichText::new(strings.save).strong()).clicked() {
                    saved = Some((root.clone(), settings.clone()));
                }
            });
        if !open || saved.is_some() {
            self.editing = None;
        }
        saved
    }
}

/// The client certificates: a row each, and a button to add a PEM one.
fn certificates(ui: &mut egui::Ui, list: &mut Vec<ClientCertificate>, strings: &Strings) {
    ui.label(RichText::new(strings.client_certificates).strong());
    let mut remove = None;
    for (i, certificate) in list.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut certificate.enabled, "")
                .accessible_name(strings.enabled);
            ui.add(
                egui::TextEdit::singleline(&mut certificate.domain)
                    .hint_text(strings.client_certificate_domain)
                    .desired_width(120.0),
            )
            .accessible_name(strings.client_certificate_domain);
            match &mut certificate.kind {
                ClientCertificateKind::Pem {
                    certificate,
                    private_key,
                } => {
                    for (text, label) in [
                        (certificate, strings.client_certificate_file),
                        (private_key, strings.client_certificate_key),
                    ] {
                        ui.add(
                            egui::TextEdit::singleline(text)
                                .hint_text(label)
                                .desired_width(130.0),
                        )
                        .accessible_name(label);
                    }
                }
                ClientCertificateKind::Pkcs12 { file } => {
                    ui.label(RichText::new(format!("PKCS#12: {file}")).monospace());
                }
            }
            if ui
                .add(crate::atoms::CloseButton::new(strings.remove_row))
                .clicked()
            {
                remove = Some(i);
            }
        });
        let _ = i;
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if ui.button(strings.client_certificate_add).clicked() {
        list.push(ClientCertificate {
            domain: String::new(),
            kind: ClientCertificateKind::Pem {
                certificate: String::new(),
                private_key: String::new(),
            },
            passphrase: String::new(),
            enabled: true,
        });
    }
    let muted = theme::palette(ui).text_muted;
    ui.label(
        RichText::new(strings.client_certificates_hint)
            .color(muted)
            .small(),
    );
}
