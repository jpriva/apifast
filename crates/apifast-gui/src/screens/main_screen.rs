//! The main screen: collection tree, environment, URL bar and response.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use crate::core::{Command, ResponseState, Snapshot};
use crate::layout;
use crate::molecules::EnvironmentSelector;
use crate::organisms::{ResponseView, TreeEvent, TreeView, UrlBar, UrlBarEvent};
use crate::strings::Strings;
use crate::theme;

const SEND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const CANCEL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);

/// The state the main screen keeps between frames.
#[derive(Debug, Default)]
pub struct MainScreen {
    tree: TreeView,
    response: ResponseView,
}

impl MainScreen {
    /// Draws `snapshot` and returns the commands the user asked for.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        strings: &Strings,
    ) -> Vec<Command> {
        let Some(collection) = &snapshot.collection else {
            return Vec::new();
        };
        let sending = snapshot.response == ResponseState::Sending;
        let selected = snapshot.request.as_ref().map(|r| &r.path);
        let (tree, response) = (&mut self.tree, &mut self.response);

        let (environment, tree_event, url_event) = layout::workbench(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&collection.tree.collection.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        EnvironmentSelector::new(
                            &snapshot.environments,
                            snapshot.environment.as_deref(),
                            strings.no_environment,
                            strings.environment,
                        )
                        .show(ui)
                    })
                    .inner
                })
                .inner
            },
            |ui| tree.show(ui, &collection.tree, snapshot.version, selected, strings),
            |ui| {
                if let Some(error) = &snapshot.error {
                    ui.label(RichText::new(error.as_str()).color(theme::palette(ui).danger));
                    ui.add_space(theme::space::M);
                }
                let Some(open) = &snapshot.request else {
                    let muted = theme::palette(ui).text_muted;
                    ui.label(RichText::new(strings.no_request).color(muted));
                    return None;
                };
                let event =
                    UrlBar::new(&open.request.method, &open.request.url, sending, strings).show(ui);
                ui.separator();
                response.show(ui, &snapshot.response, strings);
                event
            },
        );

        let mut commands = Vec::new();
        if let Some(choice) = environment {
            commands.push(Command::SelectEnvironment(choice));
        }
        if let Some(event) = tree_event {
            commands.push(match event {
                TreeEvent::RequestSelected(path) => Command::OpenRequest(path),
                TreeEvent::CreateRequest { folder, name } => {
                    Command::CreateRequest { folder, name }
                }
                TreeEvent::CreateFolder { parent, name } => Command::CreateFolder { parent, name },
                TreeEvent::Rename { item, name } => Command::Rename { item, name },
                TreeEvent::Delete(item) => Command::Delete(item),
                TreeEvent::Move { item, placement } => Command::Move { item, placement },
            });
        }
        let url_event = url_event.or_else(|| shortcut(ui, snapshot, sending));
        match url_event {
            Some(UrlBarEvent::Send) => commands.push(Command::Send),
            Some(UrlBarEvent::Cancel) => commands.push(Command::Cancel),
            None => {}
        }
        commands
    }
}

/// `Ctrl+Enter` sends the open request and `Esc` cancels the send.
fn shortcut(ui: &egui::Ui, snapshot: &Snapshot, sending: bool) -> Option<UrlBarEvent> {
    snapshot.request.as_ref()?;
    ui.input_mut(|i| {
        if sending {
            i.consume_shortcut(&CANCEL).then_some(UrlBarEvent::Cancel)
        } else {
            i.consume_shortcut(&SEND).then_some(UrlBarEvent::Send)
        }
    })
}
