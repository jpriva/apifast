//! The settings window (`Ctrl+,`), in sections: general, appearance,
//! editor, network, shortcuts and data. Changes apply at once; the screen
//! saves them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use eframe::egui::{self, RichText};

use crate::organisms::proxy_fields;
use crate::settings::ProxyModeSetting;
use crate::settings::{
    DAYS_RANGE, HISTORY_ENTRIES_RANGE, PRETTY_MB_RANGE, RESPONSES_MB_RANGE, SCALE_RANGE,
    SYSTEM_LANGUAGE, Settings, TAB_SIZES, TIMEOUT_S_RANGE,
};
use crate::strings::Strings;
use crate::theme::{self, Accent, Density, ThemeChoice};

/// Width of the list of sections.
const NAV_WIDTH: f32 = 130.0;
/// Width and height of a section's contents.
const CONTENT_SIZE: egui::Vec2 = egui::vec2(470.0, 420.0);

/// A section of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsSection {
    /// Language, the workspace at the start, closing.
    #[default]
    General,
    /// Theme, accent, scale, density.
    Appearance,
    /// Code: font, size, Tab, wrapping, line numbers; responses.
    Editor,
    /// Timeout, proxy, TLS, cookies.
    Network,
    /// The shortcuts, edited in their own window.
    Shortcuts,
    /// History, the log, and the folders in use.
    Data,
}

impl SettingsSection {
    /// Every section, in the order the window lists them.
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Appearance,
        Self::Editor,
        Self::Network,
        Self::Shortcuts,
        Self::Data,
    ];

    /// Its name.
    pub fn name(self, strings: &Strings) -> &'static str {
        match self {
            Self::General => strings.settings_general,
            Self::Appearance => strings.settings_appearance,
            Self::Editor => strings.settings_editor,
            Self::Network => strings.settings_network,
            Self::Shortcuts => strings.settings_shortcuts,
            Self::Data => strings.settings_data,
        }
    }
}

/// A file being chosen in the system's picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picking {
    /// Certificate authorities.
    Ca,
    /// The code font.
    Font,
}

/// The space the data folder takes.
#[derive(Debug, Default)]
enum Space {
    /// Not asked for yet.
    #[default]
    Unknown,
    /// Being measured, away from the interface.
    Measuring(Receiver<u64>),
    /// So many bytes.
    Measured(u64),
}

/// The window's state.
#[derive(Debug, Default)]
pub struct SettingsWindow {
    /// Whether it is open.
    pub open: bool,
    /// The section shown.
    pub section: SettingsSection,
    /// Whether a password is kept for the proxy; the screen tells.
    pub password_saved: bool,
    /// The proxy password being typed; never kept here once saved.
    password: String,
    /// The password to keep (`Some`) or forget (`None`), once, since the
    /// last time it was taken.
    pub password_change: Option<Option<apifast_domain::Secret>>,
    /// The system's file picker, while it is open.
    picking: Option<(Picking, Receiver<Option<PathBuf>>)>,
    /// Where the log goes, to show it.
    pub log: LogInfo,
    /// The folders apifast uses, to show and open them.
    pub folders: Folders,
    /// The space the data folder takes, measured when the data section
    /// shows.
    space: Space,
    /// The shortcuts' window was asked for, once, since the last time it
    /// was taken.
    pub open_shortcuts: bool,
    /// Why a folder could not be opened.
    folder_error: Option<String>,
}

/// Where the log goes, for the settings to show it.
#[derive(Debug, Clone, Default)]
pub struct LogInfo {
    /// The log's file, if it has one.
    pub file: Option<PathBuf>,
    /// `APIFAST_LOG`, when it sets the filter instead of the level setting.
    pub env_filter: Option<String>,
}

/// The folders apifast uses.
#[derive(Debug, Clone, Default)]
pub struct Folders {
    /// History, responses, workspaces and the log.
    pub data: Option<PathBuf>,
    /// `settings.yml` and `keymap.yml`.
    pub config: Option<PathBuf>,
}

fn theme_label(theme: ThemeChoice, strings: &Strings) -> &str {
    match theme {
        ThemeChoice::System => strings.theme_system,
        ThemeChoice::Light => strings.theme_light,
        ThemeChoice::Dark => strings.theme_dark,
    }
}

