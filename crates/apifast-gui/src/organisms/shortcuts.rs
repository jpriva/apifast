//! The editor of keyboard shortcuts: every action with its shortcuts, a
//! button that waits for a new one, and one that puts back the default.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use crate::actions::{ALL, Action, Keymap};
use crate::strings::Strings;
use crate::theme;

/// The editor's state.
#[derive(Debug, Default)]
pub struct ShortcutEditor {
    /// Whether the window is open.
    pub open: bool,
    /// The action waiting for its new shortcut.
    capturing: Option<Action>,
    /// What the last change did to another action, to tell the user.
    note: Option<String>,
}

/// The shortcut of a key pressed with `modifiers`, as the keymap writes it.
pub fn shortcut_of(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    let mut normal = Modifiers::NONE;
    if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
        normal = normal.plus(Modifiers::COMMAND);
    }
    if modifiers.shift {
        normal = normal.plus(Modifiers::SHIFT);
    }
    if modifiers.alt {
        normal = normal.plus(Modifiers::ALT);
    }
    KeyboardShortcut::new(normal, key)
}

impl ShortcutEditor {
    /// Whether it is waiting for a key, so nothing else should take keys.
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// Starts waiting for the new shortcut of `action`.
    pub fn capture(&mut self, action: Action) {
        self.capturing = Some(action);
        self.note = None;
    }

    /// Draws the window, if open, changing `keymap`. Returns whether it
    /// changed.
    pub fn show(&mut self, ctx: &egui::Context, keymap: &mut Keymap, strings: &Strings) -> bool {
        if !self.open {
            self.capturing = None;
            return false;
        }
        let mut changed = self.take_key(ctx, keymap, strings);
        let mut open = self.open;
        crate::molecules::window(
            ctx,
            egui::Window::new(strings.shortcuts_title)
                .id(egui::Id::unique("shortcut-editor"))
                .default_size([520.0, 480.0]),
            (strings.shortcuts_title, strings.close_window),
            &mut open,
            |ui| {
                let muted = theme::palette(ui).text_muted;
                if let Some(note) = &self.note {
                    ui.label(RichText::new(note.as_str()).color(theme::palette(ui).warning));
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("shortcut-grid")
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for &action in ALL {
                                    ui.label(action.label(strings));
                                    let text = if self.capturing == Some(action) {
                                        RichText::new(strings.shortcut_press)
                                            .color(theme::palette(ui).accent)
                                    } else {
                                        let shortcuts: Vec<String> = keymap
                                            .shortcuts(action)
                                            .iter()
                                            .map(|s| ctx.format_shortcut(s))
                                            .collect();
                                        if shortcuts.is_empty() {
                                            RichText::new(strings.shortcut_none).color(muted)
                                        } else {
                                            RichText::new(shortcuts.join(", ")).monospace()
                                        }
                                    };
                                    ui.label(text);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .button(strings.shortcut_change)
                                            .on_hover_text(action.label(strings))
                                            .clicked()
                                        {
                                            self.capture(action);
                                        }
                                        let default = keymap.shortcuts(action) == action.defaults();
                                        if ui
                                            .add_enabled(
                                                !default,
                                                egui::Button::new(strings.shortcut_reset),
                                            )
                                            .clicked()
                                        {
                                            keymap.reset(action);
                                            changed = true;
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
            },
        );
        self.open = open;
        changed
    }

    /// While waiting, takes the next key pressed as the shortcut. Escape
    /// alone stops waiting.
    fn take_key(&mut self, ctx: &egui::Context, keymap: &mut Keymap, strings: &Strings) -> bool {
        let Some(action) = self.capturing else {
            return false;
        };
        let pressed = ctx.input_mut(|i| {
            let found = i.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*modifiers, *key)),
                _ => None,
            });
            if found.is_some() {
                // Nothing else acts on this key.
                i.events
                    .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            }
            found
        });
        let Some((modifiers, key)) = pressed else {
            return false;
        };
        self.capturing = None;
        if key == Key::Escape && modifiers.is_none() {
            return false;
        }
        if let Some(previous) = keymap.bind(action, shortcut_of(modifiers, key)) {
            self.note = Some(format!(
                "{} {}.",
                strings.shortcut_moved,
                previous.label(strings)
            ));
        }
        true
    }
}
