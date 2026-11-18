//! The focused collection's cookie jar: see its cookies by domain, add,
//! change and delete them (docs/07, "Cookies").

use apifast_domain::{Cookie, SameSite, format_expiry, parse_expiry};
use eframe::egui::{self, RichText};

use crate::core::{CookieCommand, CookieSlot};
use crate::strings::Strings;
use crate::theme;

/// Width of the text fields of the form, in points.
const FIELD_WIDTH: f32 = 320.0;

/// Values longer than this are cut in the list; the form shows them whole.
const SHOWN_VALUE: usize = 40;

/// The window's state.
#[derive(Debug, Default)]
pub struct CookiesWindow {
    /// Whether it is open.
    pub open: bool,
    /// The cookie being added or changed.
    editing: Option<CookieDraft>,
    /// Asking to confirm deleting every cookie.
    confirming_clear: bool,
}

/// A cookie as the form edits it: its fields as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieDraft {
    /// The cookie it changes; `None` for a new one.
    pub replaces: Option<CookieSlot>,
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
    /// Domain.
    pub domain: String,
    /// Path.
    pub path: String,
    /// Expiry as `YYYY-MM-DD HH:MM:SS` in UTC; empty for a session cookie.
    pub expires: String,
    /// Only for the domain itself, not its subdomains.
    pub host_only: bool,
    /// Only over a secure connection.
    pub secure: bool,
    /// Not for scripts.
    pub http_only: bool,
    /// `SameSite`.
    pub same_site: Option<SameSite>,
    created_ms: i64,
}

/// Why a draft is not a cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    /// No name, or one with `=`, `;` or spaces.
    Name,
    /// No domain.
    Domain,
    /// A path that does not start with `/`.
    Path,
    /// An expiry that is not a date.
    Expires,
}

impl CookieDraft {
    /// A new cookie for `domain`, at `/`, for the session.
    pub fn new(domain: &str) -> Self {
        Self {
            replaces: None,
            name: String::new(),
            value: String::new(),
            domain: domain.to_owned(),
            path: "/".into(),
            expires: String::new(),
            host_only: true,
            secure: false,
            http_only: false,
            same_site: None,
            created_ms: 0,
        }
    }

    /// `cookie`, to change it.
    pub fn of(cookie: &Cookie) -> Self {
        Self {
            replaces: Some(CookieSlot::of(cookie)),
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires_ms.map(format_expiry).unwrap_or_default(),
            host_only: cookie.host_only,
            secure: cookie.secure,
            http_only: cookie.http_only,
            same_site: cookie.same_site,
            created_ms: cookie.created_ms,
        }
    }

    /// The cookie, made at `now_ms` if it is new.
    ///
    /// # Errors
    ///
    /// If a field is not valid.
    pub fn to_cookie(&self, now_ms: i64) -> Result<Cookie, DraftError> {
        let name = self.name.trim();
        if name.is_empty() || name.contains(['=', ';', ' ', '\t']) {
            return Err(DraftError::Name);
        }
        let domain = self
            .domain
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if domain.is_empty() {
            return Err(DraftError::Domain);
        }
        let path = self.path.trim();
        if !path.starts_with('/') {
            return Err(DraftError::Path);
        }
        let expires_ms = match self.expires.trim() {
            "" => None,
            text => Some(parse_expiry(text).ok_or(DraftError::Expires)?),
        };
        Ok(Cookie {
            name: name.to_owned(),
            value: self.value.trim().to_owned(),
            domain,
            host_only: self.host_only,
            path: path.to_owned(),
            secure: self.secure,
            http_only: self.http_only,
            same_site: self.same_site,
            expires_ms,
            created_ms: if self.replaces.is_some() {
                self.created_ms
            } else {
                now_ms
            },
        })
    }
}

