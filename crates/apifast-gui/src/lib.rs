//! Desktop interface of apifast, built with egui on wgpu.
//!
//! Components follow atomic design (see `docs/09-ui-ux.md`): design tokens
//! in [`theme`], then [`atoms`], [`molecules`] and [`organisms`], arranged
//! by [`layout`] and filled with data by [`screens`]. [`app`] ties the
//! screens to the [`core`].

pub mod app;
pub mod atoms;
pub mod core;
pub mod layout;
pub mod molecules;
pub mod organisms;
pub mod screens;
pub mod strings;
mod system;
pub mod theme;
pub mod vocabulary;
