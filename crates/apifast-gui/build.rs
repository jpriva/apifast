//! The interface's build script: the commit it is built from, and the list
//! of its languages, one per file of `src/strings/` (see that module).

#![allow(
    clippy::print_stdout,
    reason = "a build script talks to cargo on stdout"
)]

use std::fmt::Write as _;
use std::path::Path;

#[path = "../apifast-cli/build_commit.rs"]
mod build_commit;

fn main() {
    build_commit::emit();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/strings");
    let mut codes: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let code = path.file_stem()?.to_str()?.to_owned();
            (path.extension()? == "rs" && code != "mod").then_some(code)
        })
        .collect();
    codes.sort();
    for code in &codes {
        let valid = code.split('_').count() <= 2
            && code.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            && !code.starts_with('_')
            && !code.ends_with('_');
        assert!(
            valid,
            "src/strings/{code}.rs: a language's file is named by its code, `es` or `pt_br`"
        );
    }
    assert!(
        codes.iter().any(|code| code == "en"),
        "src/strings/en.rs is missing: English is what others fall back to"
    );
    let mut out = String::new();
    for code in &codes {
        let file = dir.join(format!("{code}.rs"));
        let _ = writeln!(
            out,
            "#[path = {:?}]\npub mod {code};",
            file.display().to_string()
        );
    }
    out.push_str("/// Every language, one per file of `src/strings/`, by code.\n");
    out.push_str("pub const LANGUAGES: &[Language] = &[\n");
    for code in &codes {
        let _ = writeln!(
            out,
            "    Language {{ code: {code:?}, name: {code}::NAME, strings: &{code}::STRINGS }},"
        );
    }
    out.push_str("];\n");
    let target = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|e| panic!("OUT_DIR: {e}")))
        .join("languages.rs");
    std::fs::write(&target, out)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", target.display()));
    // A new language's file, or one gone.
    println!("cargo:rerun-if-changed=src/strings");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../apifast-cli/build_commit.rs");
}
