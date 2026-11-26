//! A rendered request as code to send it (docs/12, F6: "Generación de
//! código"): cURL, JavaScript (`fetch`), Python (`requests`) and Go
//! (`net/http`). What a language's standard client cannot do (an auth the
//! transport applies, such as Digest) is said in a note, never dropped
//! silently.

mod go;
mod javascript;
mod python;

use std::path::Path;

use apifast_app::{RenderedRequest, TransportAuth};

/// A language to write a request in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeLanguage {
    /// A cURL command.
    Curl,
    /// JavaScript with `fetch` (browsers, Node 20+, Deno, Bun).
    JavaScript,
    /// Python with `requests`.
    Python,
    /// Go with `net/http`.
    Go,
}

impl CodeLanguage {
    /// Every language, in the order menus show them.
    pub const ALL: [Self; 4] = [Self::Curl, Self::JavaScript, Self::Python, Self::Go];

    /// Its name, as menus show it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Curl => "cURL",
            Self::JavaScript => "JavaScript (fetch)",
            Self::Python => "Python (requests)",
            Self::Go => "Go (net/http)",
        }
    }
}

/// A request as code, and what it does not do as apifast would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// The code.
    pub text: String,
    /// What the code leaves to the user.
    pub notes: Vec<String>,
}

/// `request` in `language`. Relative file paths are made absolute from
/// `files_root` (the collection's folder), so the code runs anywhere.
pub fn to_code(
    language: CodeLanguage,
    request: &RenderedRequest,
    files_root: Option<&Path>,
) -> Code {
    let files = Files(files_root);
    match language {
        CodeLanguage::Curl => Code {
            text: crate::to_curl(request, files_root),
            notes: Vec::new(),
        },
        CodeLanguage::JavaScript => javascript::code(request, &files),
        CodeLanguage::Python => python::code(request, &files),
        CodeLanguage::Go => go::code(request, &files),
    }
}

/// Where relative file paths are read from.
struct Files<'a>(Option<&'a Path>);

impl Files<'_> {
    fn path(&self, path: &str) -> String {
        file_on_disk(self.0, path)
    }
}

/// Where the file at `path`, relative to the collection in `root` and
/// written with `/` as the collection keeps it, is on this system: joined
/// with the system's separator (`\` on Windows), so the command does not
/// mix both. An absolute path, or one without a root, stays as it is.
pub(crate) fn file_on_disk(root: Option<&Path>, path: &str) -> String {
    match root {
        Some(root) if !Path::new(path).is_absolute() => {
            let mut full = root.to_path_buf();
            full.extend(path.split('/').filter(|part| !part.is_empty()));
            full.display().to_string()
        }
        _ => path.to_owned(),
    }
}

/// `text` as a double-quoted string literal. JSON's escaping is valid in
/// JavaScript, Python and Go alike.
fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// The note for an auth the language's client does not do by itself.
fn auth_note(auth: &TransportAuth, client: &str) -> Option<String> {
    let scheme = match auth {
        TransportAuth::Digest { .. } => "Digest",
        TransportAuth::Ntlm { .. } => "NTLM",
        TransportAuth::AwsV4(_) => "AWS Signature V4",
        _ => return None,
    };
    Some(format!(
        "{scheme} auth is not done by {client}: add it with a library"
    ))
}

#[cfg(test)]
mod tests;