fn accent_label(accent: Accent, strings: &Strings) -> &str {
    match accent {
        Accent::Blue => strings.accent_blue,
        Accent::Green => strings.accent_green,
        Accent::Teal => strings.accent_teal,
        Accent::Purple => strings.accent_purple,
        Accent::Pink => strings.accent_pink,
        Accent::Orange => strings.accent_orange,
    }
}

fn density_label(density: Density, strings: &Strings) -> &str {
    match density {
        Density::Compact => strings.density_compact,
        Density::Normal => strings.density_normal,
        Density::Comfortable => strings.density_comfortable,
    }
}

fn tab_label(size: u8, strings: &Strings) -> String {
    if size == 0 {
        strings.tab_character.to_owned()
    } else {
        crate::strings::fill(strings.tab_spaces, &[("n", &size)])
    }
}

impl SettingsWindow {
    /// Draws the window, if open, editing `settings`. Returns whether they
    /// changed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut Settings,
        strings: &Strings,
    ) -> bool {
        if !self.open {
            return false;
        }
        let mut changed = self.take_picked(settings);
        let mut open = self.open;
        egui::Window::new(strings.settings_title)
            .id(egui::Id::unique("settings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // The separator between the list and the contents
                    // takes the height it is given: without a limit, all.
                    ui.set_max_height(CONTENT_SIZE.y);
                    ui.vertical(|ui| {
                        ui.set_width(NAV_WIDTH);
                        for section in SettingsSection::ALL {
                            let name = section.name(strings);
                            ui.add_sized(
                                [NAV_WIDTH, 0.0],
                                egui::Button::selectable(self.section == section, name),
                            )
                            .clicked()
                            .then(|| self.section = section);
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("settings-content")
                            .max_height(CONTENT_SIZE.y)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(CONTENT_SIZE.x);
                                ui.heading(self.section.name(strings));
                                changed |= self.section_contents(ui, settings, strings);
                            });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(strings.settings_reset).clicked() {
                        *settings = Settings::default();
                        changed = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let muted = theme::palette(ui).text_muted;
                        ui.label(
                            RichText::new(format!("apifast {}", crate::VERSION))
                                .small()
                                .color(muted),
                        );
                    });
                });
            });
        self.open = open;
        changed
    }

    fn section_contents(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        strings: &Strings,
    ) -> bool {
        match self.section {
            SettingsSection::General => general(ui, settings, strings),
            SettingsSection::Appearance => appearance(ui, settings, strings),
            SettingsSection::Editor => self.editor(ui, settings, strings),
            SettingsSection::Network => {
                let mut changed = timeout(ui, settings, strings);
                changed |= self.proxy(ui, settings, strings);
                changed |= self.tls(ui, settings, strings);
                changed | cookies(ui, settings, strings)
            }
            SettingsSection::Shortcuts => {
                let muted = theme::palette(ui).text_muted;
                ui.label(RichText::new(strings.settings_shortcuts_hint).color(muted));
                ui.add_space(theme::space::M);
                if ui.button(strings.settings_edit_shortcuts).clicked() {
                    self.open_shortcuts = true;
                }
                false
            }
            SettingsSection::Data => {
                let mut changed = storage(ui, settings, strings);
                changed |= log(ui, settings, &self.log, strings);
                self.folders(ui, strings);
                changed
            }
        }
    }

    /// Opens the system's file picker for `what`, unless one is open.
    fn pick(&mut self, ui: &egui::Ui, what: Picking) {
        if self.picking.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let dialog = match what {
                Picking::Ca => dialog.add_filter("PEM", &["pem", "crt", "cer"]),
                Picking::Font => dialog.add_filter("TTF, OTF", &["ttf", "otf", "ttc"]),
            };
            let _ = sender.send(dialog.pick_file());
            ctx.request_repaint();
        });
        self.picking = Some((what, receiver));
    }

    /// Puts in `settings` the file picked, once the picker closes. Returns
    /// whether they changed.
    fn take_picked(&mut self, settings: &mut Settings) -> bool {
        let Some((what, receiver)) = &self.picking else {
            return false;
        };
        let Ok(file) = receiver.try_recv() else {
            return false;
        };
        let what = *what;
        self.picking = None;
        let Some(file) = file else {
            return false;
        };
        let file = file.display().to_string();
        match what {
            Picking::Ca => settings.ca_file = file,
            Picking::Font => settings.code_font = file,
        }
        true
    }

    /// The code: its font and size, what Tab inserts, wrapping and line
    /// numbers; and how large a response is shown pretty.
    fn editor(&mut self, ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
        let mut changed = false;
        let muted = theme::palette(ui).text_muted;
        grid(ui, "settings-editor", |ui| {
            ui.label(strings.settings_code_font);
            ui.horizontal(|ui| {
                let shown = if settings.code_font.is_empty() {
                    RichText::new(strings.settings_code_font_default).color(muted)
                } else {
                    RichText::new(file_name(&settings.code_font)).monospace()
                };
                ui.label(shown).on_hover_text(&settings.code_font);
                let picking = self.picking.is_some();
                if ui
                    .add_enabled(!picking, egui::Button::new(strings.choose))
                    .clicked()
                {
                    self.pick(ui, Picking::Font);
                }
                if !settings.code_font.is_empty()
                    && ui.button(strings.settings_code_font_reset).clicked()
                {
                    settings.code_font.clear();
                    changed = true;
                }
            });
            ui.end_row();
            ui.label("");
            ui.label(
                RichText::new(strings.settings_code_font_hint)
                    .small()
                    .color(muted),
            );
            ui.end_row();
            ui.label(strings.settings_code_size);
            let (min, max) = theme::text::CODE_RANGE;
            changed |= ui
                .add(
                    egui::Slider::new(&mut settings.code_size, min..=max)
                        .step_by(0.5)
                        .suffix(" pt"),
                )
                .accessible_name(strings.settings_code_size)
                .changed();
            ui.end_row();
            ui.label(strings.settings_tab);
            egui::ComboBox::from_id_salt("settings-tab")
                .selected_text(tab_label(settings.tab_size, strings))
                .show_ui(ui, |ui| {
                    for size in TAB_SIZES {
                        changed |= ui
                            .selectable_value(
                                &mut settings.tab_size,
                                size,
                                tab_label(size, strings),
                            )
                            .changed();
                    }
                })
                .response
                .on_hover_text(strings.settings_tab);
            ui.end_row();
            for (label, value) in [
                (strings.settings_wrap_lines, &mut settings.wrap_lines),
                (strings.settings_line_numbers, &mut settings.line_numbers),
            ] {
                ui.label(label);
                changed |= ui.checkbox(value, "").accessible_name(label).changed();
                ui.end_row();
            }
        });
        section(ui, strings.settings_responses);
        ui.horizontal(|ui| {
            ui.label(strings.settings_pretty_limit);
            changed |= number(
                ui,
                &mut settings.pretty_limit_mb,
                PRETTY_MB_RANGE,
                "MB",
                strings.settings_pretty_limit,
            );
        });
        changed
    }

    /// Trust in servers: an extra CA file, or no checks at all.
    fn tls(&mut self, ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
        let mut changed = false;
        section(ui, strings.tls);
        grid(ui, "settings-tls", |ui| {
            ui.label(strings.tls_ca_file);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut settings.ca_file)
                            .hint_text("/etc/ssl/certs/corp-ca.pem")
                            .desired_width(190.0),
                    )
                    .accessible_name(strings.tls_ca_file)
                    .changed();
                let picking = self.picking.is_some();
                if ui
                    .add_enabled(!picking, egui::Button::new(strings.choose))
                    .clicked()
                {
                    self.pick(ui, Picking::Ca);
                }
            });
            ui.end_row();
            ui.label(strings.tls_insecure);
            changed |= ui
                .checkbox(&mut settings.tls_insecure, "")
                .accessible_name(strings.tls_insecure)
                .changed();
            ui.end_row();
        });
        let palette = theme::palette(ui);
        if settings.tls_insecure {
            ui.label(RichText::new(strings.tls_insecure_warning).color(palette.danger));
        }
        changed
    }

    /// The application's proxy: for the collections that inherit theirs.
    fn proxy(&mut self, ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
        let mut changed = false;
        section(ui, strings.proxy);
        ui.horizontal(|ui| {
            for (mode, label) in [
                (ProxyModeSetting::System, strings.proxy_system),
                (ProxyModeSetting::Off, strings.proxy_off),
                (ProxyModeSetting::Manual, strings.proxy_manual),
            ] {
                changed |= ui
                    .radio_value(&mut settings.proxy_mode, mode, label)
                    .changed();
            }
        });
        if settings.proxy_mode == ProxyModeSetting::Manual {
            grid(ui, "settings-proxy", |ui| {
                changed |= proxy_fields(ui, "settings-proxy", &mut settings.proxy, None, strings);
                if settings.proxy.auth {
                    ui.label(strings.password);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.password)
                                .password(true)
                                .hint_text(if self.password_saved {
                                    strings.proxy_password_saved
                                } else {
                                    ""
                                })
                                .desired_width(160.0),
                        )
                        .accessible_name(strings.password);
                        if ui.button(strings.proxy_save_password).clicked() {
                            let password = std::mem::take(&mut self.password);
                            self.password_change =
                                Some(Some(apifast_domain::Secret::new(password)));
                        }
                        if self.password_saved && ui.button(strings.proxy_forget_password).clicked()
                        {
                            self.password_change = Some(None);
                        }
                    });
                    ui.end_row();
                }
            });
        }
        let muted = theme::palette(ui).text_muted;
        ui.label(RichText::new(strings.proxy_hint).color(muted).small());
        changed
    }

    /// The folders apifast uses, each with a button that opens it, and the
    /// space the data takes.
    fn folders(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let muted = theme::palette(ui).text_muted;
        section(ui, strings.settings_folders);
        let logs = (self.log.file.as_deref())
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let folders = [
            (strings.folder_data, self.folders.data.clone()),
            (strings.folder_config, self.folders.config.clone()),
            (strings.folder_logs, logs),
        ];
        grid(ui, "settings-folders", |ui| {
            for (label, folder) in folders {
                let Some(folder) = folder else { continue };
                ui.label(label);
                ui.horizontal(|ui| {
                    if ui.button(strings.settings_open_folder).clicked() {
                        self.folder_error = crate::system::open_folder(&folder).err().map(|e| {
                            crate::strings::fill(strings.settings_folder_error, &[("error", &e)])
                        });
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(folder.display().to_string())
                                .monospace()
                                .small(),
                        )
                        .selectable(true),
                    );
                });
                ui.end_row();
            }
        });
        if let Some(error) = &self.folder_error {
            ui.label(RichText::new(error).color(theme::palette(ui).danger));
        }
        if let Some(data) = self.folders.data.clone() {
            let text = match self.measure(ui.ctx(), &data) {
                Some(bytes) => crate::strings::fill(
                    strings.settings_space_used,
                    &[("size", &apifast_domain::human_size(bytes))],
                ),
                None => strings.settings_space_measuring.to_owned(),
            };
            ui.label(RichText::new(text).color(muted));
        }
    }

    /// The bytes `folder` takes, once measured; the first call starts it.
    fn measure(&mut self, ctx: &egui::Context, folder: &Path) -> Option<u64> {
        match &self.space {
            Space::Unknown => {
                let (sender, receiver) = std::sync::mpsc::channel();
                let (folder, ctx) = (folder.to_path_buf(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = sender.send(folder_size(&folder));
                    ctx.request_repaint();
                });
                self.space = Space::Measuring(receiver);
                None
            }
            Space::Measuring(receiver) => {
                let bytes = receiver.try_recv().ok()?;
                self.space = Space::Measured(bytes);
                Some(bytes)
            }
            Space::Measured(bytes) => Some(*bytes),
        }
    }
}

