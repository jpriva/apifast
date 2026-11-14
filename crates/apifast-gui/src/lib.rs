//! Desktop interface of apifast, built with egui on wgpu.
//!
//! Components follow atomic design (see `docs/09-ui-ux.md`): design tokens
//! in [`theme`], then [`atoms`]. Molecules, organisms, layouts and screens
//! arrive with the features that need them.

pub mod atoms;
pub mod theme;
