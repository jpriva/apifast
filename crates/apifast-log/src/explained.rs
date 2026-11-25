//! Warnings of libraries that are not problems, said again: what happened,
//! and what was done about it.
//!
//! wgpu and arboard warn through the `log` crate about things that are
//! normal on many systems: a Vulkan driver that cannot start there and is
//! skipped, or a clipboard protocol the desktop does not have. This logger
//! sits between `log` and `tracing`: it replaces those messages with an
//! `info` event that explains them (target `apifast_log::explained`), and
//! passes every other record on untouched. Replacing them in `tracing`
//! itself is not possible: an event emitted while another is handled is
//! dropped.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing_log::LogTracer;

/// The target of the explanations.
pub const TARGET: &str = "apifast_log::explained";

/// What to do with a record.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Log this instead, at `info`.
    Explain(String),
    /// Drop it: it is the tail of one already explained.
    Drop,
}

/// What a record from `target` saying `text` becomes; `None` if it is not
/// one of the known ones. `after_loader` is whether the last record was a
/// Vulkan loader message, whose `objects:` line follows apart.
pub fn explain(target: &str, text: &str, after_loader: &AtomicBool) -> Option<Action> {
    if target.starts_with("wgpu_hal::vulkan") {
        if text.contains("vkCreateInstance in ICD") && text.contains("Skipping this driver") {
            after_loader.store(true, Ordering::Relaxed);
            return Some(Action::Explain(skipped_driver(text)));
        }
        if after_loader.swap(false, Ordering::Relaxed) && text.trim_start().starts_with("objects:")
        {
            return Some(Action::Drop);
        }
        return None;
    }
    if target.starts_with("arboard") && text.contains("Falling back to the X11 clipboard") {
        return Some(Action::Explain(
            "clipboard: the desktop does not offer Wayland's clipboard protocol for \
             applications (ext-data-control or wlr-data-control; GNOME does not), so the \
             clipboard goes through XWayland's. Copy and paste work the same."
                .to_owned(),
        ));
    }
    None
}

/// The Vulkan loader skipped a driver that cannot start on this system.
fn skipped_driver(text: &str) -> String {
    let between = |start: &str, end: &str| {
        let from = text.find(start)? + start.len();
        let to = text[from..].find(end)? + from;
        Some(text[from..to].trim().to_owned())
    };
    let path = between("ICD ", ". Skipping").unwrap_or_else(|| "a driver".to_owned());
    let code = between("return code ", " from").unwrap_or_else(|| "?".to_owned());
    let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
    if name.contains("dzn") {
        format!(
            "vulkan: the driver {name} (Dozen, Vulkan over DirectX 12) only works on Windows \
             and WSL, so the Vulkan loader skipped it (code {code}) and apifast uses another \
             one. Nothing to do; to stop the loader trying it, uninstall it or set \
             VK_LOADER_DRIVERS_DISABLE=*dzn*."
        )
    } else {
        format!(
            "vulkan: the driver {path} cannot start on this system (vkCreateInstance returned \
             {code}), so the Vulkan loader skipped it and apifast uses another one. Nothing to \
             do, unless no driver starts at all."
        )
    }
}

/// The `log` logger: [`explain`] first, then `tracing`.
#[derive(Debug)]
pub struct Explained {
    tracer: LogTracer,
    after_loader: AtomicBool,
}

impl Default for Explained {
    fn default() -> Self {
        Self::new()
    }
}

impl Explained {
    /// The logger, passing what it does not explain to `tracing`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracer: LogTracer::new(),
            after_loader: AtomicBool::new(false),
        }
    }
}

impl log::Log for Explained {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.tracer.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        let target = record.target();
        if target.starts_with("wgpu_hal::vulkan") || target.starts_with("arboard") {
            let text = record.args().to_string();
            match explain(target, &text, &self.after_loader) {
                Some(Action::Explain(message)) => {
                    self.tracer.log(
                        &log::Record::builder()
                            .args(format_args!("{message}"))
                            .level(log::Level::Info)
                            .target(TARGET)
                            .module_path_static(Some(TARGET))
                            .build(),
                    );
                    return;
                }
                Some(Action::Drop) => return,
                None => {}
            }
        }
        self.tracer.log(record);
    }

    fn flush(&self) {
        self.tracer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DZN: &str = "GENERAL [Loader Message (0x0)]\n    terminator_CreateInstance: Received return code -9 from call to vkCreateInstance in ICD /usr/lib64/libvulkan_dzn.so. Skipping this driver.";

    #[test]
    fn a_skipped_driver_is_explained_and_its_objects_line_dropped() {
        let after = AtomicBool::new(false);
        let Some(Action::Explain(text)) = explain("wgpu_hal::vulkan::instance", DZN, &after) else {
            panic!("not explained");
        };
        assert!(
            text.contains("libvulkan_dzn.so (Dozen") && text.contains("code -9"),
            "{text}"
        );
        let objects = "    objects: (type: INSTANCE, hndl: 0x557629616880, name: ?)";
        assert_eq!(
            explain("wgpu_hal::vulkan::instance", objects, &after),
            Some(Action::Drop)
        );
        // Only right after a loader message.
        assert_eq!(explain("wgpu_hal::vulkan::instance", objects, &after), None);
    }

    #[test]
    fn another_driver_names_its_path() {
        let after = AtomicBool::new(false);
        let text = DZN.replace("libvulkan_dzn.so", "libvulkan_intel.so");
        let Some(Action::Explain(text)) = explain("wgpu_hal::vulkan::instance", &text, &after)
        else {
            panic!("not explained");
        };
        assert!(
            text.contains("/usr/lib64/libvulkan_intel.so cannot start"),
            "{text}"
        );
    }

    #[test]
    fn the_clipboard_fallback_is_explained_and_the_rest_passes() {
        let after = AtomicBool::new(false);
        let arboard = "Tried to initialize the wayland data control protocol clipboard, but failed. Falling back to the X11 clipboard protocol. The error was: …";
        assert!(matches!(
            explain("arboard::platform::linux", arboard, &after),
            Some(Action::Explain(text)) if text.starts_with("clipboard: ")
        ));
        assert_eq!(
            explain("wgpu_hal::vulkan::instance", "device lost", &after),
            None
        );
        assert_eq!(explain("wgpu_core", DZN, &after), None);
    }
}
