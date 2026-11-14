//! Dependency rules from `docs/03-arquitectura.md`, checked with `cargo metadata`.

use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, MetadataCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Domain,
    Application,
    Adapter,
    Driver,
    Tooling,
}

/// Layer of each workspace crate. A new, unclassified crate fails the tests.
fn layer(name: &str) -> Option<Layer> {
    match name {
        "apifast-domain" | "apifast-template" => Some(Layer::Domain),
        "apifast-app" => Some(Layer::Application),
        "apifast-storage-fs" | "apifast-http" | "apifast-vault" | "apifast-watch"
        | "apifast-history" | "apifast-import" | "apifast-ws" | "apifast-grpc"
        | "apifast-script-js" => Some(Layer::Adapter),
        "apifast-gui" | "apifast-cli" => Some(Layer::Driver),
        "apifast-arch-tests" => Some(Layer::Tooling),
        _ => None,
    }
}

/// External crates the domain must not use (rule 1).
const FORBIDDEN_IN_DOMAIN: &[&str] = &[
    "tokio", "serde", "reqwest", "egui", "eframe", "wgpu", "oneyaml",
];

/// Normal and build dependencies of each workspace crate.
/// Dev dependencies do not count: a test may use whatever it needs.
#[expect(
    clippy::expect_used,
    reason = "without metadata the tests cannot check anything"
)]
fn graph() -> BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("cargo metadata failed");
    metadata
        .workspace_packages()
        .into_iter()
        .map(|p| {
            let deps = p
                .dependencies
                .iter()
                .filter(|d| d.kind != DependencyKind::Development)
                .map(|d| d.name.clone())
                .collect();
            (p.name.to_string(), deps)
        })
        .collect()
}

/// Whether `from` may depend on the workspace crate `to`.
fn allowed(from: &str, a: Layer, to: &str, b: Layer) -> bool {
    match a {
        // The domain depends on nothing, except template, which may use domain.
        Layer::Domain => from == "apifast-template" && to == "apifast-domain",
        Layer::Application => b == Layer::Domain,
        Layer::Adapter => matches!(b, Layer::Domain | Layer::Application),
        Layer::Driver => b != Layer::Driver && b != Layer::Tooling,
        Layer::Tooling => false,
    }
}

#[test]
fn every_crate_has_a_layer() {
    let unclassified: Vec<_> = graph().into_keys().filter(|n| layer(n).is_none()).collect();
    assert!(
        unclassified.is_empty(),
        "crates without a layer in tests/architecture.rs: {unclassified:?}"
    );
}

#[test]
fn dependencies_respect_the_layers() {
    let graph = graph();
    let mut violations = Vec::new();
    for (from, deps) in &graph {
        let Some(a) = layer(from) else { continue };
        for to in deps.iter().filter(|d| graph.contains_key(*d)) {
            let Some(b) = layer(to) else { continue };
            if !allowed(from, a, to, b) {
                violations.push(format!("{from} ({a:?}) -> {to} ({b:?})"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "forbidden dependencies:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_domain_uses_no_io_or_serialization() {
    let mut violations = Vec::new();
    for (from, deps) in graph() {
        if layer(&from) != Some(Layer::Domain) {
            continue;
        }
        for dep in deps
            .iter()
            .filter(|d| FORBIDDEN_IN_DOMAIN.contains(&d.as_str()))
        {
            violations.push(format!("{from} -> {dep}"));
        }
    }
    assert!(
        violations.is_empty(),
        "the domain uses forbidden crates: {violations:?}"
    );
}

#[test]
fn the_rules_catch_violations() {
    use Layer::{Adapter, Application, Domain, Driver};
    assert!(!allowed(
        "apifast-domain",
        Domain,
        "apifast-template",
        Domain
    ));
    assert!(!allowed(
        "apifast-app",
        Application,
        "apifast-http",
        Adapter
    ));
    assert!(!allowed(
        "apifast-http",
        Adapter,
        "apifast-storage-fs",
        Adapter
    ));
    assert!(!allowed("apifast-http", Adapter, "apifast-cli", Driver));
    assert!(allowed("apifast-cli", Driver, "apifast-http", Adapter));
    assert!(allowed(
        "apifast-template",
        Domain,
        "apifast-domain",
        Domain
    ));
}
