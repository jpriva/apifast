//! Importers and exporters of apifast (docs/12, F6). For now, cURL: a
//! command becomes a request, with a list of what it could not keep.

#![forbid(unsafe_code)]

mod curl;

pub use curl::{CurlError, CurlImport, parse_curl, to_curl};
