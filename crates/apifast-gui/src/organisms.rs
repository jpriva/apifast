//! Organisms: components with their own logic that return events instead of
//! changing the model. Screens turn the events into core commands.

mod body_viewer;
mod kv_table;
mod request_panel;
mod response;
mod tabs;
mod tree;
mod url_bar;

pub use body_viewer::BodyViewer;
pub use kv_table::{BulkRow, KvRow, KvTable, from_bulk, to_bulk};
pub use request_panel::{RequestChange, RequestPanel, RequestTab};
pub use response::{
    BodyMode, COPY_LIMIT, PRETTY_LIMIT, ResponseTab, ResponseView, describe, millis,
};
pub use tabs::{TabBar, TabEvent};
pub use tree::{TreeEvent, TreeView};
pub use url_bar::{UrlBar, UrlBarEvent};