impl CookiesWindow {
    /// Draws the window, if open, over the cookies of the focused
    /// collection, named `collection`. Returns what it changes.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        collection: Option<&str>,
        cookies: &[Cookie],
        now_ms: i64,
        strings: &Strings,
    ) -> Vec<CookieCommand> {
        if !self.open {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let mut open = self.open;
        let title = match collection {
            Some(name) => format!("{} · {name}", strings.cookies_title),
            None => strings.cookies_title.to_owned(),
        };
        egui::Window::new(title)
            .id(egui::Id::unique("cookies-window"))
            .open(&mut open)
            .default_width(720.0)
            .collapsible(false)
            .show(ctx, |ui| {
                if collection.is_none() {
                    let muted = theme::palette(ui).text_muted;
                    ui.label(RichText::new(strings.cookies_no_collection).color(muted));
                    return;
                }
                if let Some(command) = self.form(ui, now_ms, strings) {
                    commands.push(command);
                    return;
                }
                if self.editing.is_none() {
                    self.toolbar(ui, cookies, strings);
                    commands.extend(self.list(ui, cookies, now_ms, strings));
                }
            });
        self.open = open;
        commands.extend(self.confirm_clear(ctx, strings));
        commands
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cookies: &[Cookie], strings: &Strings) {
        ui.horizontal(|ui| {
            if ui.button(strings.cookies_add).clicked() {
                let domain = cookies.first().map_or("", |c| c.domain.as_str());
                self.editing = Some(CookieDraft::new(domain));
            }
            if ui
                .add_enabled(
                    !cookies.is_empty(),
                    egui::Button::new(strings.cookies_clear),
                )
                .clicked()
            {
                self.confirming_clear = true;
            }
        });
        ui.add_space(theme::space::S);
    }

    /// The cookies by domain, each with its buttons.
    fn list(
        &mut self,
        ui: &mut egui::Ui,
        cookies: &[Cookie],
        now_ms: i64,
        strings: &Strings,
    ) -> Vec<CookieCommand> {
        let palette = theme::palette(ui);
        if cookies.is_empty() {
            ui.label(RichText::new(strings.cookies_empty).color(palette.text_muted));
            return Vec::new();
        }
        let mut commands = Vec::new();
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                for domain in domains(cookies) {
                    let of_domain: Vec<&Cookie> =
                        cookies.iter().filter(|c| c.domain == domain).collect();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(domain).strong().monospace());
                        ui.label(
                            RichText::new(format!("({})", of_domain.len()))
                                .color(palette.text_muted),
                        );
                        if ui.small_button(strings.cookies_delete_domain).clicked() {
                            commands.push(CookieCommand::RemoveDomain(domain.to_owned()));
                        }
                    });
                    egui::Grid::new(("cookies", domain))
                        .striped(true)
                        .num_columns(6)
                        .show(ui, |ui| {
                            for header in [
                                strings.cookies_name,
                                strings.cookies_value,
                                strings.cookies_path,
                                strings.cookies_expires,
                                strings.cookies_flags,
                            ] {
                                ui.label(RichText::new(header).color(palette.text_muted).small());
                            }
                            ui.end_row();
                            for cookie in of_domain {
                                commands.extend(self.row(ui, cookie, now_ms, strings));
                                ui.end_row();
                            }
                        });
                    ui.add_space(theme::space::M);
                }
            });
        commands
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        cookie: &Cookie,
        now_ms: i64,
        strings: &Strings,
    ) -> Option<CookieCommand> {
        let palette = theme::palette(ui);
        ui.label(RichText::new(&cookie.name).monospace());
        ui.add(egui::Label::new(RichText::new(shown_value(&cookie.value)).monospace()).truncate())
            .on_hover_text(&cookie.value);
        ui.label(RichText::new(&cookie.path).monospace());
        let expires = match cookie.expires_ms {
            None => RichText::new(strings.cookies_session).color(palette.text_muted),
            Some(at) if at <= now_ms => {
                RichText::new(strings.cookies_expired).color(palette.danger)
            }
            Some(at) => RichText::new(format_expiry(at)),
        };
        ui.label(expires);
        ui.label(RichText::new(flags(cookie, strings)).small());
        let mut command = None;
        ui.horizontal(|ui| {
            if ui.small_button(strings.cookies_edit).clicked() {
                self.editing = Some(CookieDraft::of(cookie));
            }
            if ui.small_button(strings.cookies_delete).clicked() {
                command = Some(CookieCommand::Remove(CookieSlot::of(cookie)));
            }
        });
        command
    }

    /// The form of the cookie being added or changed, if any. Returns the
    /// command when it is saved.
    fn form(&mut self, ui: &mut egui::Ui, now_ms: i64, strings: &Strings) -> Option<CookieCommand> {
        let draft = self.editing.as_mut()?;
        let title = if draft.replaces.is_some() {
            strings.cookies_edit_title
        } else {
            strings.cookies_add_title
        };
        ui.label(RichText::new(title).strong());
        ui.add_space(theme::space::S);
        egui::Grid::new("cookie-form")
            .num_columns(2)
            .show(ui, |ui| {
                let fields = [
                    (strings.cookies_name, &mut draft.name, ""),
                    (strings.cookies_value, &mut draft.value, ""),
                    (strings.cookies_domain, &mut draft.domain, "api.example.com"),
                    (strings.cookies_path, &mut draft.path, "/"),
                    (
                        strings.cookies_expires,
                        &mut draft.expires,
                        strings.cookies_expires_hint,
                    ),
                ];
                for (label, text, hint) in fields {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(text)
                            .hint_text(hint)
                            .desired_width(FIELD_WIDTH),
                    )
                    .accessible_name(label);
                    ui.end_row();
                }
                ui.label(strings.cookies_same_site);
                egui::ComboBox::from_id_salt("cookie-same-site")
                    .selected_text(draft.same_site.map_or("—", SameSite::as_str))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.same_site, None, "—");
                        for choice in [SameSite::Strict, SameSite::Lax, SameSite::None] {
                            ui.selectable_value(
                                &mut draft.same_site,
                                Some(choice),
                                choice.as_str(),
                            );
                        }
                    })
                    .response
                    .on_hover_text(strings.cookies_same_site);
                ui.end_row();
            });
        ui.checkbox(&mut draft.host_only, strings.cookies_host_only);
        ui.checkbox(&mut draft.secure, strings.cookies_secure);
        ui.checkbox(&mut draft.http_only, strings.cookies_http_only);
        let result = draft.to_cookie(now_ms);
        if let Err(error) = result {
            let message = match error {
                DraftError::Name => strings.cookies_bad_name,
                DraftError::Domain => strings.cookies_bad_domain,
                DraftError::Path => strings.cookies_bad_path,
                DraftError::Expires => strings.cookies_bad_expires,
            };
            ui.label(
                RichText::new(message)
                    .color(theme::palette(ui).danger)
                    .small(),
            );
        }
        ui.add_space(theme::space::S);
        let mut command = None;
        let mut cancelled = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(result.is_ok(), egui::Button::new(strings.save))
                .clicked()
                && let Ok(cookie) = result
            {
                command = Some(CookieCommand::Set {
                    replaces: draft.replaces.clone(),
                    cookie: Box::new(cookie),
                });
            }
            cancelled = ui.button(strings.cancel).clicked();
        });
        if command.is_some() || cancelled {
            self.editing = None;
        }
        command
    }

    fn confirm_clear(&mut self, ctx: &egui::Context, strings: &Strings) -> Option<CookieCommand> {
        if !self.confirming_clear {
            return None;
        }
        let mut command = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm-clear-cookies")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(strings.cookies_clear_confirm);
            ui.horizontal(|ui| {
                if ui.button(strings.cookies_clear).clicked() {
                    command = Some(CookieCommand::Clear);
                    self.confirming_clear = false;
                }
                if ui.button(strings.cancel).clicked() {
                    self.confirming_clear = false;
                }
            });
        });
        if modal.should_close() {
            self.confirming_clear = false;
        }
        command
    }
}

/// The domains of `cookies`, in their order, each once.
fn domains(cookies: &[Cookie]) -> Vec<&str> {
    let mut domains: Vec<&str> = cookies.iter().map(|c| c.domain.as_str()).collect();
    domains.dedup();
    domains
}

/// `Secure`, `HttpOnly`, `SameSite` and whether subdomains get it.
pub fn flags(cookie: &Cookie, strings: &Strings) -> String {
    let mut flags = Vec::new();
    if !cookie.host_only {
        flags.push(strings.cookies_subdomains);
    }
    if cookie.secure {
        flags.push("Secure");
    }
    if cookie.http_only {
        flags.push("HttpOnly");
    }
    if let Some(same_site) = cookie.same_site {
        flags.push(same_site.as_str());
    }
    flags.join(" · ")
}

fn shown_value(value: &str) -> String {
    match value.char_indices().nth(SHOWN_VALUE) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_owned(),
    }
}
