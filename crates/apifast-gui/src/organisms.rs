//! Organisms: components with their own logic that return events instead of
//! changing the model. Screens turn the events into core commands.

mod body_viewer;
mod environments;
mod history;
mod kv_table;
mod palette;
mod request_panel;
mod response;
mod shortcuts;
mod tabs;
mod tree;
mod url_bar;

pub use body_viewer::BodyViewer;
pub use environments::{EnvironmentEditor, EnvironmentEvent, EnvironmentsView};
pub use history::{HistoryView, ago, matches};
pub use kv_table::{BulkRow, KvRow, KvTable, from_bulk, to_bulk};
pub use palette::{CommandPalette, PaletteItem, PaletteMode, fuzzy};
pub use request_panel::{RequestChange, RequestPanel, RequestTab};
pub use response::{
    BodyMode, COPY_LIMIT, PRETTY_LIMIT, ResponseTab, ResponseView, describe, millis,
};
pub use shortcuts::{ShortcutEditor, shortcut_of};
pub use tabs::{TabBar, TabEvent};
pub use tree::{TreeEvent, TreeView};
pub use url_bar::{UrlBar, UrlBarEvent};
