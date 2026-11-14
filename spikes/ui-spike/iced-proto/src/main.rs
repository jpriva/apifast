//! iced prototype: virtualized 5000-row tree + 1 MB JSON editor with highlighting.
//! Run with SPIKE_SCENARIO=1 to execute the scripted measurement and exit.
//! Backend: ICED_BACKEND=wgpu | tiny-skia.

use common::{Action, Scenario, Tok, TreeRow, ROW_HEIGHT};
use iced::widget::{column, container, row, scrollable, space, text, text_editor};
use iced::{Color, Element, Font, Length, Subscription, Task, Theme};
use std::ops::Range;
use std::time::{Duration, Instant};

const TREE_ID: &str = "tree";

struct App {
    scenario: Scenario,
    rows: Vec<TreeRow>,
    content: text_editor::Content,
    tree_offset: f32,
    tree_height: f32,
    idle: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Frame(Instant),
    Edit(text_editor::Action),
    TreeScrolled(f32, f32),
    IdleTick(Instant),
}

fn boot() -> App {
    let started = *START.get().expect("start");
    let scenario = Scenario::new("iced", started);
    let empty = scenario.is_empty_mode();
    let mut content = text_editor::Content::with_text(if empty { "" } else { BIG.get().map(String::as_str).unwrap_or("") });
    if !empty {
        // Put the cursor in the middle of the document, as in the egui prototype.
        for _ in 0..content.line_count() / 2 {
            content.perform(text_editor::Action::Move(text_editor::Motion::Down));
        }
    }
    App {
        scenario,
        rows: if empty { Vec::new() } else { common::tree_rows() },
        content,
        tree_offset: 0.0,
        tree_height: 800.0,
        idle: false,
    }
}

fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::Frame(_) => match app.scenario.next() {
            Action::Wait => Task::none(),
            Action::Type => {
                app.content.perform(text_editor::Action::Edit(text_editor::Edit::Insert('x')));
                Task::none()
            }
            Action::ScrollTree(px) => iced::widget::operation::scroll_by(
                TREE_ID,
                scrollable::AbsoluteOffset { x: 0.0, y: px },
            ),
            Action::ScrollEditor(_) => {
                app.content.perform(text_editor::Action::Scroll { lines: 2 });
                Task::none()
            }
            Action::Idle => {
                app.idle = true;
                Task::none()
            }
            Action::Exit => iced::exit(),
        },
        Message::IdleTick(_) => {
            if app.scenario.idle_deadline_passed() {
                app.scenario.finish_idle();
                return iced::exit();
            }
            Task::none()
        }
        Message::Edit(a) => {
            app.content.perform(a);
            Task::none()
        }
        Message::TreeScrolled(y, h) => {
            app.tree_offset = y;
            app.tree_height = h;
            Task::none()
        }
    }
}

fn method_color(m: &str) -> Color {
    match m {
        "GET" => Color::from_rgb8(100, 200, 120),
        "POST" => Color::from_rgb8(240, 180, 80),
        "DELETE" => Color::from_rgb8(240, 100, 100),
        _ => Color::from_rgb8(120, 170, 255),
    }
}

fn view(app: &App) -> Element<'_, Message> {
    if app.scenario.is_empty_mode() {
        return container(text("apifast spike (empty)")).padding(20).into();
    }
    // Manual virtualization: only the visible rows are built.
    let total = app.rows.len();
    let first = ((app.tree_offset / ROW_HEIGHT) as usize).min(total);
    let count = (app.tree_height / ROW_HEIGHT) as usize + 2;
    let last = (first + count).min(total);
    let mut list = column![space().height(first as f32 * ROW_HEIGHT)];
    for r in &app.rows[first..last] {
        let indent = space().width(12.0 * f32::from(r.depth) + 4.0);
        let line = if r.method.is_empty() {
            row![indent, text(format!("▾ {}", r.label)).size(13)]
        } else {
            row![
                indent,
                text(r.method).size(12).font(Font::MONOSPACE).color(method_color(r.method)).width(52),
                text(&r.label).size(13)
            ]
        };
        list = list.push(container(line).height(ROW_HEIGHT));
    }
    list = list.push(space().height((total - last) as f32 * ROW_HEIGHT));

    let tree = scrollable(list)
        .id(TREE_ID)
        .width(280)
        .height(Length::Fill)
        .on_scroll(|v| Message::TreeScrolled(v.absolute_offset().y, v.bounds().height));

    let editor = text_editor(&app.content)
        .on_action(Message::Edit)
        .font(Font::MONOSPACE)
        .size(13)
        .height(Length::Fill)
        .highlight_with::<JsonHighlighter>((), |t, _| iced::advanced::text::highlighter::Format {
            color: Some({
                let [r, g, b] = common::color(*t);
                Color::from_rgb8(r, g, b)
            }),
            font: None,
        });

    row![tree, editor].spacing(8).padding(8).into()
}

fn subscription(app: &App) -> Subscription<Message> {
    if !app.scenario.enabled {
        return Subscription::none();
    }
    if app.idle {
        iced::time::every(Duration::from_millis(5100)).map(Message::IdleTick)
    } else {
        iced::window::frames().map(Message::Frame)
    }
}

/// Line-based highlighter: iced only re-highlights lines that changed.
struct JsonHighlighter {
    line: usize,
    spans: Vec<(Range<usize>, Tok)>,
}

impl iced::advanced::text::Highlighter for JsonHighlighter {
    type Settings = ();
    type Highlight = Tok;
    type Iterator<'a> = std::vec::Drain<'a, (Range<usize>, Tok)>;

    fn new(_: &Self::Settings) -> Self {
        Self { line: 0, spans: Vec::new() }
    }
    fn update(&mut self, _: &Self::Settings) {}
    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }
    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.line += 1;
        self.spans.clear();
        common::tokenize(line, &mut self.spans);
        self.spans.drain(..)
    }
    fn current_line(&self) -> usize {
        self.line
    }
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static BIG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn main() -> iced::Result {
    let _ = START.set(Instant::now());
    let _ = BIG.set(common::big_json());
    iced::application(boot, update, view)
        .title("apifast spike — iced")
        .subscription(subscription)
        .theme(|_: &App| Theme::Dark)
        .window_size((1280.0, 800.0))
        .run()
}
