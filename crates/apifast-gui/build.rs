//! The interface's build script: the commit it is built from, the list of
//! its languages, one per file of `src/strings/` (see that module), and the
//! dependencies it is built with and their licenses, for the About window.

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
    licenses();
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

/// Writes `licenses.rs`: the crates the interface is built with, each with
/// its version and license, sorted by name. Without `cargo metadata` (an
/// unusual build), the list is empty and the About window says so.
fn licenses() {
    let dependencies = dependencies().unwrap_or_else(|e| {
        println!("cargo:warning=the About window will not list the dependencies: {e}");
        Vec::new()
    });
    let mut out = String::from("/// The crates apifast is built with, by name.\n");
    out.push_str("pub const DEPENDENCIES: &[Dependency] = &[\n");
    for (name, version, license) in &dependencies {
        let _ = writeln!(
            out,
            "    Dependency {{ name: {name:?}, version: {version:?}, license: {license:?} }},"
        );
    }
    out.push_str("];\n");
    let target = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|e| panic!("OUT_DIR: {e}")))
        .join("licenses.rs");
    std::fs::write(&target, out)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", target.display()));
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.toml");
}

/// The crates from outside the workspace that this one uses to run (not to
/// build or test), on the platform it is built for: name, version and
/// license.
fn dependencies() -> Result<Vec<(String, String, String)>, String> {
    use serde_json::Value;
    let cargo = std::env::var("CARGO").map_err(|e| format!("CARGO: {e}"))?;
    let target = std::env::var("TARGET").map_err(|e| format!("TARGET: {e}"))?;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .args(["--filter-platform", &target, "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_owned();
    let packages: std::collections::HashMap<String, &Value> = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata: no packages")?
        .iter()
        .map(|p| (text(p, "id"), p))
        .collect();
    let nodes: std::collections::HashMap<String, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata: no resolve")?
        .iter()
        .map(|n| (text(n, "id"), n))
        .collect();
    let root = text(&metadata["resolve"], "root");
    let mut seen = std::collections::BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let Some(node) = nodes.get(&id) else { continue };
        for dep in node["deps"].as_array().into_iter().flatten() {
            let runs = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|kind| kind["kind"].is_null());
            let pkg = text(dep, "pkg");
            if runs && seen.insert(pkg.clone()) {
                pending.push(pkg);
            }
        }
    }
    let mut list: Vec<(String, String, String)> = seen
        .iter()
        .filter_map(|id| packages.get(id))
        // Workspace members have no source: they are apifast itself.
        .filter(|p| !p["source"].is_null())
        .map(|p| {
            let license = p["license"]
                .as_str()
                .map_or_else(|| "see its license file".to_owned(), str::to_owned);
            (text(p, "name"), text(p, "version"), license)
        })
        .collect();
    list.sort();
    list.dedup();
    Ok(list)
}
