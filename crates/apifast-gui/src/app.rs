//! The application: reads the core's latest snapshot every frame, draws the
//! screen that fits it and sends the user's commands back.

use eframe::egui;

use crate::core::Core;
use crate::screens::{MainScreen, Welcome};
use crate::strings::{self, Strings};

/// The whole interface.
pub struct App {
    core: Core,
    strings: &'static Strings,
    welcome: Welcome,
    main: MainScreen,
}

impl App {
    /// The interface over `core`.
    pub fn new(core: Core) -> Self {
        Self {
            core,
            strings: &strings::EN,
            welcome: Welcome::default(),
            main: MainScreen::default(),
        }
    }

    /// The core, to send it commands from outside the interface, such as
    /// a folder given on the command line.
    pub fn core(&self) -> &Core {
        &self.core
    }

    /// Draws one frame.
    pub fn show(&mut self, ui: &mut egui::Ui) {
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
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
