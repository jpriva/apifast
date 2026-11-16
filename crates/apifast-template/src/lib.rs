//! Template parser and renderer of apifast.
//!
//! Templates use Bruno's `{{ }}` syntax, plus Yaak-style functions with named
//! arguments:
//!
//! | Form | Example |
//! |---|---|
//! | Variable | `{{host}}` |
//! | Dynamic variable | `{{$randomUUID}}` |
//! | Prompt | `{{?OTP code}}` |
//! | Process environment | `{{process.env.API_KEY}}` |
//! | Function | `{{ response(request='auth/login', path='$.token') }}` |
//! | Escape | `\{{literal}}` |
//!
//! [`Template::parse`] is lossless, so a template can be edited (for example,
//! to rename a variable) without touching the rest of the text. [`render`]
//! resolves the tags; everything impure comes from a [`Host`].

mod dynamic;
mod parse;
mod render;

pub use dynamic::iso_timestamp;
pub use parse::{Expr, Invalid, InvalidReason, Tag, Template, Token};
pub use render::{FunctionArg, Host, MAX_DEPTH, Mode, Problem, RenderError, Rendered, render};
