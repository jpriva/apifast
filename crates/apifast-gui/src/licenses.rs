//! apifast's own license and those of the crates it is built with, listed
//! by the build script from `cargo metadata` for the platform built for.

/// apifast's license.
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");

/// Where apifast lives.
pub const SITE: &str = "https://github.com/jpriva/apifast";

/// Where bugs are reported.
pub const BUGS: &str = "https://github.com/jpriva/apifast/issues";

/// A crate apifast is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dependency {
    /// Its name.
    pub name: &'static str,
    /// Its version.
    pub version: &'static str,
    /// Its license, as an SPDX expression.
    pub license: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/licenses.rs"));

/// How many dependencies have each license, most used first (then by
/// name).
pub fn by_license(dependencies: &[Dependency]) -> Vec<(&'static str, usize)> {
    let mut counts = std::collections::BTreeMap::new();
    for dependency in dependencies {
        *counts.entry(dependency.license).or_insert(0) += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dependencies_are_listed_without_apifast_itself() {
        assert!(DEPENDENCIES.len() > 50, "{}", DEPENDENCIES.len());
        assert!(DEPENDENCIES.iter().any(|d| d.name == "eframe"));
        assert!(!DEPENDENCIES.iter().any(|d| d.name.starts_with("apifast")));
        // Only what runs: test-only crates are not shipped.
        assert!(!DEPENDENCIES.iter().any(|d| d.name == "egui_kittest"));
        assert!(DEPENDENCIES.iter().all(|d| !d.license.is_empty()));
    }

    #[test]
    fn licenses_are_counted_most_used_first() {
        let dep = |name, license| Dependency {
            name,
            version: "1.0.0",
            license,
        };
        let list = [dep("a", "MIT"), dep("b", "Apache-2.0"), dep("c", "MIT")];
        assert_eq!(by_license(&list), [("MIT", 2), ("Apache-2.0", 1)]);
    }
}
