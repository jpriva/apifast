//! The known warnings of libraries, through the real `log` facade and the
//! global log: replaced by their explanation, at `info`. One test: the log
//! is set up once per process.

use apifast_log::{LogLevel, LogOptions, init};

#[test]
fn known_warnings_arrive_explained_and_others_as_they_are() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let file = dir.path().join("apifast.log");
    let logging = init(&LogOptions {
        level: LogLevel::Info,
        file: Some(file.clone()),
        ..LogOptions::default()
    })
    .unwrap_or_else(|e| panic!("{e}"));
    let vulkan = "wgpu_hal::vulkan::instance";
    log::warn!(target: vulkan, "GENERAL [Loader Message (0x0)]\n    terminator_CreateInstance: Received return code -9 from call to vkCreateInstance in ICD /usr/lib64/libvulkan_dzn.so. Skipping this driver.");
    log::warn!(target: vulkan, "    objects: (type: INSTANCE, hndl: 0x557629616880, name: ?)");
    log::warn!(target: "arboard::platform::linux", "Tried to initialize the wayland data control protocol clipboard, but failed. Falling back to the X11 clipboard protocol. The error was: …");
    log::warn!(target: "wgpu_core::device", "a real warning");
    log::debug!(target: "wgpu_core::device", "below the libraries' level");

    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        text.contains(" INFO apifast_log::explained: vulkan: the driver libvulkan_dzn.so (Dozen"),
        "{text}"
    );
    assert!(
        text.contains(" INFO apifast_log::explained: clipboard: the desktop does not offer"),
        "{text}"
    );
    assert!(
        !text.contains("Skipping this driver") && !text.contains("objects:"),
        "{text}"
    );
    assert!(!text.contains("Falling back"), "{text}");
    assert!(
        text.contains(" WARN wgpu_core::device: a real warning"),
        "{text}"
    );
    assert!(!text.contains("below the libraries' level"), "{text}");

    // At warn, the explanations (info) are not written at all.
    logging.set_level(LogLevel::Warn);
    log::warn!(target: "arboard::platform::linux", "Falling back to the X11 clipboard protocol, again");
    let again = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(again.matches("clipboard:").count(), 1, "{again}");
    assert!(!again.contains("again"), "{again}");
}
