//! The request panel: tabs to edit the parameters, headers, body, auth and
//! settings of the request, each showing what it inherits.

use apifast_domain::{
    ApiKeyAuth, ApiKeyPlacement, AuthScheme, BasicAuth, BearerAuth, Body, DigestAuth, FormField,
    Header, HttpRequest, MultipartPart, MultipartValue, NtlmAuth, Param, ParamKind, Resolved,
    Setting,
};
use eframe::egui::{self, RichText};

use crate::actions::Action;
use crate::atoms::{CodeEditor, TemplateField, TemplateFieldTexts, VariableHints};
use crate::code::{self, HIGHLIGHT_LIMIT, Language};
use crate::core::{TabContext, TokenState};
use crate::organisms::KvTable;
use crate::strings::Strings;
use crate::theme;
use crate::vocabulary::HEADER_NAMES;

/// Rows the body editor shows before scrolling.
const BODY_ROWS: usize = 12;

/// Which part of the request is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RequestTab {
    /// Query and path parameters.
    #[default]
    Params,
    /// Headers.
    Headers,
    /// Body.
    Body,
    /// Authentication.
    Auth,
    /// Assertions and captures.
    Tests,
    /// Send settings.
    Settings,
}

/// What the user changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestChange {
    /// The parameters: the URL's query must follow them.
    Params,
    /// Anything else.
    Other,
}

/// The request panel and its open tab.
#[derive(Debug, Default)]
pub struct RequestPanel {
    /// The open tab.
    pub tab: RequestTab,
    /// The shortcuts that format the body while it is edited; the default
    /// (`Alt+Shift+F`) when `None`.
    pub format_shortcuts: Option<Vec<egui::KeyboardShortcut>>,
    /// Format the body on the next frame, as if its button was pressed.
    pub format_now: bool,
    /// Whether an OAuth 2.0 token is being obtained for this request.
    pub token_busy: bool,
    /// What the user asked of the OAuth 2.0 token, once, since the last
    /// time it was taken.
    pub token_action: Option<TokenAction>,
    /// The collection's folder and the file dialog of multipart and file
    /// bodies.
    pub files: super::body_parts::BodyFiles,
}

/// What the user can ask of a request's OAuth 2.0 token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAction {
    /// Get a new one now.
    Get,
    /// Forget the stored one.
    Forget,
}

impl RequestPanel {
    /// Draws the panel over `request`, which it edits in place. `context`
    /// is what the request inherits.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        request: &mut HttpRequest,
        context: &TabContext,
        hints: &VariableHints,
        strings: &Strings,
    ) -> Option<RequestChange> {
        ui.horizontal(|ui| {
            let params = request.params.iter().filter(|p| p.enabled).count();
            let headers = request.headers.iter().filter(|h| h.enabled).count();
            let tests = request.assertions.iter().filter(|a| a.enabled).count()
                + request.captures.iter().filter(|c| c.enabled).count();
            for (tab, label) in [
                (RequestTab::Params, counted(strings.params, params)),
                (RequestTab::Headers, counted(strings.headers, headers)),
                (RequestTab::Body, strings.body.to_owned()),
                (RequestTab::Auth, strings.auth.to_owned()),
                (RequestTab::Tests, counted(strings.tests, tests)),
                (RequestTab::Settings, strings.settings.to_owned()),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("request-panel")
            .auto_shrink([false, true])
            .show(ui, |ui| match self.tab {
                RequestTab::Params => params(ui, request, hints, strings),
                RequestTab::Headers => {
                    headers(ui, request, context, hints, strings).then_some(RequestChange::Other)
                }
                RequestTab::Body => {
                    let shortcuts = (self.format_shortcuts.clone())
                        .unwrap_or_else(|| Action::FormatBody.defaults());
                    let now = std::mem::take(&mut self.format_now);
                    let files = &mut self.files;
                    body(
                        ui,
                        &mut request.body,
                        (&shortcuts, now),
                        files,
                        hints,
                        strings,
                    )
                    .then_some(RequestChange::Other)
                }
                RequestTab::Auth => {
                    let changed = auth(ui, &mut request.auth, context, hints, strings);
                    if context.token != TokenState::NotUsed {
                        ui.add_space(theme::space::M);
                        let busy = self.token_busy;
                        self.token_action = token_row(ui, &context.token, busy, strings)
                            .or(self.token_action.take());
                    }
                    changed.then_some(RequestChange::Other)
                }
                RequestTab::Tests => crate::organisms::tests_panel(ui, request, hints, strings)
                    .then_some(RequestChange::Other),
                RequestTab::Settings => {
                    settings(ui, request, context, strings).then_some(RequestChange::Other)
                }
            })
            .inner
    }
}

/// The stored OAuth 2.0 token: when it expires, and buttons to get a new
/// one or forget it.
fn token_row(
    ui: &mut egui::Ui,
    token: &TokenState,
    busy: bool,
    strings: &Strings,
) -> Option<TokenAction> {
    let palette = theme::palette(ui);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let (text, color) = match token {
        TokenState::NotUsed => return None,
        TokenState::Missing => (strings.token_none.to_owned(), palette.text_muted),
        TokenState::Unreadable(e) => (e.clone(), palette.danger),
        TokenState::Stored(info) => match info.expires_at_ms {
            Some(at) if at <= now => (strings.token_expired.to_owned(), palette.warning),
            Some(at) => (
                strings
                    .token_expires_in
                    .replace("{time}", &until(at.saturating_sub(now))),
                palette.success,
            ),
            None => (strings.token_no_expiry.to_owned(), palette.success),
        },
    };
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(strings.token_label).strong());
        ui.label(RichText::new(text).color(color));
        if busy {
            ui.spinner();
        } else if ui.button(strings.token_get).clicked() {
            action = Some(TokenAction::Get);
        }
        if matches!(token, TokenState::Stored(_)) && ui.button(strings.token_forget).clicked() {
            action = Some(TokenAction::Forget);
        }
    });
    action
}

