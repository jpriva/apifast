//! The application: reads the core's latest snapshot every frame, draws the
//! screen that fits it and sends the user's commands back.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use apifast_storage_fs::{FsKeymapStore, FsLayoutStore, FsSettingsStore, LayoutFile};
use eframe::egui;

use crate::actions::Keymap;
use crate::core::{Command, Core};
use crate::screens::MainScreen;
use crate::settings::Settings;
use crate::strings::{self, Strings};

/// The whole interface.
pub struct App {
    core: Core,
    /// The system's language, if apifast has it: what the `system`
    /// language setting shows. `None` shows English.
    system_language: Option<&'static strings::Language>,
    /// The language the core was told last.
    told: Option<&'static str>,
    /// The log, to change its level with the settings.
    logging: Option<std::sync::Arc<apifast_log::Logging>>,
    main: MainScreen,
    /// Where the keymap is kept, if anywhere.
    keymap_store: Option<FsKeymapStore>,
    /// Where the settings are kept, if anywhere.
    settings_store: Option<FsSettingsStore>,
    /// Where the editor's layouts are kept.
    layouts_store: Option<FsLayoutStore>,
    /// Results of saves, which run away from the interface.
    saved: (mpsc::Sender<SaveResult>, mpsc::Receiver<SaveResult>),
    /// When the process started, to report how long it took to show the
    /// first frame and the collection's tree; `None` when not asked for.
    timings: Option<Timings>,
}

/// What is left to report of the startup times.
#[derive(Debug)]
struct Timings {
    started: Instant,
    first_frame: bool,
    tree: bool,
}

/// How saving went: an error ready to show.
type SaveResult = Result<(), String>;

impl App {
    /// The interface over `core`.
    pub fn new(core: Core) -> Self {
        Self {
            core,
            system_language: None,
            told: None,
            logging: None,
            main: MainScreen::default(),
            keymap_store: None,
            settings_store: None,
            layouts_store: None,
            saved: mpsc::channel(),
            timings: None,
        }
    }

    /// Shows `language` when the language setting says `system`, as the
    /// system's: what [`strings::system_language`] found.
    #[must_use]
    pub fn with_system_language(mut self, language: Option<&'static strings::Language>) -> Self {
        self.system_language = language;
        self.tell_language();
        self
    }

