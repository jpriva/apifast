//! The `apifast-gui` binary. `apifast-gui <folder>` opens that collection.

use std::path::PathBuf;

use apifast_gui::app::App;
use apifast_gui::core::{Command, Core};
use eframe::egui;

fn main() -> eframe::Result {
    let folder = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("apifast"),
        ..Default::default()
    };
    eframe::run_native(
        "apifast",
        options,
        Box::new(move |cc| {
            apifast_gui::theme::install(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let core = Core::start(apifast_history::dirs::data_dir(), move || {
                ctx.request_repaint();
            });
            if let Some(folder) = folder {
                core.send(Command::OpenCollection(folder));
            }
            Ok(Box::new(App::new(core)))
        }),
    )
}
