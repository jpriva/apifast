//! The application: reads the core's latest snapshot every frame, draws the
//! screen that fits it and sends the user's commands back.

use std::sync::mpsc;

use apifast_storage_fs::{FsKeymapStore, FsSettingsStore};
use eframe::egui;

use crate::actions::Keymap;
use crate::core::{Command, Core};
use crate::screens::{MainScreen, Welcome};
use crate::settings::Settings;
use crate::strings::{self, Strings};

/// The whole interface.
pub struct App {
    core: Core,
    strings: &'static Strings,
    welcome: Welcome,
    main: MainScreen,
    /// Where the keymap is kept, if anywhere.
    keymap_store: Option<FsKeymapStore>,
    /// Where the settings are kept, if anywhere.
    settings_store: Option<FsSettingsStore>,
    /// Results of saves, which run away from the interface.
    saved: (mpsc::Sender<SaveResult>, mpsc::Receiver<SaveResult>),
}

/// How saving went: an error ready to show.
type SaveResult = Result<(), String>;

impl App {
    /// The interface over `core`.
    pub fn new(core: Core) -> Self {
        Self {
            core,
            strings: &strings::EN,
            welcome: Welcome::default(),
            main: MainScreen::default(),
            keymap_store: None,
            settings_store: None,
            saved: mpsc::channel(),
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
                        self.strings.keymap_problems,
                        problems.join(", ")
                    ));
                }
            }
            Err(e) => self.main.notice = Some(e.to_string()),
        }
        self.keymap_store = Some(store);
        self
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
                        self.strings.settings_problems,
                        problems.join(", ")
                    ));
                }
            }
            Err(e) => self.main.notice = Some(e.to_string()),
        }
        self.configure_core();
        self.settings_store = Some(store);
        self
    }

    fn configure_core(&self) {
        let settings = &self.main.settings;
        self.core.send(Command::Configure {
            history: settings.history_limits(),
            responses: settings.response_limits(),
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
        self.main.apply_appearance(ui.ctx());
        let snapshot = self.core.snapshot();
        if snapshot.collection.is_some() {
            for command in self.main.show(ui, &snapshot, self.strings) {
                self.core.send(command);
            }
        } else if let Some(command) = self
            .welcome
            .show(ui, snapshot.error.as_deref(), self.strings)
        {
            self.core.send(command);
        }
        let ctx = ui.ctx().clone();
        self.save(&ctx);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
