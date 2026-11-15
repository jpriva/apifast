//! Organisms: components with their own logic that return events instead of
//! changing the model. Screens turn the events into core commands.

mod response;
mod tree;
mod url_bar;

pub use response::{ResponseTab, ResponseView};
pub use tree::{TreeEvent, TreeView};
pub use url_bar::{UrlBar, UrlBarEvent};
