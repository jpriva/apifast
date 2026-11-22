//! Puts the commit the binary is built from in `APIFAST_COMMIT` (its short
//! id, or `unknown` outside a git repository), so `apifast --version` and the
//! interface say which code runs. The build scripts of `apifast-cli` and
//! `apifast-gui` both use it.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Tells cargo the commit, and when to look again.
pub fn emit() {
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
}
