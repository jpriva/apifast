//! Línea de comandos de apifast y raíz de composición.

// La CLI es el único crate que escribe directamente en la terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    println!("apifast {}", env!("CARGO_PKG_VERSION"));
}
