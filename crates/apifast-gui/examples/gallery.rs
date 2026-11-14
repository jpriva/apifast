//! Component gallery: every component in every state, in both themes side by
//! side. Run with `cargo run -p apifast-gui --example gallery`.
//!
//! Every new component is added here.

use apifast_domain::Method;
use apifast_gui::{atoms, theme};
use eframe::egui::{self, Theme};

struct Gallery;

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.columns(2, |columns| {
                for (column, theme) in columns.iter_mut().zip([Theme::Light, Theme::Dark]) {
                    themed(column, theme, showcase);
                }
            });
        });
    }
}

/// Draws `content` with `theme`, whatever the window's theme is.
fn themed(ui: &mut egui::Ui, theme: Theme, content: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        // `visuals_mut` changes only this scope. (`ui.set_visuals` would
        // reach `Context::set_visuals` through `Deref` and change the whole
        // app from the next frame.)
        *ui.visuals_mut() = theme::visuals(theme);
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(theme::space::XL)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                content(ui);
            });
    });
}

fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.label(
        egui::RichText::new(title)
            .size(theme::text::HEADING)
            .strong(),
    );
    ui.add_space(theme::space::S);
    content(ui);
    ui.add_space(theme::space::L);
}

fn showcase(ui: &mut egui::Ui) {
    section(ui, "MethodBadge", |ui| {
        let methods = [
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Patch,
            Method::Delete,
            Method::Head,
            Method::Options,
            Method::Custom("PURGE".into()),
            Method::Custom("PROPFIND-VERY-LONG".into()),
        ];
        for method in &methods {
            ui.horizontal(|ui| {
                ui.add(atoms::MethodBadge::new(method));
                ui.label("request name");
            });
        }
    });
    section(ui, "StatusBadge", |ui| {
        ui.horizontal_wrapped(|ui| {
            for status in [
                101, 200, 201, 204, 301, 304, 400, 401, 404, 418, 422, 500, 503, 599,
            ] {
                ui.add(atoms::StatusBadge::new(status));
            }
        });
    });
}

fn main() -> eframe::Result {
    eframe::run_native(
        "apifast gallery",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([900.0, 640.0])
                .with_title("apifast — component gallery"),
            ..Default::default()
        },
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(Gallery))
        }),
    )
}