/// A time span for people: `45 s`, `12 min`, `3 h`, `2 d`.
fn until(ms: i64) -> String {
    let s = ms / 1000;
    match s {
        ..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86_400),
    }
}

fn counted(label: &str, count: usize) -> String {
    if count == 0 {
        label.to_owned()
    } else {
        format!("{label} ({count})")
    }
}

fn muted(ui: &egui::Ui, text: &str) -> RichText {
    RichText::new(text).color(theme::palette(ui).text_muted)
}

fn texts(strings: &Strings) -> TemplateFieldTexts<'_> {
    TemplateFieldTexts {
        placeholder: strings.value,
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    }
}

fn params(
    ui: &mut egui::Ui,
    request: &mut HttpRequest,
    hints: &VariableHints,
    strings: &Strings,
) -> Option<RequestChange> {
    ui.label(RichText::new(strings.query_params).strong());
    let mut changed = KvTable::new(
        "query-params",
        &mut request.params,
        |p| p.kind == ParamKind::Query,
        |name, value| Param {
            name,
            value,
            kind: ParamKind::Query,
            enabled: true,
        },
        hints,
        strings,
    )
    .show(ui);
    ui.add_space(theme::space::L);
    ui.label(RichText::new(strings.path_params).strong());
    if request.params.iter().any(|p| p.kind == ParamKind::Path) {
        changed |= KvTable::new(
            "path-params",
            &mut request.params,
            |p| p.kind == ParamKind::Path,
            |name, value| Param {
                name,
                value,
                kind: ParamKind::Path,
                enabled: true,
            },
            hints,
            strings,
        )
        .fixed_names()
        .show(ui);
    } else {
        ui.label(muted(ui, strings.path_params_hint));
    }
    changed.then_some(RequestChange::Params)
}

fn headers(
    ui: &mut egui::Ui,
    request: &mut HttpRequest,
    context: &TabContext,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let changed = KvTable::new(
        "headers",
        &mut request.headers,
        |_| true,
        Header::new,
        hints,
        strings,
    )
    .name_suggestions(HEADER_NAMES)
    .show(ui);
    if !context.inherited_headers.is_empty() {
        ui.add_space(theme::space::L);
        ui.label(RichText::new(strings.inherited).strong());
        egui::Grid::new("inherited-headers")
            .num_columns(3)
            .show(ui, |ui| {
                for Resolved { value, origin } in &context.inherited_headers {
                    let replaced = request
                        .headers
                        .iter()
                        .any(|own| own.enabled && own.name.eq_ignore_ascii_case(&value.name));
                    let mut name = muted(ui, &value.name).monospace();
                    let mut text = muted(ui, &value.value).monospace();
                    if replaced {
                        name = name.strikethrough();
                        text = text.strikethrough();
                    }
                    let response = ui.label(name);
                    if replaced {
                        response.on_hover_text(strings.overridden);
                    }
                    ui.label(text);
                    ui.label(muted(ui, &strings.origin(origin, &context.folders)));
                    ui.end_row();
                }
            });
    }
    changed
}

