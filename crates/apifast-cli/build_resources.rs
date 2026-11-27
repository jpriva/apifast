//! On Windows, puts apifast's icon (`packaging/windows/apifast.ico`, made
//! from the SVG by `packaging/icons.sh`) and its version details in the
//! executable, so Explorer, the Start menu and the installer show them.
//! Elsewhere it does nothing. The build scripts of `apifast-cli` and
//! `apifast-gui` both use it.

/// The icon, from the manifest's folder of either crate.
const ICON: &str = "../../packaging/windows/apifast.ico";

/// Embeds the icon and the version details in the binary `binary`.
/// `description` is the name Explorer and the task manager show for it.
/// Without a resource compiler (an unusual build), it warns and the
/// executable is built without them.
pub fn embed(binary: &str, description: &str) {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=../apifast-cli/build_resources.rs");
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon(ICON)
            .set("ProductName", "apifast")
            .set("FileDescription", description)
            .set("LegalCopyright", "apifast contributors, MIT OR Apache-2.0");
        if let Err(e) = resource.compile_for(binary) {
            println!("cargo:warning={binary}.exe will have no icon: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (binary, description);
}
