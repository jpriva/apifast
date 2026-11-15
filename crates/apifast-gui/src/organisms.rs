//! Organisms: components with their own logic that return events instead of
//! changing the model. Screens turn the events into core commands.

mod kv_table;
mod request_panel;
mod response;
mod tabs;
mod tree;
mod url_bar;

pub use kv_table::{KvRow, KvTable};
pub use request_panel::{RequestChange, RequestPanel, RequestTab};
pub use response::{ResponseTab, ResponseView};
pub use tabs::{TabBar, TabEvent};
pub use tree::{TreeEvent, TreeView};
pub use url_bar::{UrlBar, UrlBarEvent};
