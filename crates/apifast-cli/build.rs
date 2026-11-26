//! The CLI's build script: the commit it is built from and, on Windows, the
//! executable's icon and version details.

#![allow(
    clippy::print_stdout,
    reason = "a build script talks to cargo on stdout"
)]

mod build_commit;
mod build_resources;

fn main() {
    build_commit::emit();
    build_resources::embed("apifast", "apifast command line");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_commit.rs");
}
