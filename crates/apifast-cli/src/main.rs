//! Command line of apifast and composition root.

// The CLI is the only crate that writes straight to the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    println!("apifast {}", env!("CARGO_PKG_VERSION"));
}
