//! Architecture rules from `docs/03-arquitectura.md` and
//! `docs/11-calidad-y-clean-code.md`, checked with `cargo metadata`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cargo_metadata::{DependencyKind, MetadataCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Domain,
    Application,
    Adapter,
    Driver,
    Tooling,
    /// What the tests of several crates share, such as the contract every
    /// adapter of a port must pass. Only a dev dependency may use it.
    TestSupport,
}

/// Layer of each workspace crate. A new, unclassified crate fails the tests.
fn layer(name: &str) -> Option<Layer> {
    match name {
        "apifast-domain" | "apifast-template" | "apifast-query" => Some(Layer::Domain),
        "apifast-app" => Some(Layer::Application),
        "apifast-storage-fs" | "apifast-http" | "apifast-vault" | "apifast-watch"
        | "apifast-history" | "apifast-import" | "apifast-ws" | "apifast-grpc"
        | "apifast-script-js" | "apifast-git" | "apifast-log" => Some(Layer::Adapter),
        "apifast-gui" | "apifast-cli" => Some(Layer::Driver),
        "apifast-arch-tests" => Some(Layer::Tooling),
        "apifast-git-contract" => Some(Layer::TestSupport),
        _ => None,
    }
}

/// External crates the domain must not use.
const FORBIDDEN_IN_DOMAIN: &[&str] = &[
    "tokio", "serde", "reqwest", "egui", "eframe", "wgpu", "oneyaml",
];

/// External crates that only one workspace crate may use directly.
///
/// This keeps each technology behind its adapter, so it can be replaced in one
/// place. It also keeps I/O out of `apifast-app`, since all of these belong to
/// an adapter or a driver.
const EXCLUSIVE_OWNERS: &[(&str, &[&str])] = &[
    ("reqwest", &["apifast-http"]),
    ("hyper", &["apifast-http"]),
    ("rustls", &["apifast-http"]),
    ("oneyaml", &["apifast-storage-fs"]),
    ("keyring", &["apifast-vault"]),
    ("chacha20poly1305", &["apifast-vault"]),
    ("notify", &["apifast-watch"]),
    ("tokio-tungstenite", &["apifast-ws"]),
    ("tonic", &["apifast-grpc"]),
    ("prost-reflect", &["apifast-grpc"]),
    ("tonic-reflection", &["apifast-grpc"]),
    ("protox", &["apifast-grpc"]),
    ("rquickjs", &["apifast-script-js"]),
    ("gix", &["apifast-git"]),
    ("tracing-subscriber", &["apifast-log"]),
    ("roxmltree", &["apifast-query"]),
    ("egui", &["apifast-gui"]),
    ("eframe", &["apifast-gui"]),
    ("wgpu", &["apifast-gui"]),
];

/// External crates reserved for a layer. Only the drivers turn errors into
/// messages, so only they use `anyhow`; the other crates use `thiserror`.
const LAYER_ONLY: &[(&str, Layer)] = &[("anyhow", Layer::Driver)];

const LICENSE: &str = "MIT OR Apache-2.0";

/// A workspace crate, as seen by these tests.
struct Crate {
    name: String,
    /// Normal and build dependencies. Dev dependencies do not count: a test
    /// may use whatever it needs.
    deps: BTreeSet<String>,
    manifest: PathBuf,
    license: Option<String>,
}

#[expect(
    clippy::expect_used,
    reason = "without metadata the tests cannot check anything"
)]
fn workspace() -> Vec<Crate> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("cargo metadata failed");
    metadata
        .workspace_packages()
        .into_iter()
        .map(|p| Crate {
            name: p.name.to_string(),
            deps: p
                .dependencies
                .iter()
                .filter(|d| d.kind != DependencyKind::Development)
                .map(|d| d.name.clone())
                .collect(),
            manifest: p.manifest_path.clone().into_std_path_buf(),
            license: p.license.clone(),
        })
        .collect()
}

/// Whether `from` may depend on the workspace crate `to`.
fn allowed(from: &str, a: Layer, to: &str, b: Layer) -> bool {
    // Test support is for tests: dev dependencies, which are not checked.
    if b == Layer::TestSupport {
        return false;
    }
    match a {
        // The domain depends on nothing; template and query may use it.
        Layer::Domain => {
            matches!(from, "apifast-template" | "apifast-query") && to == "apifast-domain"
        }
        Layer::Application => b == Layer::Domain,
        // The WebSocket handshake is an HTTP request: it goes through the
        // HTTP transport, for its proxy, TLS and certificates; a gRPC
        // connection is opened by it too.
        Layer::Adapter => {
            matches!(b, Layer::Domain | Layer::Application)
                || matches!((from, to), ("apifast-ws" | "apifast-grpc", "apifast-http"))
        }
        Layer::Driver => b != Layer::Driver && b != Layer::Tooling,
        Layer::Tooling => false,
        Layer::TestSupport => matches!(b, Layer::Domain | Layer::Application),
    }
}

/// Whether the workspace crate `from`, in layer `layer`, may use the external
/// crate `dep`, according to [`EXCLUSIVE_OWNERS`] and [`LAYER_ONLY`].
fn external_allowed(from: &str, layer: Layer, dep: &str) -> bool {
    let owner_ok = EXCLUSIVE_OWNERS
        .iter()
        .find(|(name, _)| *name == dep)
        .is_none_or(|(_, owners)| owners.contains(&from));
    let layer_ok = LAYER_ONLY
        .iter()
        .find(|(name, _)| *name == dep)
        .is_none_or(|(_, only)| *only == layer);
    owner_ok && layer_ok
}

