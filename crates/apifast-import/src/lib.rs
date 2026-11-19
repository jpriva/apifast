//! Importers and exporters of apifast (docs/12, F6): cURL commands, and
//! Postman collections and environments, each into the app's import model
//! with a list of what it could not keep.

#![forbid(unsafe_code)]

mod curl;
mod postman;

pub use curl::{CurlError, CurlImport, parse_curl, to_curl};
pub use postman::{PostmanError, import_postman, import_postman_environment};
