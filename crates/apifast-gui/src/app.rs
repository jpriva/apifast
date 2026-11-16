//! The application: reads the core's latest snapshot every frame, draws the
//! screen that fits it and sends the user's commands back.

use std::sync::mpsc;

use apifast_storage_fs::FsKeymapStore;
use eframe::egui;

use crate::actions::Keymap;
use crate::core::Core;
use crate::screens::{MainScreen, Welcome};
use crate::strings::{self, Strings};

/// The whole interface.
pub struct App {
    core: Core,
    strings: &'static Strings,
    welcome: Welcome,
    main: MainScreen,
    /// Where the keymap is kept, if anywhere.
    keymap_store: Option<FsKeymapStore>,
    /// Results of keymap saves, which run away from the interface.
    saved: (mpsc::Sender<SaveResult>, mpsc::Receiver<SaveResult>),
}

/// How saving the keymap went: an error ready to show.
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
                    self.main.keymap_notice = Some(format!(
                        "{} {}",
                        self.strings.keymap_problems,
                        problems.join(", ")
                    ));
                }
            }
            Err(e) => self.main.keymap_notice = Some(e.to_string()),
        }
        self.keymap_store = Some(store);
        self
    }

    /// Saves the keymap if it changed, on a thread of its own.
    fn save_keymap(&mut self) {
        if !self.main.take_keymap_change() {
            return;
        }
        let Some(store) = self.keymap_store.clone() else {
            return;
        };
        let entries = self.main.keymap.entries();
        let done = self.saved.0.clone();
        std::thread::spawn(move || {
            let _ = done.send(store.save(&entries).map_err(|e| e.to_string()));
        });
    }

    /// The core, to send it commands from outside the interface, such as
    /// a folder given on the command line.
    pub fn core(&self) -> &Core {
        &self.core
    }

    /// Draws one frame.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        while let Ok(result) = self.saved.1.try_recv() {
            self.main.keymap_notice = result.err();
        }
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
        self.save_keymap();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
