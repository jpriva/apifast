//! The CLI's build script: the commit it is built from.

#![allow(
    clippy::print_stdout,
    reason = "a build script talks to cargo on stdout"
)]

mod build_commit;

fn main() {
    build_commit::emit();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_commit.rs");
}