/// The bytes of the files under `folder`, without following links; what
/// cannot be read counts as nothing.
pub fn folder_size(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// The last part of `path`, or all of it.
fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A grid of labels and their controls.
fn grid<R>(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([theme::space::L, theme::space::S])
        .show(ui, add)
        .inner
}

/// Language, the workspace at the start, and closing with unsaved changes.
fn general(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    let muted = theme::palette(ui).text_muted;
    grid(ui, "settings-general", |ui| {
        ui.label(strings.language);
        changed |= language_choice(ui, settings, strings);
        ui.end_row();
        ui.label(strings.settings_reopen_workspace);
        changed |= ui
            .checkbox(&mut settings.reopen_workspace, "")
            .accessible_name(strings.settings_reopen_workspace)
            .on_hover_text(strings.settings_reopen_workspace_hint)
            .changed();
        ui.end_row();
        ui.label(strings.settings_confirm_close);
        changed |= ui
            .checkbox(&mut settings.confirm_close, "")
            .accessible_name(strings.settings_confirm_close)
            .changed();
        ui.end_row();
    });
    ui.label(
        RichText::new(strings.settings_confirm_close_hint)
            .small()
            .color(muted),
    );
    changed
}

/// Theme, accent, scale and density. Returns whether they changed.
fn appearance(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    let dark = ui.visuals().dark_mode;
    grid(ui, "settings-appearance", |ui| {
        ui.label(strings.settings_theme);
        egui::ComboBox::from_id_salt("settings-theme")
            .selected_text(theme_label(settings.theme, strings))
            .show_ui(ui, |ui| {
                for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                    changed |= ui
                        .selectable_value(&mut settings.theme, choice, theme_label(choice, strings))
                        .changed();
                }
            })
            .response
            .on_hover_text(strings.settings_theme);
        ui.end_row();
        ui.label(strings.settings_accent);
        let theme = if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        let color = |accent: Accent| accent.palette(theme).accent;
        ui.horizontal(|ui| {
            swatch(ui, color(settings.accent));
            egui::ComboBox::from_id_salt("settings-accent")
                .selected_text(accent_label(settings.accent, strings))
                .show_ui(ui, |ui| {
                    for accent in Accent::ALL {
                        ui.horizontal(|ui| {
                            swatch(ui, color(accent));
                            changed |= ui
                                .selectable_value(
                                    &mut settings.accent,
                                    accent,
                                    accent_label(accent, strings),
                                )
                                .changed();
                        });
                    }
                })
                .response
                .on_hover_text(strings.settings_accent);
        });
        ui.end_row();
        ui.label(strings.settings_scale);
        changed |= ui
            .add(
                egui::Slider::new(&mut settings.ui_scale, SCALE_RANGE.0..=SCALE_RANGE.1)
                    .step_by(0.05)
                    .custom_formatter(|n, _| format!("{:.0}%", n * 100.0)),
            )
            .on_hover_text(strings.settings_scale_hint)
            .changed();
        ui.end_row();
        ui.label(strings.settings_density);
        ui.horizontal(|ui| {
            for density in Density::ALL {
                changed |= ui
                    .radio_value(
                        &mut settings.density,
                        density,
                        density_label(density, strings),
                    )
                    .changed();
            }
        });
        ui.end_row();
    });
    changed
}