    /// The texts of the language the settings choose.
    fn strings(&self) -> &'static Strings {
        self.main.settings.strings(self.system_language)
    }

    /// Changes the level of `logging` with the settings, and shows in them
    /// where it goes: `file`, if it has one.
    #[must_use]
    pub fn with_logging(
        mut self,
        logging: std::sync::Arc<apifast_log::Logging>,
        file: Option<std::path::PathBuf>,
        env_filter: Option<String>,
    ) -> Self {
        logging.set_level(self.main.settings.log_level);
        self.main.settings_window.log = crate::organisms::LogInfo {
            file,
            env_filter: logging.from_env().then_some(env_filter).flatten(),
        };
        self.logging = Some(logging);
        self
    }

    /// Shows in the settings the folders apifast uses: `data` (history,
    /// responses, workspaces, the log) and `config` (settings, shortcuts).
    #[must_use]
    pub fn with_folders(mut self, data: PathBuf, config: PathBuf) -> Self {
        self.main.settings_window.folders = crate::organisms::Folders {
            data: Some(data),
            config: Some(config),
        };
        self
    }

    /// Tells the core the language, when it is not the one it has.
    fn tell_language(&mut self) {
        let code = self
            .main
            .settings
            .chosen_language(self.system_language)
            .code;
        if self.told != Some(code) {
            self.told = Some(code);
            self.core.send(Command::Language(code));
        }
    }

    /// Reads the shortcuts from `store`, and saves them there when the user
    /// changes them. What cannot be read is told on screen; the defaults
    /// stand in for it.
    #[must_use]
    pub fn with_keymap(mut self, store: FsKeymapStore) -> Self {
        match store.load() {
            Ok(entries) => {
                let (keymap, problems) = Keymap::from_entries(&entries);
                self.main.keymap = keymap;
                if !problems.is_empty() {
                    self.main.notice = Some(format!(
                        "{} {}",
                        self.strings().keymap_problems,
                        problems.join(", ")
                    ));
                }
            }
            Err(e) => self.main.notice = Some(e.to_string()),
        }
        self.keymap_store = Some(store);
        self
    }

    /// Reads the editor's layouts from `store`, puts the one left on
    /// screen, and saves them there when they change. What cannot be read
    /// is told on screen; the default layout stands in for it.
    #[must_use]
    pub fn with_layouts(mut self, store: FsLayoutStore) -> Self {
        match store.load() {
            Ok(file) => {
                if self.main.set_layouts((file.current, file.saved)) {
                    self.main.notice = Some(self.strings().layout_not_understood.to_owned());
                }
            }
            Err(e) => self.main.notice = Some(e.to_string()),
        }
        // What is on screen now is what the file has: nothing to write.
        let _ = self.main.take_layouts_change();
        self.layouts_store = Some(store);
        self
    }

    /// Writes to stderr how long after `started` (the start of the process)
    /// the first frame and the collection's tree were shown: the internal
    /// timestamps of `docs/10-rendimiento-y-memoria.md`.
    #[must_use]
    pub fn with_timings(mut self, started: Instant) -> Self {
        self.timings = Some(Timings {
            started,
            first_frame: false,
            tree: false,
        });
        self
    }

    /// Reports the startup times not reported yet.
    fn report_timings(&mut self, tree_shown: bool) {
        let Some(timings) = &mut self.timings else {
            return;
        };
        let ms = timings.started.elapsed().as_millis();
        let mut stderr = std::io::stderr();
        if !timings.first_frame {
            timings.first_frame = true;
            let _ = writeln!(stderr, "apifast: first frame after {ms} ms");
        }
        if tree_shown && !timings.tree {
            timings.tree = true;
            let _ = writeln!(stderr, "apifast: collection tree shown after {ms} ms");
        }
    }

    /// Reads the settings from `store`, tells the core its limits, and
    /// saves them there when the user changes them. What cannot be read is
    /// told on screen; the defaults stand in for it.
    #[must_use]
    pub fn with_settings(mut self, store: FsSettingsStore) -> Self {
        match store.load() {
            Ok(entries) => {
                let (settings, problems) = Settings::from_entries(&entries);
                self.main.settings = settings;
                if !problems.is_empty() {
                    self.main.notice = Some(format!(
                        "{} {}",
                        self.strings().settings_problems,
                        problems.join(", ")
                    ));
                }
            }
            Err(e) => self.main.notice = Some(e.to_string()),
        }
        self.configure_core();
        self.tell_language();
        self.settings_store = Some(store);
        self
    }

    fn configure_core(&self) {
        let settings = &self.main.settings;
        if let Some(logging) = &self.logging {
            logging.set_level(settings.log_level);
        }
        self.core.send(Command::Configure {
            history: settings.history_limits(),
            responses: settings.response_limits(),
            proxy: settings.proxy_choice(),
            trust: settings.trust(),
            cookies: (settings.send_cookies, settings.store_cookies),
            timeout_ms: settings.timeout_ms(),
            reopen_last: settings.reopen_workspace,
        });
    }

    /// Saves what changed, each on a thread of its own. Settings wait until
    /// the mouse is released, so dragging a slider does not write the file
    /// on every frame.
    fn save(&mut self, ctx: &egui::Context) {
        let done = self.saved.0.clone();
        if self.main.take_keymap_change()
            && let Some(store) = self.keymap_store.clone()
        {
            let entries = self.main.keymap.entries();
            std::thread::spawn(move || {
                let _ = done.send(store.save(&entries).map_err(|e| e.to_string()));
            });
        }
        let done = self.saved.0.clone();
        // A pane being dragged or resized is written once it is let go.
        if !ctx.input(|i| i.pointer.any_down())
            && let Some(store) = self.layouts_store.clone()
            && let Some((current, saved)) = self.main.take_layouts_change()
        {
            let file = LayoutFile {
                current: Some(current),
                saved,
            };
            std::thread::spawn(move || {
                let _ = done.send(store.save(&file).map_err(|e| e.to_string()));
            });
        }
        let done = self.saved.0.clone();
        if !ctx.input(|i| i.pointer.any_down()) && self.main.take_settings_change() {
            self.configure_core();
            if let Some(store) = self.settings_store.clone() {
                let entries = self.main.settings.entries();
                std::thread::spawn(move || {
                    let _ = done.send(store.save(&entries).map_err(|e| e.to_string()));
                });
            }
        }
    }

    /// The core, to send it commands from outside the interface, such as
    /// a folder given on the command line.
    pub fn core(&self) -> &Core {
        &self.core
    }

    /// Draws one frame.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        while let Ok(result) = self.saved.1.try_recv() {
            self.main.notice = result.err();
        }
        self.main.apply_appearance(ui.ctx(), self.strings());
        self.tell_language();
        let snapshot = self.core.snapshot();
        for command in self.main.show(ui, &snapshot, self.strings()) {
            self.core.send(command);
        }
        let ctx = ui.ctx().clone();
        self.save(&ctx);
        self.report_timings(snapshot.collection.is_some());
    }
}

impl App {
    /// The window is transparent where nothing is drawn, so apifast draws
    /// its frame (see [`crate::chrome::window_frame`]).
    #[must_use]
    pub fn with_transparent_window(mut self, transparent: bool) -> Self {
        self.main.transparent = transparent;
        self
    }
}

impl eframe::App for App {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.main.transparent {
            [0.0; 4]
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.main.about.graphics.is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.main.about.graphics = Some(graphics(&render.adapter.get_info()));
        }
        self.show(ui);
    }
}

/// The graphics backend and adapter, for the About window and bug reports:
/// `Vulkan · AMD Radeon 780M (radv, Mesa 25.2.4)`.
pub fn graphics(info: &eframe::wgpu::AdapterInfo) -> String {
    let driver = [info.driver.as_str(), info.driver_info.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let backend = format!("{:?}", info.backend);
    if driver.is_empty() {
        format!("{backend} · {}", info.name)
    } else {
        format!("{backend} · {} ({driver})", info.name)
    }
}

/// The icon, 128 px, made from the SVG by `packaging/icons.sh`.
const ICON_PNG: &[u8] =
    include_bytes!("../../../packaging/linux/icons/hicolor/128x128/apps/com.tryxtek.apifast.png");

/// The window's own icon, for desktops that do not take it from the menu
/// entry (X11, Windows). `None` if it cannot be read, and the window goes
/// without one.
#[must_use]
pub fn window_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(ICON_PNG).ok()
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn the_window_icon_is_a_128_px_image() {
        let icon = super::window_icon();
        assert_eq!(icon.map(|i| (i.width, i.height)), Some((128, 128)));
    }
}
