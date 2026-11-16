//! Component gallery: every component in every state, in both themes side by
//! side. Run with `cargo run -p apifast-gui --example gallery`.
//!
//! Every new component is added here.

use apifast_app::{
    CollectionTree, HistoryEntry, RequestPath, ResponseHead, ResponseId, TimelineEvent,
    TimelineKind, TreeItem,
};
use apifast_domain::{Body, Collection, Folder, Header, HttpRequest, Method};
use apifast_gui::atoms::{VariableHint, VariableHints};
use apifast_gui::body::BodyHandle;
use apifast_gui::core::{Received, ResponseState, Tab, TabContext};
use apifast_gui::molecules::EnvironmentSelector;
use apifast_gui::organisms::{HistoryView, RequestPanel, ResponseView, TabBar, TreeView, UrlBar};
use apifast_gui::strings::EN;
use apifast_gui::{atoms, theme};
use eframe::egui::{self, Theme};

/// The sample data, and one copy of each stateful component per theme.
struct Gallery {
    tree: CollectionTree,
    environments: Vec<String>,
    responses: Vec<ResponseState>,
    tabs: Vec<Tab>,
    hints: VariableHints,
    columns: [Column; 2],
}

struct Column {
    tree: TreeView,
    tab_bar: TabBar,
    environment: Option<String>,
    urls: [(Method, String); 2],
    request: HttpRequest,
    request_panel: RequestPanel,
    responses: Vec<ResponseView>,
    history: HistoryView,
}

impl Default for Column {
    fn default() -> Self {
        Self {
            tree: TreeView::default(),
            tab_bar: TabBar::default(),
            environment: None,
            urls: [
                (Method::Get, "{{host}}/users?page=2".into()),
                (
                    Method::Post,
                    "{{host}}/users/{{user_id}}?t={{$timestamp}}".into(),
                ),
            ],
            request: sample_request(),
            request_panel: RequestPanel::default(),
            responses: Vec::new(),
            history: HistoryView::default(),
        }
    }
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let Self {
            tree,
            environments,
            responses,
            tabs,
            hints,
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
                            showcase(ui, tree, environments, responses, tabs, hints, state);
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
    tabs: &[Tab],
    hints: &VariableHints,
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
    section(ui, "ColorSwatch", |ui| {
        ui.horizontal(|ui| {
            for color in [Some("#22aa55"), Some("#d9534f"), Some("not a color"), None] {
                ui.add(atoms::ColorSwatch::new(color));
                ui.label(color.unwrap_or("none"));
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
        for ((method, url), sending) in state.urls.iter_mut().zip([false, true]) {
            let _ = UrlBar::new(method, url, hints, sending, &EN).show(ui);
        }
    });
    section(ui, "RequestPanel", |ui| {
        ui.allocate_ui(egui::vec2(ui.available_width(), 260.0), |ui| {
            let _ = state.request_panel.show(
                ui,
                &mut state.request,
                &TabContext::default(),
                hints,
                &EN,
            );
        });
    });
    section(ui, "TabBar", |ui| {
        let _ = state.tab_bar.show(ui, tabs, Some(&tabs[1].path), &EN);
    });
    section(ui, "TreeView", |ui| {
        ui.allocate_ui(egui::vec2(ui.available_width(), 160.0), |ui| {
            let _ = state
                .tree
                .show(ui, tree, 0, Some(&RequestPath::new("login.yml")), &EN);
        });
    });
    section(ui, "HistoryView", |ui| {
        ui.allocate_ui(egui::vec2(ui.available_width(), 120.0), |ui| {
            let _ = state.history.show(ui, &sample_history(), 10_000_000, &EN);
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

fn sample_history() -> Vec<HistoryEntry> {
    let head = ResponseHead {
        status: 201,
        version: "HTTP/1.1".into(),
        url: "http://localhost:8080/users".into(),
        redirects: 0,
        headers: Vec::new(),
        elapsed_ms: 182,
        timeline: Vec::new(),
    };
    vec![
        HistoryEntry::failed(
            10_000_000 - 7_200_000,
            RequestPath::new("health.yml"),
            ("GET".into(), "{{host}}/health".into()),
            "cannot connect to `localhost:8080`".into(),
        ),
        HistoryEntry::received(
            10_000_000 - 65_000,
            RequestPath::new("users/create.yml"),
            ("POST".into(), "{{host}}/users".into()),
            head,
            (64, ResponseId("sample".into())),
            &[],
        ),
    ]
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

fn sample_request() -> HttpRequest {
    let mut request = HttpRequest::new(
        "Create user",
        Method::Post,
        "{{host}}/users/:id?notify=true",
    );
    request.headers.push(Header::new("x-trace", "{{$uuid}}"));
    // On one line, so the Format button has something to do.
    request.body = Body::Json(
        r#"{"name": "{{name}}", "age": 30, "admin": false, "id": {{user_id}}, "tags": []}"#.into(),
    );
    apifast_domain::sync_params_from_url(&mut request);
    request
}

/// `host` is defined; `user_id` is not.
fn sample_hints() -> VariableHints {
    VariableHints::from([(
        "host".to_owned(),
        VariableHint {
            value: "http://localhost:8080".into(),
            origin: "Environment local".into(),
        },
    )])
}

fn sample_tabs() -> Vec<Tab> {
    let tab = |path: &str, name: &str, method, dirty: bool| {
        let saved = HttpRequest::new(name, method, "{{host}}");
        let draft = dirty.then(|| HttpRequest::new(name, Method::Put, "{{host}}/x"));
        Tab {
            path: RequestPath::new(path),
            saved,
            draft,
            response: ResponseState::Idle,
            revision: 0,
            context: std::sync::Arc::default(),
            conflict: false,
        }
    };
    vec![
        tab("login.yml", "Login", Method::Post, false),
        tab("users/create.yml", "Create user", Method::Post, true),
        tab("users/list.yml", "List users", Method::Get, false),
    ]
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
                timeline: vec![
                    TimelineEvent {
                        at_us: 0,
                        kind: TimelineKind::Request {
                            method: "GET".into(),
                            url: "http://localhost:8080/users".into(),
                        },
                    },
                    TimelineEvent {
                        at_us: 151_400,
                        kind: TimelineKind::Headers { status },
                    },
                    TimelineEvent {
                        at_us: 182_000,
                        kind: TimelineKind::Body {
                            bytes: preview.len() as u64,
                        },
                    },
                ],
            },
            size: preview.len() as u64,
            body: BodyHandle::memory(format!("sample-{status}"), preview),
            checks: apifast_gui::core::Checks::default(),
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
                tabs: sample_tabs(),
                hints: sample_hints(),
                columns: Default::default(),
            }))
        }),
    )
}
