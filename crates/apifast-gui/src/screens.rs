//! Screens: a layout filled with real data, turning component events into
//! core commands.

mod main_screen;
mod welcome;

pub use main_screen::{MainScreen, Sidebar};
pub use welcome::Welcome;
