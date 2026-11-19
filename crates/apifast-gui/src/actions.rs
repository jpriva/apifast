//! Every action the keyboard and the command palette can run, their default
//! shortcuts, and the [`Keymap`] that the user can change.
//!
//! Shortcuts are written as `Ctrl+Shift+S` in `keymap.yml`. `Ctrl` means
//! the command key of each platform (`Cmd` on macOS), as in egui.

use std::collections::BTreeMap;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::strings::Strings;

/// Something the user can do from the keyboard or the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Send the active request.
    Send,
    /// Cancel its send.
    Cancel,
    /// Save the active tab.
    Save,
    /// Save every tab.
    SaveAll,
    /// Open the command palette.
    Palette,
    /// Open the palette on requests only.
    QuickOpen,
    /// Create a request at the root of the collection.
    NewRequest,
    /// Close the active tab.
    CloseTab,
    /// Open again the tab closed last.
    ReopenTab,
    /// Show the next tab.
    NextTab,
    /// Show the previous tab.
    PreviousTab,
    /// Show tab number `n`, from 1.
    GoToTab(u8),
    /// Put the cursor in the URL.
    FocusUrl,
    /// Open the environment editor.
    Environments,
    /// Show or hide the sidebar.
    ToggleSidebar,
    /// Show the history in the sidebar.
    ShowHistory,
    /// Format the body being edited.
    FormatBody,
    /// Open the editor of these shortcuts.
    Shortcuts,
    /// Follow the system's theme.
    ThemeSystem,
    /// Use the light theme.
    ThemeLight,
    /// Use the dark theme.
    ThemeDark,
    /// Open the settings.
    Settings,
    /// Open the collection key window.
    CollectionKey,
    /// Open the cookie editor.
    Cookies,
    /// Copy the active request as a cURL command.
    CopyAsCurl,
    /// Open a collection folder.
    OpenCollection,
    /// Create a collection.
    NewCollection,
}

/// Every action, in the order the editor of shortcuts lists them.
pub const ALL: &[Action] = &[
    Action::Send,
    Action::Cancel,
    Action::Save,
    Action::SaveAll,
    Action::Palette,
    Action::QuickOpen,
    Action::NewRequest,
    Action::CloseTab,
    Action::ReopenTab,
    Action::NextTab,
    Action::PreviousTab,
    Action::GoToTab(1),
    Action::GoToTab(2),
    Action::GoToTab(3),
    Action::GoToTab(4),
    Action::GoToTab(5),
    Action::GoToTab(6),
    Action::GoToTab(7),
    Action::GoToTab(8),
    Action::GoToTab(9),
    Action::FocusUrl,
    Action::Environments,
    Action::ToggleSidebar,
    Action::ShowHistory,
    Action::FormatBody,
    Action::Shortcuts,
    Action::ThemeSystem,
    Action::ThemeLight,
    Action::ThemeDark,
    Action::Settings,
    Action::CollectionKey,
    Action::Cookies,
    Action::CopyAsCurl,
    Action::OpenCollection,
    Action::NewCollection,
];

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

