//! Component gallery: every component in every state, in both themes side by
//! side. Run with `cargo run -p apifast-gui --example gallery`.
//!
//! Every new component is added here.

use apifast_app::{CollectionTree, RequestPath, ResponseHead, TreeItem};
use apifast_domain::{Collection, Folder, Method};
use apifast_gui::core::{Received, ResponseState};
use apifast_gui::molecules::EnvironmentSelector;
use apifast_gui::organisms::{ResponseView, TreeView, UrlBar};
use apifast_gui::strings::EN;
use apifast_gui::{atoms, theme};
use eframe::egui::{self, Theme};

/// The sample data, and one copy of each stateful component per theme.
struct Gallery {
    tree: CollectionTree,
    environments: Vec<String>,
    responses: Vec<ResponseState>,
    columns: [Column; 2],
}

#[derive(Default)]
struct Column {
    tree: TreeView,
    environment: Option<String>,
    responses: Vec<ResponseView>,
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let Self {
            tree,
            environments,
            responses,
            columns: state,
        } = self;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.columns(2, |columns| {
                    for ((column, theme), state) in columns
                        .iter_mut()
                        .zip([Theme::Light, Theme::Dark])
                        .zip(state.iter_mut())
                    {
                        themed(column, theme, |ui| {
                            showcase(ui, tree, environments, responses, state);
                        });
                    }
                });
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

fn showcase(
    ui: &mut egui::Ui,
    tree: &CollectionTree,
    environments: &[String],
    responses: &[ResponseState],
    state: &mut Column,
) {
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
    section(ui, "EnvironmentSelector", |ui| {
        let active = state.environment.as_deref();
        if let Some(choice) =
            EnvironmentSelector::new(environments, active, EN.no_environment, EN.environment)
                .show(ui)
        {
            state.environment = choice;
        }
    });
    section(ui, "UrlBar", |ui| {
        let _ = UrlBar::new(&Method::Get, "{{host}}/users?page=2", false, &EN).show(ui);
        let _ = UrlBar::new(&Method::Post, "{{host}}/users", true, &EN).show(ui);
    });
    section(ui, "TreeView", |ui| {
        ui.allocate_ui(egui::vec2(ui.available_width(), 160.0), |ui| {
            let _ = state
                .tree
                .show(ui, tree, 0, Some(&RequestPath::new("login.yml")), &EN);
        });
    });
    section(ui, "ResponseView", |ui| {
        state
            .responses
            .resize_with(responses.len(), ResponseView::default);
        for (response, view) in responses.iter().zip(&mut state.responses) {
            ui.push_id(std::ptr::from_ref(response), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_max_height(140.0);
                    view.show(ui, response, &EN);
                });
            });
        }
    });
}

fn sample_tree() -> CollectionTree {
    let request = |path: &str, name: &str, method| TreeItem::Request {
        path: RequestPath::new(path),
        name: name.into(),
        method,
        seq: None,
    };
    CollectionTree {
        collection: Collection::new("Shop API"),
        items: vec![
            TreeItem::Folder {
                path: "users".into(),
                folder: Folder::new("Users"),
                children: vec![
                    request("users/list.yml", "List users", Method::Get),
                    request("users/create.yml", "Create user", Method::Post),
                    request("users/delete.yml", "Delete user", Method::Delete),
                ],
            },
            request("login.yml", "Login", Method::Post),
            TreeItem::Other {
                path: RequestPath::new("search.yml"),
                name: "Search".into(),
                kind: "graphql".into(),
                seq: None,
            },
            TreeItem::Invalid {
                path: RequestPath::new("broken.yml"),
                message: "line 3: expected a mapping".into(),
            },
        ],
    }
}

fn sample_responses() -> Vec<ResponseState> {
    let received = |status, preview: &str| {
        ResponseState::Received(Received {
            head: ResponseHead {
                status,
                version: "HTTP/1.1".into(),
                url: "http://localhost:8080/users".into(),
                redirects: 0,
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("content-length".into(), preview.len().to_string()),
                ],
                elapsed_ms: 182,
            },
            size: preview.len() as u64,
            preview: preview.into(),
            truncated: false,
        })
    };
    vec![
        ResponseState::Idle,
        ResponseState::Sending,
        received(201, "{ \"id\": 981, \"name\": \"Ana Ruiz\" }"),
        received(404, "{ \"error\": \"not found\" }"),
        ResponseState::Failed("cannot connect to 127.0.0.1:8080".into()),
        ResponseState::Cancelled,
    ]
}

fn main() -> eframe::Result {
    eframe::run_native(
        "apifast gallery",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 800.0])
                .with_title("apifast — component gallery"),
            ..Default::default()
        },
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(Gallery {
                tree: sample_tree(),
                environments: vec!["local".into(), "staging".into(), "production".into()],
                responses: sample_responses(),
                columns: Default::default(),
            }))
        }),
    )
}
