//! Puts the commit the binary is built from in `APIFAST_COMMIT` (its short
//! id, or `unknown` outside a git repository), so `apifast --version` and the
//! interface say which code runs. `apifast-gui` uses this script too.

#![allow(
    clippy::print_stdout,
    reason = "a build script talks to cargo on stdout"
)]

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn main() {
    let commit = git(&["rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=APIFAST_COMMIT={commit}");
    // Built again when HEAD moves: to another branch, or a commit on it.
    if let (Some(dir), Some(common)) = (
        git(&["rev-parse", "--git-dir"]),
        git(&["rev-parse", "--git-common-dir"]),
    ) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={common}/packed-refs");
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={common}/{branch}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