const fn key(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

impl Action {
    /// Its name in `keymap.yml`, which never changes.
    pub fn id(self) -> String {
        match self {
            Self::Send => "send".into(),
            Self::Cancel => "cancel".into(),
            Self::Save => "save".into(),
            Self::SaveAll => "save_all".into(),
            Self::Palette => "palette".into(),
            Self::QuickOpen => "quick_open".into(),
            Self::NewRequest => "new_request".into(),
            Self::CloseTab => "close_tab".into(),
            Self::ReopenTab => "reopen_tab".into(),
            Self::NextTab => "next_tab".into(),
            Self::PreviousTab => "previous_tab".into(),
            Self::GoToTab(n) => format!("tab_{n}"),
            Self::FocusUrl => "focus_url".into(),
            Self::Environments => "environments".into(),
            Self::ToggleSidebar => "toggle_sidebar".into(),
            Self::ShowHistory => "show_history".into(),
            Self::FormatBody => "format_body".into(),
            Self::Shortcuts => "shortcuts".into(),
            Self::ThemeSystem => "theme_system".into(),
            Self::ThemeLight => "theme_light".into(),
            Self::ThemeDark => "theme_dark".into(),
            Self::Settings => "settings".into(),
            Self::CollectionKey => "collection_key".into(),
            Self::Cookies => "cookies".into(),
            Self::CopyAsCurl => "copy_as_curl".into(),
            Self::OpenCollection => "open_collection".into(),
            Self::NewCollection => "new_collection".into(),
        }
    }

    /// The action named `id` in `keymap.yml`.
    pub fn from_id(id: &str) -> Option<Self> {
        ALL.iter().copied().find(|action| action.id() == id)
    }

    /// What it does, ready to show.
    pub fn label(self, strings: &Strings) -> String {
        let text = match self {
            Self::Send => strings.action_send,
            Self::Cancel => strings.action_cancel,
            Self::Save => strings.action_save,
            Self::SaveAll => strings.action_save_all,
            Self::Palette => strings.action_palette,
            Self::QuickOpen => strings.action_quick_open,
            Self::NewRequest => strings.action_new_request,
            Self::CloseTab => strings.action_close_tab,
            Self::ReopenTab => strings.action_reopen_tab,
            Self::NextTab => strings.action_next_tab,
            Self::PreviousTab => strings.action_previous_tab,
            Self::GoToTab(n) => return format!("{} {n}", strings.action_go_to_tab),
            Self::FocusUrl => strings.action_focus_url,
            Self::Environments => strings.action_environments,
            Self::ToggleSidebar => strings.action_toggle_sidebar,
            Self::ShowHistory => strings.action_show_history,
            Self::FormatBody => strings.action_format_body,
            Self::Shortcuts => strings.action_shortcuts,
            Self::ThemeSystem => strings.action_theme_system,
            Self::ThemeLight => strings.action_theme_light,
            Self::ThemeDark => strings.action_theme_dark,
            Self::Settings => strings.action_settings,
            Self::CollectionKey => strings.action_collection_key,
            Self::Cookies => strings.action_cookies,
            Self::CopyAsCurl => strings.copy_as_curl,
            Self::OpenCollection => strings.open_collection,
            Self::NewCollection => strings.new_collection,
        };
        text.to_owned()
    }

    /// Its shortcuts before the user changes them (see `docs/09-ui-ux.md`).
    pub fn defaults(self) -> Vec<KeyboardShortcut> {
        let one = |s| vec![s];
        match self {
            Self::Send => one(key(CTRL, Key::Enter)),
            Self::Cancel => one(key(Modifiers::NONE, Key::Escape)),
            Self::Save => one(key(CTRL, Key::S)),
            Self::SaveAll => one(key(CTRL_SHIFT, Key::S)),
            Self::Palette => one(key(CTRL, Key::K)),
            Self::QuickOpen => one(key(CTRL, Key::P)),
            Self::NewRequest => one(key(CTRL, Key::N)),
            Self::CloseTab => one(key(CTRL, Key::W)),
            Self::ReopenTab => one(key(CTRL_SHIFT, Key::T)),
            Self::NextTab => one(key(CTRL, Key::Tab)),
            Self::PreviousTab => one(key(CTRL_SHIFT, Key::Tab)),
            Self::GoToTab(n) => [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ]
            .get(usize::from(n).wrapping_sub(1))
            .map(|&k| vec![key(CTRL, k)])
            .unwrap_or_default(),
            Self::FocusUrl => one(key(CTRL, Key::L)),
            Self::Settings => one(key(CTRL, Key::Comma)),
            Self::Environments => one(key(CTRL, Key::E)),
            Self::OpenCollection => one(key(CTRL, Key::O)),
            Self::ToggleSidebar => one(key(CTRL, Key::B)),
            Self::FormatBody => one(key(Modifiers::ALT.plus(Modifiers::SHIFT), Key::F)),
            Self::ShowHistory
            | Self::Shortcuts
            | Self::ThemeSystem
            | Self::ThemeLight
            | Self::ThemeDark
            | Self::CollectionKey
            | Self::Cookies
            | Self::CopyAsCurl
            | Self::NewCollection => Vec::new(),
        }
    }
}

/// `Ctrl+Shift+S` as a shortcut. Names are matched without case; `Cmd`
/// and `Command` mean `Ctrl`, `Option` means `Alt`.
pub fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key_name, modifier_names) = parts.split_last()?;
    let mut modifiers = Modifiers::NONE;
    for name in modifier_names {
        modifiers = modifiers.plus(match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" => Modifiers::COMMAND,
            "shift" => Modifiers::SHIFT,
            "alt" | "option" => Modifiers::ALT,
            _ => return None,
        });
    }
    let key = match key_name.to_ascii_lowercase().as_str() {
        "esc" => Key::Escape,
        "return" => Key::Enter,
        "=" | "plus" => Key::Equals,
        _ => Key::from_name(key_name).or_else(|| Key::from_name(&key_name.to_ascii_uppercase()))?,
    };
    Some(KeyboardShortcut::new(modifiers, key))
}

