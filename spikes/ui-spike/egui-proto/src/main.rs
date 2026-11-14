//! egui prototype: virtualized 5000-row tree + 1 MB JSON editor with highlighting.
//! Run with SPIKE_SCENARIO=1 to execute the scripted measurement and exit.
//! Backend: SPIKE_BACKEND=wgpu | glow.

use common::{Action, Scenario, Tok, TreeRow, ROW_HEIGHT};
use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct App {
    scenario: Scenario,
    rows: Vec<TreeRow>,
    text: String,
    insert_at: usize,
    tree_offset: Option<f32>,
    editor_offset: Option<f32>,
    tree_y: f32,
    editor_y: f32,
    cache: Option<(u64, Arc<egui::Galley>)>,
}

fn text_hash(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn layout_job(text: &str) -> LayoutJob {
    let font = FontId::monospace(13.0);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut spans = Vec::new();
    common::tokenize(text, &mut spans);
    for (range, tok) in spans {
        let [r, g, b] = common::color(tok);
        job.append(&text[range], 0.0, TextFormat::simple(font.clone(), Color32::from_rgb(r, g, b)));
    }
    job
}

fn method_color(m: &str) -> Color32 {
    match m {
        "GET" => Color32::from_rgb(100, 200, 120),
        "POST" => Color32::from_rgb(240, 180, 80),
        "DELETE" => Color32::from_rgb(240, 100, 100),
        _ => Color32::from_rgb(120, 170, 255),
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        match self.scenario.next() {
            Action::Wait => ctx.request_repaint(),
            Action::Type => {
                self.text.insert(self.insert_at, 'x');
                ctx.request_repaint();
            }
            Action::ScrollTree(px) => {
                self.tree_offset = Some(self.tree_y + px);
                ctx.request_repaint();
            }
            Action::ScrollEditor(px) => {
                self.editor_offset = Some(self.editor_y + px);
                ctx.request_repaint();
            }
            Action::Idle => {
                if std::env::var("SPIKE_DEBUG_REPAINT").is_ok() {
                    eprintln!("idle frame, causes: {:?}", ctx.repaint_causes());
                }
                if self.scenario.enabled {
                    ctx.request_repaint_after(Duration::from_millis(5100));
                }
            }
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }

        if self.scenario.is_empty_mode() {
            egui::CentralPanel::default().show(ui, |ui| ui.label("apifast spike (empty)"));
            return;
        }

        egui::Panel::left("tree").exact_size(280.0).show(ui, |ui| {
            let mut area = egui::ScrollArea::vertical().id_salt("tree").auto_shrink(false);
            if let Some(y) = self.tree_offset.take() {
                area = area.vertical_scroll_offset(y);
            }
            let out = area.show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
                for r in &self.rows[range] {
                    ui.allocate_ui(egui::vec2(ui.available_width(), ROW_HEIGHT), |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(12.0 * f32::from(r.depth) + 4.0);
                            if r.method.is_empty() {
                                ui.label(format!("▾ {}", r.label));
                            } else {
                                ui.label(egui::RichText::new(r.method).monospace().size(12.0).color(method_color(r.method)));
                                ui.label(&r.label);
                            }
                        });
                    });
                }
            });
            self.tree_y = out.state.offset().y;
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let mut area = egui::ScrollArea::both().id_salt("editor").auto_shrink(false);
            if let Some(y) = self.editor_offset.take() {
                area = area.vertical_scroll_offset(y);
            }
            let cache = &mut self.cache;
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let s = buf.as_str();
                let h = text_hash(s);
                if let Some((ch, g)) = cache.as_ref() {
                    if *ch == h {
                        return g.clone();
                    }
                }
                let g = ui.ctx().fonts_mut(|f| f.layout_job(layout_job(s)));
                *cache = Some((h, g.clone()));
                g
            };
            let out = area.show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
            });
            self.editor_y = out.state.offset().y;
        });
    }
}

fn main() -> eframe::Result {
    let started = Instant::now();
    let scenario = Scenario::new("egui", started);
    let empty = scenario.is_empty_mode();
    let renderer = match scenario.backend.as_str() {
        "glow" => eframe::Renderer::Glow,
        _ => eframe::Renderer::Wgpu,
    };
    let text = if empty { String::new() } else { common::big_json() };
    // Middle of the text, on an ASCII boundary (the generated JSON is ASCII).
    let insert_at = text.len() / 2;
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title("apifast spike — egui"),
        ..Default::default()
    };
    eframe::run_native(
        "apifast-spike-egui",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App {
                scenario,
                rows: if empty { Vec::new() } else { common::tree_rows() },
                text,
                insert_at,
                tree_offset: None,
                editor_offset: None,
                tree_y: 0.0,
                editor_y: 0.0,
                cache: None,
            }))
        }),
    )
}

#[allow(dead_code)]
fn _assert_tok_used(_: Tok) {}
