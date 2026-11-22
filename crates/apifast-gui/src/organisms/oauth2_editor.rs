//! The fields of an OAuth 2.0 auth, as a grid in the Auth tab: only those
//! the chosen flow uses.

use apifast_domain::{CredentialsPlacement, OAuth2Auth, OAuth2Flow, TokenPlacement};
use eframe::egui::{self, RichText};

use crate::atoms::{TemplateField, TemplateFieldTexts, VariableHints};
use crate::strings::Strings;
use crate::theme;

const FLOWS: [OAuth2Flow; 3] = [
    OAuth2Flow::ClientCredentials,
    OAuth2Flow::Password,
    OAuth2Flow::AuthorizationCode,
];

fn flow_label(flow: OAuth2Flow, strings: &Strings) -> &str {
    match flow {
        OAuth2Flow::ClientCredentials => strings.oauth2_client_credentials,
        OAuth2Flow::Password => strings.oauth2_password,
        OAuth2Flow::AuthorizationCode => strings.oauth2_authorization_code,
        OAuth2Flow::Implicit => strings.oauth2_implicit,
    }
}

/// Draws the fields of `auth` into the current grid (two columns). Returns
/// whether the user changed something.
pub fn oauth2_fields(
    ui: &mut egui::Ui,
    auth: &mut OAuth2Auth,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let texts = TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    };
    let mut changed = flow_menu(ui, auth, strings);
    let (code, password) = (
        auth.flow == OAuth2Flow::AuthorizationCode,
        auth.flow == OAuth2Flow::Password,
    );
    // Label, text, whether it is hidden, whether this flow uses it.
    let fields: [(&str, &mut String, bool, bool); 10] = [
        (
            strings.oauth2_authorization_url,
            &mut auth.authorization_url,
            false,
            code,
        ),
        (
            strings.oauth2_callback_url,
            &mut auth.callback_url,
            false,
            code,
        ),
        (
            strings.oauth2_access_token_url,
            &mut auth.access_token_url,
            false,
            true,
        ),
        (
            strings.oauth2_refresh_token_url,
            &mut auth.refresh_token_url,
            false,
            true,
        ),
        (strings.oauth2_client_id, &mut auth.client_id, false, true),
        (
            strings.oauth2_client_secret,
            &mut auth.client_secret,
            true,
            true,
        ),
        (strings.username, &mut auth.username, false, password),
        (strings.password, &mut auth.password, true, password),
        (strings.oauth2_scope, &mut auth.scope, false, true),
        (strings.oauth2_state, &mut auth.state, false, code),
    ];
    for (label, text, hidden, used) in fields {
        if !used {
            continue;
        }
        ui.label(label);
        changed |= TemplateField::new(text, hints, texts)
            .password(hidden)
            .accessible_name(label)
            .show(ui)
            .changed();
        ui.end_row();
    }
    changed |= client_options(ui, auth, strings);
    changed |= token_fields(ui, auth, strings);
    if !auth.parameters.is_empty() {
        ui.label("");
        let text = strings.count(&strings.oauth2_parameters_kept, auth.parameters.len(), &[]);
        ui.label(RichText::new(text).color(theme::palette(ui).text_muted));
        ui.end_row();
    }
    changed
}

/// The grant menu. Returns whether it changed.
fn flow_menu(ui: &mut egui::Ui, auth: &mut OAuth2Auth, strings: &Strings) -> bool {
    ui.label(strings.oauth2_flow);
    let mut flow = auth.flow;
    egui::ComboBox::from_id_salt("oauth2-flow")
        .selected_text(flow_label(flow, strings))
        .show_ui(ui, |ui| {
            for choice in FLOWS {
                ui.selectable_value(&mut flow, choice, flow_label(choice, strings));
            }
        })
        .response
        .on_hover_text(strings.oauth2_flow);
    ui.end_row();
    let changed = flow != auth.flow;
    auth.flow = flow;
    changed
}

/// How the client credentials travel, and PKCE for the code flow.
fn client_options(ui: &mut egui::Ui, auth: &mut OAuth2Auth, strings: &Strings) -> bool {
    let mut changed = false;
    ui.label(strings.oauth2_send_credentials);
    ui.horizontal(|ui| {
        for (placement, label) in [
            (CredentialsPlacement::Body, strings.oauth2_in_body),
            (
                CredentialsPlacement::BasicAuthHeader,
                strings.oauth2_in_basic,
            ),
        ] {
            changed |= ui
                .radio_value(&mut auth.credentials_placement, placement, label)
                .changed();
        }
    });
    ui.end_row();
    if auth.flow == OAuth2Flow::AuthorizationCode {
        ui.label(strings.oauth2_pkce);
        changed |= ui
            .checkbox(&mut auth.pkce, "")
            .accessible_name(strings.oauth2_pkce)
            .changed();
        ui.end_row();
    }
    changed
}

/// Where the token goes, its id and when it is fetched.
fn token_fields(ui: &mut egui::Ui, auth: &mut OAuth2Auth, strings: &Strings) -> bool {
    let mut changed = false;
    ui.label(strings.oauth2_token_in);
    ui.horizontal(|ui| {
        let header = matches!(auth.token_placement, TokenPlacement::Header(_));
        if ui.radio(header, strings.in_header).clicked() && !header {
            auth.token_placement = TokenPlacement::Header("Bearer".into());
            changed = true;
        }
        if ui.radio(!header, strings.in_query).clicked() && header {
            auth.token_placement = TokenPlacement::Query("access_token".into());
            changed = true;
        }
        let (text, name) = match &mut auth.token_placement {
            TokenPlacement::Header(prefix) => (prefix, strings.oauth2_header_prefix),
            TokenPlacement::Query(key) => (key, strings.oauth2_query_key),
        };
        changed |= ui
            .add(
                egui::TextEdit::singleline(text)
                    .hint_text(name)
                    .desired_width(140.0),
            )
            .accessible_name(name)
            .changed();
    });
    ui.end_row();
    ui.label(strings.oauth2_token_id);
    changed |= ui
        .add(egui::TextEdit::singleline(&mut auth.token_id).desired_width(200.0))
        .accessible_name(strings.oauth2_token_id)
        .changed();
    ui.end_row();
    ui.label(strings.oauth2_token_source);
    egui::ComboBox::from_id_salt("oauth2-token-source")
        .selected_text(auth.token_source.as_str())
        .show_ui(ui, |ui| {
            for source in ["access_token", "id_token"] {
                changed |= ui
                    .selectable_value(&mut auth.token_source, source.to_owned(), source)
                    .changed();
            }
        })
        .response
        .on_hover_text(strings.oauth2_token_source);
    ui.end_row();
    ui.label(strings.oauth2_automatic);
    ui.vertical(|ui| {
        changed |= ui
            .checkbox(&mut auth.auto_fetch, strings.oauth2_auto_fetch)
            .changed();
        changed |= ui
            .checkbox(&mut auth.auto_refresh, strings.oauth2_auto_refresh)
            .changed();
    });
    ui.end_row();
    changed
}