/// The kinds of body the selector offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    None,
    Json,
    Text,
    Xml,
    Sparql,
    Form,
    Multipart,
    File,
}

impl BodyKind {
    /// Kinds that can be picked. SPARQL is shown when a file has it.
    const EDITABLE: [Self; 7] = [
        Self::None,
        Self::Json,
        Self::Text,
        Self::Xml,
        Self::Form,
        Self::Multipart,
        Self::File,
    ];

    fn of(body: &Body) -> Self {
        match body {
            Body::Json(_) => Self::Json,
            Body::Text(_) => Self::Text,
            Body::Xml(_) => Self::Xml,
            Body::Sparql(_) => Self::Sparql,
            Body::FormUrlEncoded(_) => Self::Form,
            Body::Multipart(_) => Self::Multipart,
            Body::File { .. } => Self::File,
            // `None`, and kinds added after this version.
            _ => Self::None,
        }
    }

    fn label(self, strings: &Strings) -> &str {
        match self {
            Self::None => strings.body_none,
            Self::Json => strings.body_json,
            Self::Text => strings.body_text,
            Self::Xml => strings.body_xml,
            Self::Sparql => strings.body_sparql,
            Self::Form => strings.body_form,
            Self::Multipart => strings.body_multipart,
            Self::File => strings.body_file,
        }
    }

