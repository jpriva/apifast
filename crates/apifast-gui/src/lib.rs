//! Desktop interface of apifast, built with egui on wgpu.
//!
//! Components follow atomic design (see `docs/09-ui-ux.md`): design tokens
//! in [`theme`], then [`atoms`], [`molecules`] and [`organisms`], arranged
//! by [`layout`] and filled with data by [`screens`]. [`app`] ties the
//! screens to the [`core`].

/// The version and the commit it is built from: `0.1.0 (abc123456)`.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("APIFAST_COMMIT"), ")");

pub mod actions;
pub mod app;
pub mod atoms;
pub mod body;
pub mod code;
pub mod core;
pub mod layout;
pub mod molecules;
pub mod organisms;
pub mod screens;
pub mod settings;
pub mod strings;
mod system;
pub mod theme;
pub mod vocabulary;
