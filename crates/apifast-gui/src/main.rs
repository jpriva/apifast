//! The `apifast-gui` binary.

use eframe::egui;

struct App;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("apifast");
        });
    }
}

fn main() -> eframe::Result {
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
        Box::new(|cc| {
            apifast_gui::theme::install(&cc.egui_ctx);
            Ok(Box::new(App))
        }),
    )
}