    /// `body` as this kind, keeping its text when both are text, and its
    /// fields between a form and a multipart body (text parts only).
    fn convert(self, body: &mut Body) {
        let old = std::mem::take(body);
        let fields: Vec<FormField> = match &old {
            Body::FormUrlEncoded(fields) => fields.clone(),
            Body::Multipart(parts) => (parts.iter())
                .filter_map(|p| match &p.value {
                    MultipartValue::Text(value) => Some(FormField {
                        name: p.name.clone(),
                        value: value.clone(),
                        enabled: p.enabled,
                    }),
                    MultipartValue::Files(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let text = match old {
            Body::Json(t) | Body::Text(t) | Body::Xml(t) | Body::Sparql(t) => t,
            _ => String::new(),
        };
        *body = match self {
            Self::None => Body::None,
            Self::Json => Body::Json(text),
            Self::Text => Body::Text(text),
            Self::Xml => Body::Xml(text),
            Self::Sparql => Body::Sparql(text),
            Self::Form => Body::FormUrlEncoded(fields),
            Self::Multipart => Body::Multipart(
                fields
                    .into_iter()
                    .map(|f| MultipartPart {
                        name: f.name,
                        value: MultipartValue::Text(f.value),
                        content_type: None,
                        enabled: f.enabled,
                    })
                    .collect(),
            ),
            Self::File => Body::File {
                path: String::new(),
                content_type: None,
            },
        };
    }
}

fn body(
    ui: &mut egui::Ui,
    body: &mut Body,
    (shortcuts, now): (&[egui::KeyboardShortcut], bool),
    files: &mut super::body_parts::BodyFiles,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let current = BodyKind::of(body);
    let editor = ui.make_persistent_id("body-text");
    let formats = Language::of(body).is_some_and(Language::formats);
    let mut chosen = None;
    let mut format = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("body-type")
            .selected_text(current.label(strings))
            .show_ui(ui, |ui| {
                let mut kinds = BodyKind::EDITABLE.to_vec();
                if !kinds.contains(&current) {
                    kinds.push(current);
                }
                for kind in kinds {
                    if ui
                        .selectable_label(kind == current, kind.label(strings))
                        .clicked()
                    {
                        chosen = Some(kind);
                    }
                }
            })
            .response
            .on_hover_text(strings.body_type);
        if formats {
            format = ui
                .button(strings.format)
                .on_hover_text(strings.format_hint)
                .clicked();
        }
    });
    format |= formats && now;
    // The shortcut (Alt+Shift+F) while the editor has the focus. Taken
    // before the editor runs, so the key does not also type a letter.
    if formats && ui.memory(|m| m.has_focus(editor)) {
        format |= ui.input_mut(|i| {
            let pressed = shortcuts.iter().any(|s| i.consume_shortcut(s));
            if pressed {
                i.events.retain(|e| !matches!(e, egui::Event::Text(_)));
            }
            pressed
        });
    }
    let mut changed = false;
    if let Some(kind) = chosen.filter(|kind| *kind != current) {
        kind.convert(body);
        changed = true;
    }
    ui.add_space(theme::space::S);
    let language = Language::of(body);
    match body {
        Body::None => {}
        Body::Json(text) | Body::Text(text) | Body::Xml(text) | Body::Sparql(text) => {
            let language = language.unwrap_or(Language::Text);
            changed |= code_body(ui, text, language, editor, format, hints, strings);
        }
        Body::FormUrlEncoded(fields) => {
            changed |= KvTable::new(
                "form-fields",
                fields,
                |_| true,
                |name, value| FormField {
                    name,
                    value,
                    enabled: true,
                },
                hints,
                strings,
            )
            .show(ui);
        }
        Body::Multipart(parts) => {
            changed |= super::body_parts::multipart(ui, parts, files, hints, strings);
        }
        Body::File { path, content_type } => {
            changed |= super::body_parts::file(ui, (path, content_type), files, strings);
        }
        _ => {
            ui.label(muted(ui, strings.body_not_editable));
        }
    }
    changed
}

/// The editor of a text body. `format` asks to format it first. Returns
/// whether the text changed.
fn code_body(
    ui: &mut egui::Ui,
    text: &mut String,
    language: Language,
    editor: egui::Id,
    format: bool,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    // The line where formatting failed, shown until the text changes.
    let failed = editor.with("format-failed");
    let mut changed = false;
    if format {
        match code::format(text, language) {
            Ok(formatted) => {
                if formatted != *text {
                    *text = formatted;
                    changed = true;
                }
                ui.data_mut(|d| d.remove::<usize>(failed));
            }
            Err(error) => {
                let line = code::line_of(text, error.offset);
                ui.data_mut(|d| d.insert_temp(failed, line));
            }
        }
    }
    if let Some(line) = ui.data(|d| d.get_temp::<usize>(failed)) {
        let message = format!("{} {line}.", strings.format_failed);
        ui.label(RichText::new(message).color(theme::palette(ui).danger));
    }
    if text.len() > HIGHLIGHT_LIMIT {
        ui.label(muted(ui, strings.highlight_off));
    }
    let edited = CodeEditor::new(text, language, hints, texts(strings))
        .id(editor)
        .desired_rows(BODY_ROWS)
        .accessible_name(strings.body)
        .show(ui)
        .changed();
    if edited {
        ui.data_mut(|d| d.remove::<usize>(failed));
    }
    changed || edited
}

/// The kinds of auth the selector offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    Inherit,
    None,
    Basic,
    Bearer,
    ApiKey,
    Digest,
    Ntlm,
    AwsV4,
    OAuth2,
    Unsupported,
}

impl AuthKind {
    const CHOICES: [Self; 9] = [
        Self::Inherit,
        Self::None,
        Self::Basic,
        Self::Bearer,
        Self::ApiKey,
        Self::Digest,
        Self::Ntlm,
        Self::AwsV4,
        Self::OAuth2,
    ];

    fn of(auth: &Setting<AuthScheme>) -> Self {
        match auth {
            Setting::Inherit => Self::Inherit,
            Setting::Set(AuthScheme::None) => Self::None,
            Setting::Set(AuthScheme::Basic(_)) => Self::Basic,
            Setting::Set(AuthScheme::Bearer(_)) => Self::Bearer,
            Setting::Set(AuthScheme::ApiKey(_)) => Self::ApiKey,
            Setting::Set(AuthScheme::Digest(_)) => Self::Digest,
            Setting::Set(AuthScheme::Ntlm(_)) => Self::Ntlm,
            Setting::Set(AuthScheme::AwsV4(_)) => Self::AwsV4,
            Setting::Set(AuthScheme::OAuth2(_)) => Self::OAuth2,
            // `Unsupported`, and schemes added after this version.
            Setting::Set(_) => Self::Unsupported,
        }
    }

