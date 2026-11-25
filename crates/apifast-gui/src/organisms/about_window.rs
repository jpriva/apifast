//! The About window: the icon, name, version and commit, apifast's license
//! and those of its dependencies, the site, the folders in use, and a
//! button that copies what a bug report needs.

use std::fmt::Write as _;

use eframe::egui::{self, RichText};

use crate::licenses::{self, Dependency};
use crate::organisms::Folders;
use crate::strings::{Strings, fill};
use crate::theme;

/// Size the icon is drawn at.
const ICON_SIZE: f32 = 96.0;
/// Licenses named in the summary; the rest are counted together.
const SUMMARY_LICENSES: usize = 5;
/// Height of the list of dependencies.
const LIST_HEIGHT: f32 = 160.0;

/// The window's state.
#[derive(Default)]
pub struct AboutWindow {
    /// Whether it is open.
    pub open: bool,
    /// The graphics backend and adapter, once the window knows them.
    pub graphics: Option<String>,
    /// The operating system, read once.
    system: Option<String>,
    /// The icon, loaded the first time the window shows.
    icon: Option<egui::TextureHandle>,
    /// Whether the information was copied since the window opened.
    copied: bool,
}

impl std::fmt::Debug for AboutWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AboutWindow")
            .field("open", &self.open)
            .field("graphics", &self.graphics)
            .finish_non_exhaustive()
    }
}

impl AboutWindow {
    /// What a bug report needs: the version, the system and the graphics.
    pub fn report(&mut self) -> String {
        let system = self.system().to_owned();
        let mut report = format!("apifast {}\nSystem: {system}\n", crate::VERSION);
        if let Some(graphics) = &self.graphics {
            let _ = writeln!(report, "Graphics: {graphics}");
        }
        report
    }

    fn system(&mut self) -> &str {
        self.system
            .get_or_insert_with(crate::system::description)
            .as_str()
    }

    /// Draws the window, if open; `folders` are those the settings show.
    pub fn show(&mut self, ctx: &egui::Context, folders: &Folders, strings: &Strings) {
        if !self.open {
            self.copied = false;
            return;
        }
        let mut open = self.open;
        egui::Window::new(strings.about_title)
            .id(egui::Id::unique("about-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| {
                self.header(ui, strings);
                ui.separator();
                self.details(ui, folders, strings);
                ui.separator();
                dependencies(ui, licenses::DEPENDENCIES, strings);
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    if ui.button(strings.about_copy).clicked() {
                        let report = self.report();
                        ui.ctx().copy_text(report);
                        self.copied = true;
                    }
                    if self.copied {
                        let muted = theme::palette(ui).text_muted;
                        ui.label(RichText::new(strings.about_copied).color(muted));
                    }
                });
            });
        self.open = open;
    }

    /// The icon, the name, what apifast is and the links.
    fn header(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let muted = theme::palette(ui).text_muted;
        ui.vertical_centered(|ui| {
            if let Some(icon) = self.icon(ui.ctx()) {
                ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::Vec2::splat(ICON_SIZE)));
            }
            ui.heading(RichText::new("apifast").strong());
            ui.label(RichText::new(strings.about_summary).color(muted));
            ui.horizontal(|ui| {
                // Centered: the row is as wide as its links.
                let links = [
                    (strings.about_site, licenses::SITE),
                    (strings.about_bugs, licenses::BUGS),
                ];
                let width: f32 = links
                    .iter()
                    .map(|(text, _)| {
                        ui.fonts_mut(|f| {
                            f.layout_no_wrap(
                                (*text).to_owned(),
                                egui::TextStyle::Body.resolve(ui.style()),
                                egui::Color32::WHITE,
                            )
                            .size()
                            .x
                        })
                    })
                    .sum::<f32>()
                    + ui.spacing().item_spacing.x;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                for (text, url) in links {
                    ui.hyperlink_to(text, url).on_hover_text(url);
                }
            });
        });
    }

    /// The version, the license, the system and the folders.
    fn details(&mut self, ui: &mut egui::Ui, folders: &Folders, strings: &Strings) {
        let system = self.system().to_owned();
        let graphics = self.graphics.clone();
        egui::Grid::new("about-details")
            .num_columns(2)
            .spacing([theme::space::L, theme::space::S])
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, value: &str, mono: bool| {
                    ui.label(label);
                    let text = if mono {
                        RichText::new(value).monospace()
                    } else {
                        RichText::new(value)
                    };
                    ui.add(egui::Label::new(text).selectable(true));
                    ui.end_row();
                };
                row(ui, strings.about_version, crate::VERSION, false);
                row(ui, strings.about_license, licenses::LICENSE, false);
                row(ui, strings.about_system, &system, false);
                if let Some(graphics) = &graphics {
                    row(ui, strings.about_graphics, graphics, false);
                }
                for (label, folder) in [
                    (strings.folder_data, &folders.data),
                    (strings.folder_config, &folders.config),
                ] {
                    if let Some(folder) = folder {
                        row(ui, label, &folder.display().to_string(), true);
                    }
                }
            });
    }

    /// The icon as a texture, made once.
    fn icon(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if self.icon.is_none() {
            let data = crate::app::window_icon()?;
            let size = [
                usize::try_from(data.width).ok()?,
                usize::try_from(data.height).ok()?,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &data.rgba);
            self.icon = Some(ctx.load_texture("apifast-icon", image, egui::TextureOptions::LINEAR));
        }
        self.icon.clone()
    }
}

/// How many dependencies there are and under which licenses, with the whole
/// list folded underneath.
fn dependencies(ui: &mut egui::Ui, dependencies: &[Dependency], strings: &Strings) {
    let muted = theme::palette(ui).text_muted;
    if dependencies.is_empty() {
        ui.label(RichText::new(strings.about_dependencies_none).color(muted));
        return;
    }
    let title = strings.count(&strings.about_dependencies, dependencies.len(), &[]);
    let counts = licenses::by_license(dependencies);
    let mut summary: Vec<String> = (counts.iter().take(SUMMARY_LICENSES))
        .map(|(license, n)| fill("{license} ({n})", &[("license", license), ("n", n)]))
        .collect();
    let others: usize = counts.iter().skip(SUMMARY_LICENSES).map(|(_, n)| n).sum();
    if others > 0 {
        summary.push(strings.count(&strings.about_other_licenses, others, &[]));
    }
    let summary = summary.join(", ");
    egui::CollapsingHeader::new(title)
        .id_salt("about-dependencies")
        .show(ui, |ui| {
            ui.label(
                RichText::new(strings.about_dependencies_hint)
                    .small()
                    .color(muted),
            );
            ui.label(RichText::new(summary).small());
            egui::ScrollArea::vertical()
                .id_salt("about-dependency-list")
                .max_height(LIST_HEIGHT)
                .show(ui, |ui| {
                    egui::Grid::new("about-dependency-grid")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            for dependency in dependencies {
                                ui.label(dependency.name);
                                ui.label(RichText::new(dependency.version).color(muted));
                                ui.label(dependency.license);
                                ui.end_row();
                            }
                        });
                });
        });
}
