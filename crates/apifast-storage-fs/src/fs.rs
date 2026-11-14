//! File system helpers: safe relative paths, atomic writes, ignore patterns
//! and `.env` files.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// Folders apifast never lists or writes into through a request path.
const RESERVED: &[&str] = &[".git", ".apifast", "node_modules"];

/// Why a relative path was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathProblem {
    Empty,
    /// Absolute, or with `..`, `.` or a drive prefix.
    Escapes,
    /// Inside `.git`, `.apifast` or `node_modules`.
    Reserved,
}

impl PathProblem {
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Empty => "the path is empty",
            Self::Escapes => "the path must stay inside the collection",
            Self::Reserved => "the path is inside a reserved folder",
        }
    }
}

/// Joins a `/`-separated path relative to the collection onto `root`,
/// refusing anything that could leave it.
pub(crate) fn resolve(root: &Path, relative: &str) -> Result<PathBuf, PathProblem> {
    if relative.is_empty() || relative.contains('\0') {
        return Err(PathProblem::Empty);
    }
    let mut path = root.to_path_buf();
    for (i, component) in Path::new(relative).components().enumerate() {
        match component {
            Component::Normal(part) => {
                if i == 0 && RESERVED.iter().any(|r| part == *r) {
                    return Err(PathProblem::Reserved);
                }
                path.push(part);
            }
            _ => return Err(PathProblem::Escapes),
        }
    }
    Ok(path)
}

/// Replaces `path` with `contents` atomically: writes a temporary file next
/// to it, flushes it to disk and renames it over the original. A crash leaves
/// either the old file or the new one, never a half-written one. The
/// original's permissions are kept.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.apifast-tmp-{}", std::process::id()));

    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&tmp, metadata.permissions())?;
        }
        fs::rename(&tmp, path)?;
        sync_dir(dir);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Flushes the rename to disk. Best effort: not every platform allows it.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Whether the entry at `relative` (a `/`-separated path) is skipped when
/// listing the collection.
pub(crate) fn is_ignored(relative: &str, patterns: &[String]) -> bool {
    let segments: Vec<&str> = relative.split('/').collect();
    if segments.iter().any(|s| RESERVED.contains(s)) {
        return true;
    }
    if segments.len() == 1 && matches!(segments[0], "environments" | ".env" | "opencollection.yml")
    {
        return true;
    }
    patterns
        .iter()
        .any(|pattern| matches_pattern(relative, &segments, pattern))
}

/// An ignore pattern matches a path equal to it or under it; a pattern
/// without `/` also matches any single segment. `*` matches within a
/// segment and `**` across segments.
fn matches_pattern(relative: &str, segments: &[&str], pattern: &str) -> bool {
    let pattern = pattern
        .trim()
        .trim_start_matches("./")
        .trim_end_matches('/');
    if pattern.is_empty() {
        return false;
    }
    if !pattern.contains('/') {
        return segments
            .iter()
            .any(|s| glob(pattern.as_bytes(), s.as_bytes(), false));
    }
    let parts: Vec<&str> = relative.split('/').collect();
    // The path itself or any of its parents.
    (1..=parts.len()).any(|n| glob(pattern.as_bytes(), parts[..n].join("/").as_bytes(), true))
}

/// Glob match. With `cross`, `**` may span `/`; `*` never does.
fn glob(pattern: &[u8], text: &[u8], cross: bool) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) if cross && rest.first() == Some(&b'*') => {
            let rest = rest[1..].strip_prefix(b"/").unwrap_or(&rest[1..]);
            (0..=text.len()).any(|i| glob(rest, &text[i..], cross))
        }
        Some((b'*', rest)) => (0..=text.len())
            .take_while(|&i| i == 0 || text[i - 1] != b'/')
            .any(|i| glob(rest, &text[i..], cross)),
        Some((&c, rest)) => text.first() == Some(&c) && glob(rest, &text[1..], cross),
    }
}

/// Parses a `.env` file: `NAME=value` lines, with `#` comments, an optional
/// `export ` prefix, and single or double quotes. Double quotes understand
/// `\n`, `\t`, `\"` and `\\`; unquoted values end at ` #`.
pub(crate) fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        vars.insert(name.to_owned(), dotenv_value(value.trim()));
    }
    vars
}