/// A square of `color`: painted, as the fonts have no square glyph.
fn swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let size = egui::vec2(12.0, 12.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, theme::radius::SMALL, color);
}

/// The language: the system's, or one of those apifast has, each by its
/// name in it.
fn language_choice(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let name = |code: &str| {
        if code == SYSTEM_LANGUAGE {
            strings.language_system
        } else {
            crate::strings::language(code).map_or(strings.language_system, |l| l.name)
        }
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt("settings-language")
        .selected_text(name(&settings.language))
        .show_ui(ui, |ui| {
            let codes = std::iter::once(SYSTEM_LANGUAGE)
                .chain(crate::strings::LANGUAGES.iter().map(|l| l.code));
            for code in codes {
                let chosen = settings.language == code;
                if ui.selectable_label(chosen, name(code)).clicked() && !chosen {
                    code.clone_into(&mut settings.language);
                    changed = true;
                }
            }
        })
        .response
        .on_hover_text(strings.language);
    changed
}

/// The timeout of requests that set none.
fn timeout(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let muted = theme::palette(ui).text_muted;
    let changed = ui
        .horizontal(|ui| {
            ui.label(strings.settings_timeout);
            number(
                ui,
                &mut settings.timeout_s,
                TIMEOUT_S_RANGE,
                "s",
                strings.settings_timeout,
            )
        })
        .inner;
    ui.label(
        RichText::new(strings.settings_timeout_hint)
            .small()
            .color(muted),
    );
    changed
}

/// How much history and how many responses are kept. Returns whether that
/// changed.
fn storage(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    let muted = theme::palette(ui).text_muted;
    section(ui, strings.settings_storage);
    grid(ui, "settings-storage", |ui| {
        ui.label(strings.settings_history_entries);
        changed |= number(
            ui,
            &mut settings.history_entries,
            HISTORY_ENTRIES_RANGE,
            "",
            strings.settings_history_entries,
        );
        ui.end_row();
        ui.label(strings.settings_keep_days);
        changed |= number(
            ui,
            &mut settings.keep_days,
            DAYS_RANGE,
            strings.days,
            strings.settings_keep_days,
        );
        ui.end_row();
        ui.label(strings.settings_responses_mb);
        changed |= number(
            ui,
            &mut settings.responses_mb,
            RESPONSES_MB_RANGE,
            "MB",
            strings.settings_responses_mb,
        );
        ui.end_row();
    });
    ui.label(
        RichText::new(strings.settings_storage_hint)
            .color(muted)
            .small(),
    );
    changed
}

/// Whether cookies from the jars are sent, and those responses set kept.
fn cookies(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    section(ui, strings.cookies);
    grid(ui, "settings-cookies", |ui| {
        for (label, value) in [
            (strings.send_cookies, &mut settings.send_cookies),
            (strings.store_cookies, &mut settings.store_cookies),
        ] {
            ui.label(label);
            changed |= ui.checkbox(value, "").accessible_name(label).changed();
            ui.end_row();
        }
    });
    changed
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(theme::space::M);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// A number within `range`, with `unit` after it. Returns whether it changed.
fn number(
    ui: &mut egui::Ui,
    value: &mut u32,
    (min, max): (u32, u32),
    unit: &str,
    name: &str,
) -> bool {
    let mut drag = egui::DragValue::new(value).range(min..=max).speed(1.0);
    if !unit.is_empty() {
        drag = drag.suffix(format!(" {unit}"));
    }
    ui.add(drag).accessible_name(name).changed()
}

/// The log: its level, its file, or that `APIFAST_LOG` sets it.
fn log(ui: &mut egui::Ui, settings: &mut Settings, info: &LogInfo, strings: &Strings) -> bool {
    use apifast_log::LogLevel;
    let name = |level: LogLevel| match level {
        LogLevel::Error => strings.log_error,
        LogLevel::Warn => strings.log_warn,
        LogLevel::Info => strings.log_info,
        LogLevel::Debug => strings.log_debug,
        LogLevel::Trace => strings.log_trace,
    };
    let muted = theme::palette(ui).text_muted;
    let mut changed = false;
    section(ui, strings.settings_log);
    ui.horizontal(|ui| {
        ui.label(strings.settings_log_level);
        ui.add_enabled_ui(info.env_filter.is_none(), |ui| {
            egui::ComboBox::from_id_salt("settings-log-level")
                .selected_text(name(settings.log_level))
                .show_ui(ui, |ui| {
                    for level in LogLevel::ALL {
                        changed |= ui
                            .selectable_value(&mut settings.log_level, level, name(level))
                            .changed();
                    }
                })
                .response
                .on_hover_text(strings.settings_log_level);
        });
    });
    if let Some(filter) = &info.env_filter {
        let text = crate::strings::fill(strings.settings_log_env, &[("filter", filter)]);
        ui.label(RichText::new(text).small().color(muted));
    }
    if let Some(file) = &info.file {
        ui.horizontal(|ui| {
            ui.label(strings.settings_log_file);
            ui.add(
                egui::Label::new(
                    RichText::new(file.display().to_string())
                        .monospace()
                        .small(),
                )
                .selectable(true),
            );
        });
    }
    ui.label(
        RichText::new(strings.settings_log_hint)
            .small()
            .color(muted),
    );
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_measures_the_files_under_it() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir(dir.path().join("sub")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("a"), [0; 10]).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("sub/b"), [0; 5]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(folder_size(dir.path()), 15);
        assert_eq!(folder_size(&dir.path().join("missing")), 0);
    }
}