/// A shortcut as `keymap.yml` writes it: `Ctrl+Shift+S`.
pub fn shortcut_text(shortcut: &KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let mut parts = Vec::new();
    if m.command || m.ctrl || m.mac_cmd {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(shortcut.logical_key.name());
    parts.join("+")
}

/// Whether two shortcuts are pressed the same way.
fn same(a: KeyboardShortcut, b: KeyboardShortcut) -> bool {
    a.logical_key == b.logical_key && shortcut_text(&a) == shortcut_text(&b)
}

/// The shortcuts of every action: the defaults, with the user's changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    /// Actions whose shortcuts the user changed, and what they are now.
    changed: BTreeMap<Action, Vec<KeyboardShortcut>>,
}

impl Keymap {
    /// The keymap described by `entries`, as `keymap.yml` holds them. Also
    /// returns what could not be understood, to tell the user.
    pub fn from_entries(entries: &[(String, Vec<String>)]) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut problems = Vec::new();
        for (id, texts) in entries {
            let Some(action) = Action::from_id(id) else {
                problems.push(format!("`{id}`"));
                continue;
            };
            let mut shortcuts = Vec::new();
            for text in texts {
                match parse_shortcut(text) {
                    Some(shortcut) => shortcuts.push(shortcut),
                    None => problems.push(format!("`{id}: {text}`")),
                }
            }
            keymap.set(action, shortcuts);
        }
        (keymap, problems)
    }

    /// The entries for `keymap.yml`: only the actions that differ from
    /// their defaults.
    pub fn entries(&self) -> Vec<(String, Vec<String>)> {
        self.changed
            .iter()
            .map(|(action, shortcuts)| (action.id(), shortcuts.iter().map(shortcut_text).collect()))
            .collect()
    }

    /// The shortcuts of `action`.
    pub fn shortcuts(&self, action: Action) -> Vec<KeyboardShortcut> {
        self.changed
            .get(&action)
            .cloned()
            .unwrap_or_else(|| action.defaults())
    }

    /// The first shortcut of `action`, if it has one.
    pub fn first(&self, action: Action) -> Option<KeyboardShortcut> {
        self.shortcuts(action).into_iter().next()
    }

    /// Gives `action` these shortcuts.
    pub fn set(&mut self, action: Action, shortcuts: Vec<KeyboardShortcut>) {
        if shortcuts == action.defaults() {
            self.changed.remove(&action);
        } else {
            self.changed.insert(action, shortcuts);
        }
    }

    /// Puts back the defaults of `action`.
    pub fn reset(&mut self, action: Action) {
        self.changed.remove(&action);
    }

    /// The action other than `except` that `shortcut` runs, if any.
    pub fn owner(&self, shortcut: &KeyboardShortcut, except: Action) -> Option<Action> {
        ALL.iter()
            .copied()
            .filter(|action| *action != except)
            .find(|action| self.shortcuts(*action).iter().any(|s| same(*s, *shortcut)))
    }

    /// Gives `action` only `shortcut`, taking it from the action that had
    /// it. Returns that action.
    pub fn bind(&mut self, action: Action, shortcut: KeyboardShortcut) -> Option<Action> {
        let previous = self.owner(&shortcut, action);
        if let Some(previous) = previous {
            let kept = (self.shortcuts(previous).into_iter())
                .filter(|s| !same(*s, shortcut))
                .collect();
            self.set(previous, kept);
        }
        self.set(action, vec![shortcut]);
        previous
    }

    /// The actions whose shortcuts were pressed this frame, consuming those
    /// key presses. Shortcuts with more modifiers are tried first: egui
    /// takes `Ctrl+S` as pressed when `Ctrl+Shift+S` is.
    pub fn pressed(&self, input: &mut egui::InputState, actions: &[Action]) -> Vec<Action> {
        let mut bound: Vec<(Action, KeyboardShortcut)> = actions
            .iter()
            .flat_map(|&action| self.shortcuts(action).into_iter().map(move |s| (action, s)))
            .collect();
        let weight = |s: &KeyboardShortcut| {
            let m = s.modifiers;
            u8::from(m.command || m.ctrl) + u8::from(m.shift) + u8::from(m.alt)
        };
        bound.sort_by_key(|(_, s)| std::cmp::Reverse(weight(s)));
        let mut pressed = Vec::new();
        for (action, shortcut) in bound {
            if input.consume_shortcut(&shortcut) && !pressed.contains(&action) {
                pressed.push(action);
            }
        }
        pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_read_and_written_the_same_way() {
        for text in [
            "Ctrl+Enter",
            "Ctrl+Shift+S",
            "Alt+Shift+F",
            "Escape",
            "Ctrl+1",
            "F2",
        ] {
            let shortcut = parse_shortcut(text).unwrap();
            assert_eq!(shortcut_text(&shortcut), text);
        }
        assert_eq!(
            parse_shortcut("cmd + shift + s"),
            parse_shortcut("Ctrl+Shift+S")
        );
        assert_eq!(shortcut_text(&parse_shortcut("esc").unwrap()), "Escape");
        for bad in ["", "Ctrl+", "Hyper+S", "Ctrl+Nope"] {
            assert_eq!(parse_shortcut(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_action_has_a_stable_unique_id_and_its_defaults_differ() {
        let mut ids: Vec<String> = ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
        for action in ALL {
            assert_eq!(Action::from_id(&action.id()), Some(*action));
        }
        let keymap = Keymap::default();
        for action in ALL {
            for shortcut in action.defaults() {
                assert_eq!(keymap.owner(&shortcut, *action), None, "{action:?}");
            }
        }
    }

    #[test]
    fn the_file_holds_only_what_changed() {
        let mut keymap = Keymap::default();
        assert_eq!(keymap.entries(), []);
        keymap.set(Action::Send, vec![parse_shortcut("Ctrl+R").unwrap()]);
        keymap.set(Action::ReopenTab, Vec::new());
        assert_eq!(
            keymap.entries(),
            [
                ("send".to_owned(), vec!["Ctrl+R".to_owned()]),
                ("reopen_tab".to_owned(), vec![])
            ]
        );
        let (read, problems) = Keymap::from_entries(&keymap.entries());
        assert_eq!(read, keymap);
        assert_eq!(problems, Vec::<String>::new());
        keymap.reset(Action::Send);
        keymap.set(Action::ReopenTab, Action::ReopenTab.defaults());
        assert_eq!(keymap.entries(), []);
    }

    #[test]
    fn unknown_actions_and_shortcuts_are_reported() {
        let entries = [
            ("nope".to_owned(), vec!["Ctrl+A".to_owned()]),
            (
                "send".to_owned(),
                vec!["Ctrl+R".to_owned(), "Bad+X".to_owned()],
            ),
        ];
        let (keymap, problems) = Keymap::from_entries(&entries);
        assert_eq!(problems, ["`nope`", "`send: Bad+X`"]);
        assert_eq!(
            keymap.shortcuts(Action::Send),
            [parse_shortcut("Ctrl+R").unwrap()]
        );
    }

    #[test]
    fn binding_a_taken_shortcut_takes_it_from_its_action() {
        let mut keymap = Keymap::default();
        let ctrl_s = parse_shortcut("Ctrl+S").unwrap();
        assert_eq!(keymap.bind(Action::Send, ctrl_s), Some(Action::Save));
        assert_eq!(keymap.shortcuts(Action::Send), [ctrl_s]);
        assert_eq!(keymap.shortcuts(Action::Save), []);
        assert_eq!(keymap.owner(&ctrl_s, Action::Palette), Some(Action::Send));
    }
}