fn dotenv_value(raw: &str) -> String {
    if let Some(inner) = raw
        .strip_prefix('"')
        .and_then(|r| r.rfind('"').map(|end| &r[..end]))
    {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if let Some(inner) = raw
        .strip_prefix('\'')
        .and_then(|r| r.rfind('\'').map(|end| &r[..end]))
    {
        return inner.to_owned();
    }
    raw.split(" #")
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

/// A file name for an environment called `name`: characters that are not
/// valid in file names on some system become `-`.
pub(crate) fn file_name_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        "environment".into()
    } else {
        cleaned.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored(path: &str, patterns: &[&str]) -> bool {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        is_ignored(path, &patterns)
    }

    #[test]
    fn paths_cannot_leave_the_collection() {
        let root = Path::new("/c");
        assert_eq!(
            resolve(root, "users/list.yml"),
            Ok(PathBuf::from("/c/users/list.yml"))
        );
        assert_eq!(resolve(root, "../etc/passwd"), Err(PathProblem::Escapes));
        assert_eq!(resolve(root, "users/../../x"), Err(PathProblem::Escapes));
        assert_eq!(resolve(root, "/etc/passwd"), Err(PathProblem::Escapes));
        assert_eq!(resolve(root, "./x.yml"), Err(PathProblem::Escapes));
        assert_eq!(resolve(root, ""), Err(PathProblem::Empty));
        assert_eq!(resolve(root, "a\0b"), Err(PathProblem::Empty));
        assert_eq!(
            resolve(root, ".apifast/collection.yml"),
            Err(PathProblem::Reserved)
        );
        assert_eq!(resolve(root, ".git/config"), Err(PathProblem::Reserved));
    }

    #[test]
    fn reserved_and_root_files_are_ignored() {
        assert!(ignored(".apifast", &[]));
        assert!(ignored("a/node_modules/x.yml", &[]));
        assert!(ignored("environments", &[]));
        assert!(ignored("opencollection.yml", &[]));
        assert!(!ignored("users/environments", &[]));
        assert!(!ignored("users/opencollection.yml", &[]));
        assert!(!ignored(".hidden/x.yml", &[]));
    }

    #[test]
    fn ignore_patterns_match_names_paths_and_globs() {
        assert!(ignored("drafts", &["drafts"]));
        assert!(ignored("a/drafts/x.yml", &["drafts"]));
        assert!(ignored("a/b.yml", &["a/b.yml"]));
        assert!(ignored("a/b/c.yml", &["a/b"]));
        assert!(!ignored("a/bc.yml", &["a/b"]));
        assert!(ignored("drafts/x/y.yml", &["drafts/**"]));
        assert!(ignored("x/tmp.yml", &["*.tmp.yml", "tmp.yml"]));
        assert!(ignored("x/a.bak", &["*.bak"]));
        assert!(!ignored("x/a.yml", &["*.bak"]));
        assert!(ignored("x/y/z.yml", &["**/z.yml"]));
        assert!(!ignored("x/y.yml", &["x/*/y.yml"]));
        assert!(ignored("x/q/y.yml", &["x/*/y.yml"]));
        assert!(!ignored("a.yml", &["", "  "]));
    }

    #[test]
    fn dotenv_files_are_parsed() {
        let vars = parse_dotenv(
            "# comment\n\nA=1\nexport B = two \nC=\"line\\nnext \\\"q\\\"\"\nD='raw \\n'\nE=x # note\nF=a=b\n=skip\nnoequals\n",
        );
        assert_eq!(vars["A"], "1");
        assert_eq!(vars["B"], "two");
        assert_eq!(vars["C"], "line\nnext \"q\"");
        assert_eq!(vars["D"], "raw \\n");
        assert_eq!(vars["E"], "x");
        assert_eq!(vars["F"], "a=b");
        assert_eq!(vars.len(), 6);
    }

    #[test]
    fn environment_file_names_are_safe() {
        assert_eq!(file_name_for("Producción EU"), "Producción EU");
        assert_eq!(file_name_for("a/b:c"), "a-b-c");
        assert_eq!(file_name_for(".."), "environment");
        assert_eq!(file_name_for("  "), "environment");
    }

    #[test]
    fn atomic_writes_replace_the_file_and_leave_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.yml");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["r.yml"]);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_writes_keep_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.yml");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_failed_atomic_write_leaves_the_original() {
        let dir = tempfile::tempdir().unwrap();
        // The target is a directory, so the rename fails.
        let target = dir.path().join("taken");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "x").unwrap();
        assert!(write_atomic(&target, b"new").is_err());
        assert!(target.join("keep").exists());
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["taken"]);
    }
}
