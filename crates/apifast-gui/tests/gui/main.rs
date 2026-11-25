//! Tests of the interface, in one binary: each file is a module, so the
//! interface and its dependencies are linked once, not once per file.

mod atoms;
mod body_viewer;
mod code_editor;
mod conflict;
mod cookies;
mod core_thread;
mod editing;
mod environments;
mod flow;
#[cfg(feature = "git")]
mod git;
#[cfg(feature = "grpc")]
mod grpc;
mod history;
mod keyboard;
mod organisms;
mod request_panel;
mod response;
mod runner_window;
mod settings;
mod tabs;
mod template_field;
mod tree;
mod vault;
mod workspace_tree;
