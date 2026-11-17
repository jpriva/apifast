//! The fields of a proxy, as rows of a two-column grid: protocol, host,
//! port, credentials and the hosts that go direct.

use apifast_domain::{ProxyConfig, ProxyProtocol};
use eframe::egui;

use crate::strings::Strings;

const PROTOCOLS: [ProxyProtocol; 4] = [
    ProxyProtocol::Http,
    ProxyProtocol::Https,
    ProxyProtocol::Socks4,
    ProxyProtocol::Socks5,
];

/// Draws the rows of `config`; the password row only when `password` is
/// given (the application keeps its own apart). Returns whether something
/// changed.
pub fn proxy_fields(
    ui: &mut egui::Ui,
    id_salt: &str,
    config: &mut ProxyConfig,
    password: Option<&mut String>,
    strings: &Strings,
) -> bool {
    let mut changed = false;
    ui.label(strings.proxy_protocol);
    egui::ComboBox::from_id_salt((id_salt, "protocol"))
        .selected_text(config.protocol.as_str())
        .show_ui(ui, |ui| {
            for protocol in PROTOCOLS {
                changed |= ui
                    .selectable_value(&mut config.protocol, protocol, protocol.as_str())
                    .changed();
            }
        })
        .response
        .on_hover_text(strings.proxy_protocol);
    ui.end_row();
    changed |= text(
        ui,
        strings.proxy_host,
        &mut config.hostname,
        "proxy.example.com",
        false,
    );
    changed |= text(ui, strings.proxy_port, &mut config.port, "8080", false);
    ui.label(strings.proxy_auth);
    changed |= ui
        .checkbox(&mut config.auth, "")
        .accessible_name(strings.proxy_auth)
        .changed();
    ui.end_row();
    if config.auth {
        changed |= text(ui, strings.username, &mut config.username, "", false);
        if let Some(password) = password {
            changed |= text(ui, strings.password, password, "", true);
        }
    }
    let hint = "localhost, .internal, 10.0.0.0/8";
    changed |= text(ui, strings.proxy_bypass, &mut config.bypass, hint, false);
    changed
}

/// A labelled text row. Returns whether it changed.
fn text(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, hidden: bool) -> bool {
    ui.label(label);
    let changed = ui
        .add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .password(hidden)
                .desired_width(240.0),
        )
        .accessible_name(label)
        .changed();
    ui.end_row();
    changed
}
