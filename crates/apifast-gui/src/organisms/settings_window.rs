//! The settings window: appearance, responses, and how much history is
//! kept. Changes apply at once; the screen saves them.

use eframe::egui::{self, RichText};

use crate::organisms::proxy_fields;
use crate::settings::ProxyModeSetting;
use crate::settings::{
    DAYS_RANGE, HISTORY_ENTRIES_RANGE, PRETTY_MB_RANGE, RESPONSES_MB_RANGE, SCALE_RANGE, Settings,
};
use crate::strings::Strings;
use crate::theme::{self, ThemeChoice};

/// The window's state.
#[derive(Debug, Default)]
pub struct SettingsWindow {
    /// Whether it is open.
    pub open: bool,
    /// Whether a password is kept for the proxy; the screen tells.
    pub password_saved: bool,
    /// The proxy password being typed; never kept here once saved.
    password: String,
    /// The password to keep (`Some`) or forget (`None`), once, since the
    /// last time it was taken.
    pub password_change: Option<Option<apifast_domain::Secret>>,
}

fn theme_label(theme: ThemeChoice, strings: &Strings) -> &str {
    match theme {
        ThemeChoice::System => strings.theme_system,
        ThemeChoice::Light => strings.theme_light,
        ThemeChoice::Dark => strings.theme_dark,
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
        let mut changed = false;
        let mut open = self.open;
        egui::Window::new(strings.settings_title)
            .id(egui::Id::unique("settings-window"))
            .open(&mut open)
            .default_width(460.0)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                changed |= appearance(ui, settings, strings);

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

                changed |= storage(ui, settings, strings);
                changed |= self.proxy(ui, settings, strings);
                ui.add_space(theme::space::M);
                if ui.button(strings.settings_reset).clicked() {
                    *settings = Settings::default();
                    changed = true;
                }
            });
        self.open = open;
        changed
    }
}

impl SettingsWindow {
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
            egui::Grid::new("settings-proxy")
                .num_columns(2)
                .spacing([theme::space::L, theme::space::S])
                .show(ui, |ui| {
                    changed |=
                        proxy_fields(ui, "settings-proxy", &mut settings.proxy, None, strings);
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
                            if self.password_saved
                                && ui.button(strings.proxy_forget_password).clicked()
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
}

/// Theme and scale. Returns whether they changed.
fn appearance(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    section(ui, strings.settings_appearance);
    egui::Grid::new("settings-appearance")
        .num_columns(2)
        .spacing([theme::space::L, theme::space::S])
        .show(ui, |ui| {
            ui.label(strings.settings_theme);
            egui::ComboBox::from_id_salt("settings-theme")
                .selected_text(theme_label(settings.theme, strings))
                .show_ui(ui, |ui| {
                    for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                        changed |= ui
                            .selectable_value(
                                &mut settings.theme,
                                choice,
                                theme_label(choice, strings),
                            )
                            .changed();
                    }
                })
                .response
                .on_hover_text(strings.settings_theme);
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
        });
    changed
}

/// How much history and how many responses are kept. Returns whether that
/// changed.
fn storage(ui: &mut egui::Ui, settings: &mut Settings, strings: &Strings) -> bool {
    let mut changed = false;
    let muted = theme::palette(ui).text_muted;
    section(ui, strings.settings_storage);
    egui::Grid::new("settings-storage")
        .num_columns(2)
        .spacing([theme::space::L, theme::space::S])
        .show(ui, |ui| {
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