    fn label(self, strings: &Strings) -> &str {
        match self {
            Self::Inherit => strings.auth_inherit,
            Self::None => strings.auth_none,
            Self::Basic => strings.auth_basic,
            Self::Bearer => strings.auth_bearer,
            Self::ApiKey => strings.auth_api_key,
            Self::Digest => strings.auth_digest,
            Self::Ntlm => strings.auth_ntlm,
            Self::AwsV4 => strings.auth_aws_v4,
            Self::OAuth2 => strings.auth_oauth2,
            Self::Unsupported => strings.auth_unsupported,
        }
    }

    fn empty(self) -> Setting<AuthScheme> {
        match self {
            Self::Inherit => Setting::Inherit,
            Self::None | Self::Unsupported => Setting::Set(AuthScheme::None),
            Self::Basic => Setting::Set(AuthScheme::Basic(BasicAuth {
                username: String::new(),
                password: String::new(),
            })),
            Self::Bearer => Setting::Set(AuthScheme::Bearer(BearerAuth {
                token: String::new(),
            })),
            Self::ApiKey => Setting::Set(AuthScheme::ApiKey(ApiKeyAuth {
                key: String::new(),
                value: String::new(),
                placement: ApiKeyPlacement::Header,
            })),
            Self::Digest => Setting::Set(AuthScheme::Digest(DigestAuth::default())),
            Self::Ntlm => Setting::Set(AuthScheme::Ntlm(NtlmAuth::default())),
            Self::AwsV4 => Setting::Set(AuthScheme::AwsV4(Box::default())),
            Self::OAuth2 => Setting::Set(AuthScheme::OAuth2(Box::default())),
        }
    }
}

fn scheme_label<'a>(scheme: &AuthScheme, strings: &'a Strings) -> &'a str {
    AuthKind::of(&Setting::Set(scheme.clone())).label(strings)
}

/// The menu of auth types. Choosing another starts it empty; returns
/// whether it did.
fn auth_kind_menu(ui: &mut egui::Ui, auth: &mut Setting<AuthScheme>, strings: &Strings) -> bool {
    let current = AuthKind::of(auth);
    let mut chosen = None;
    egui::ComboBox::from_id_salt("auth-type")
        .selected_text(current.label(strings))
        .show_ui(ui, |ui| {
            for kind in AuthKind::CHOICES {
                if ui
                    .selectable_label(kind == current, kind.label(strings))
                    .clicked()
                {
                    chosen = Some(kind);
                }
            }
        })
        .response
        .on_hover_text(strings.auth_type);
    match chosen.filter(|kind| *kind != current) {
        Some(kind) => {
            *auth = kind.empty();
            true
        }
        None => false,
    }
}

fn auth(
    ui: &mut egui::Ui,
    auth: &mut Setting<AuthScheme>,
    context: &TabContext,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let mut changed = auth_kind_menu(ui, auth, strings);
    ui.add_space(theme::space::S);
    let texts = texts(strings);
    let field = |ui: &mut egui::Ui, label: &str, text: &mut String, hidden: bool| {
        ui.label(label);
        let response = TemplateField::new(text, hints, texts)
            .password(hidden)
            .show(ui);
        ui.end_row();
        response.changed()
    };
    egui::Grid::new("auth-fields")
        .num_columns(2)
        .show(ui, |ui| match auth {
            Setting::Inherit => {
                let inherited = &context.inherited_auth;
                ui.label(muted(
                    ui,
                    &format!(
                        "{} {}: {}",
                        strings.inherited_from,
                        strings.origin(&inherited.origin, &context.folders),
                        scheme_label(&inherited.value, strings)
                    ),
                ));
                ui.end_row();
            }
            Setting::Set(AuthScheme::Basic(basic)) => {
                changed |= field(ui, strings.username, &mut basic.username, false);
                changed |= field(ui, strings.password, &mut basic.password, true);
            }
            Setting::Set(AuthScheme::Bearer(bearer)) => {
                changed |= field(ui, strings.token, &mut bearer.token, true);
            }
            Setting::Set(AuthScheme::ApiKey(key)) => {
                changed |= field(ui, strings.key, &mut key.key, false);
                changed |= field(ui, strings.value, &mut key.value, true);
                ui.label(strings.add_to);
                ui.horizontal(|ui| {
                    for (placement, label) in [
                        (ApiKeyPlacement::Header, strings.in_header),
                        (ApiKeyPlacement::Query, strings.in_query),
                    ] {
                        changed |= ui
                            .radio_value(&mut key.placement, placement, label)
                            .changed();
                    }
                });
                ui.end_row();
            }
            Setting::Set(AuthScheme::Digest(digest)) => {
                changed |= field(ui, strings.username, &mut digest.username, false);
                changed |= field(ui, strings.password, &mut digest.password, true);
            }
            Setting::Set(AuthScheme::Ntlm(ntlm)) => {
                changed |= field(ui, strings.username, &mut ntlm.username, false);
                changed |= field(ui, strings.password, &mut ntlm.password, true);
                changed |= field(ui, strings.ntlm_domain, &mut ntlm.domain, false);
            }
            Setting::Set(AuthScheme::OAuth2(oauth)) => {
                changed |= crate::organisms::oauth2_fields(ui, oauth, hints, strings);
            }
            Setting::Set(AuthScheme::AwsV4(aws)) => {
                for (label, text, hidden) in [
                    (strings.aws_access_key_id, &mut aws.access_key_id, false),
                    (
                        strings.aws_secret_access_key,
                        &mut aws.secret_access_key,
                        true,
                    ),
                    (strings.aws_session_token, &mut aws.session_token, true),
                    (strings.aws_service, &mut aws.service, false),
                    (strings.aws_region, &mut aws.region, false),
                ] {
                    changed |= field(ui, label, text, hidden);
                }
            }
            Setting::Set(AuthScheme::None) => {}
            Setting::Set(_) => {
                ui.label(muted(ui, strings.auth_unsupported));
                ui.end_row();
            }
        });
    changed
}