fn assert_no_violations(violations: &[String], what: &str) {
    assert!(violations.is_empty(), "{what}:\n{}", violations.join("\n"));
}

#[test]
fn every_crate_has_a_layer() {
    let unclassified: Vec<String> = workspace()
        .into_iter()
        .filter(|c| layer(&c.name).is_none())
        .map(|c| c.name)
        .collect();
    assert_no_violations(
        &unclassified,
        "crates without a layer in tests/architecture.rs",
    );
}

#[test]
fn dependencies_respect_the_layers() {
    let crates = workspace();
    let names: BTreeSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let mut violations = Vec::new();
    for c in &crates {
        let Some(a) = layer(&c.name) else { continue };
        for to in c.deps.iter().filter(|d| names.contains(d.as_str())) {
            let Some(b) = layer(to) else { continue };
            if !allowed(&c.name, a, to, b) {
                violations.push(format!("{} ({a:?}) -> {to} ({b:?})", c.name));
            }
        }
    }
    assert_no_violations(&violations, "forbidden dependencies");
}

#[test]
fn the_domain_uses_no_io_or_serialization() {
    let mut violations = Vec::new();
    for c in workspace() {
        if layer(&c.name) != Some(Layer::Domain) {
            continue;
        }
        for dep in c
            .deps
            .iter()
            .filter(|d| FORBIDDEN_IN_DOMAIN.contains(&d.as_str()))
        {
            violations.push(format!("{} -> {dep}", c.name));
        }
    }
    assert_no_violations(&violations, "the domain uses forbidden crates");
}

#[test]
fn external_crates_stay_with_their_owner() {
    let mut violations = Vec::new();
    for c in workspace() {
        let Some(l) = layer(&c.name) else { continue };
        for dep in c.deps.iter().filter(|d| !external_allowed(&c.name, l, d)) {
            violations.push(format!("{} ({l:?}) -> {dep}", c.name));
        }
    }
    assert_no_violations(
        &violations,
        "external crates used outside their owner (see EXCLUSIVE_OWNERS and LAYER_ONLY)",
    );
}

/// Without `[lints] workspace = true`, a crate silently skips the bans on
/// `unsafe`, `unwrap` and friends.
#[test]
fn every_crate_inherits_the_workspace_lints() {
    let mut violations = Vec::new();
    for c in workspace() {
        let inherits = std::fs::read_to_string(&c.manifest)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|manifest| manifest.get("lints")?.get("workspace")?.as_bool())
            .unwrap_or(false);
        if !inherits {
            violations.push(c.name);
        }
    }
    assert_no_violations(&violations, "crates without `[lints] workspace = true`");
}

#[test]
fn every_crate_has_a_readme_and_the_project_license() {
    let mut violations = Vec::new();
    for c in workspace() {
        if !c.manifest.with_file_name("README.md").is_file() {
            violations.push(format!("{}: no README.md", c.name));
        }
        if c.license.as_deref() != Some(LICENSE) {
            violations.push(format!("{}: license is {:?}", c.name, c.license));
        }
    }
    assert_no_violations(&violations, "crates missing project metadata");
}

#[test]
fn the_rules_catch_violations() {
    use Layer::{Adapter, Application, Domain, Driver, TestSupport};
    // Between workspace crates.
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
    assert!(allowed("apifast-ws", Adapter, "apifast-http", Adapter));
    assert!(!allowed("apifast-http", Adapter, "apifast-ws", Adapter));
    assert!(allowed("apifast-grpc", Adapter, "apifast-http", Adapter));
    assert!(!allowed("apifast-grpc", Adapter, "apifast-ws", Adapter));
    assert!(!allowed(
        "apifast-history",
        Adapter,
        "apifast-http",
        Adapter
    ));
    assert!(allowed("apifast-cli", Driver, "apifast-http", Adapter));
    assert!(!allowed(
        "apifast-git",
        Adapter,
        "apifast-git-contract",
        TestSupport
    ));
    assert!(!allowed(
        "apifast-cli",
        Driver,
        "apifast-git-contract",
        TestSupport
    ));
    assert!(allowed(
        "apifast-git-contract",
        TestSupport,
        "apifast-app",
        Application
    ));
    assert!(!allowed(
        "apifast-git-contract",
        TestSupport,
        "apifast-git",
        Adapter
    ));
    assert!(allowed(
        "apifast-template",
        Domain,
        "apifast-domain",
        Domain
    ));
    // External crates.
    assert!(!external_allowed("apifast-app", Application, "reqwest"));
    assert!(!external_allowed("apifast-app", Application, "oneyaml"));
    assert!(!external_allowed("apifast-storage-fs", Adapter, "reqwest"));
    assert!(!external_allowed("apifast-cli", Driver, "egui"));
    assert!(!external_allowed("apifast-http", Adapter, "anyhow"));
    assert!(external_allowed("apifast-http", Adapter, "reqwest"));
    assert!(external_allowed("apifast-cli", Driver, "anyhow"));
    assert!(external_allowed("apifast-app", Application, "thiserror"));
}