fn settings(
    ui: &mut egui::Ui,
    request: &mut HttpRequest,
    context: &TabContext,
    strings: &Strings,
) -> bool {
    let inherited = &context.inherited_settings;
    let settings = &mut request.settings;
    let yes_no = |on: &bool| if *on { strings.yes } else { strings.no }.to_owned();
    let mut changed = false;
    egui::Grid::new("settings")
        .num_columns(3)
        .spacing([theme::space::L, theme::space::S])
        .show(ui, |ui| {
            changed |= setting_row(
                ui,
                strings.timeout,
                &mut settings.timeout_ms,
                &inherited.timeout_ms,
                |ui, ms| {
                    ui.add(egui::DragValue::new(ms).range(0..=3_600_000).suffix(" ms"))
                        .changed()
                },
                |ms| format!("{ms} ms"),
                context,
                strings,
            );
            changed |= setting_row(
                ui,
                strings.follow_redirects,
                &mut settings.follow_redirects,
                &inherited.follow_redirects,
                |ui, on| ui.checkbox(on, "").changed(),
                yes_no,
                context,
                strings,
            );
            changed |= setting_row(
                ui,
                strings.max_redirects,
                &mut settings.max_redirects,
                &inherited.max_redirects,
                |ui, n| ui.add(egui::DragValue::new(n).range(0..=100)).changed(),
                u32::to_string,
                context,
                strings,
            );
            changed |= setting_row(
                ui,
                strings.encode_url,
                &mut settings.encode_url,
                &inherited.encode_url,
                |ui, on| ui.checkbox(on, "").changed(),
                yes_no,
                context,
                strings,
            );
        });
    changed
}

/// One setting: its name, an "inherit" checkbox, and its editor or the
/// inherited value with where it comes from.
#[expect(clippy::too_many_arguments, reason = "one row of a grid, kept flat")]
fn setting_row<T: Clone>(
    ui: &mut egui::Ui,
    label: &str,
    setting: &mut Setting<T>,
    inherited: &Resolved<T>,
    edit: impl FnOnce(&mut egui::Ui, &mut T) -> bool,
    show: impl Fn(&T) -> String,
    context: &TabContext,
    strings: &Strings,
) -> bool {
    ui.label(label);
    let mut inherits = matches!(setting, Setting::Inherit);
    let mut changed = false;
    if ui.checkbox(&mut inherits, strings.inherit).changed() {
        *setting = if inherits {
            Setting::Inherit
        } else {
            // Start from the value it had.
            Setting::Set(inherited.value.clone())
        };
        changed = true;
    }
    match setting {
        Setting::Inherit => {
            let origin = strings.origin(&inherited.origin, &context.folders);
            ui.label(muted(ui, &format!("{} ({origin})", show(&inherited.value))));
        }
        Setting::Set(value) => changed |= edit(ui, value),
    }
    ui.end_row();
    changed
}
