//! The `apifast-gui` binary. `apifast-gui <folder>` opens that collection.

use std::path::PathBuf;
use std::time::Instant;

use apifast_gui::app::App;
use apifast_gui::core::{Command, Core};
use apifast_storage_fs::{FsKeymapStore, FsSettingsStore, WORKSPACE_FILE};
use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::Backends;

fn main() -> eframe::Result {
    let started = Instant::now();
    let timings = std::env::var_os("APIFAST_TIMINGS").is_some();
    let folder = std::env::args_os().nth(1).map(PathBuf::from);
    // The native backends first (Vulkan, Metal, DirectX 12). With OpenGL
    // as well, wgpu also starts the OpenGL driver, which on Linux loads
    // Mesa and LLVM: about 14 MB more at rest (docs/10). OpenGL is the
    // fallback for systems without the others; `WGPU_BACKEND` overrides
    // both.
    match Backends::from_env() {
        Some(backends) => run(backends, folder, started, timings),
        None => run(Backends::PRIMARY, folder.clone(), started, timings)
            .or_else(|_| run(Backends::PRIMARY | Backends::GL, folder, started, timings)),
    }
}

/// Runs the window with wgpu on `backends`, until it closes.
fn run(
    backends: Backends,
    folder: Option<PathBuf>,
    started: Instant,
    timings: bool,
) -> eframe::Result {
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = backends;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("apifast")
            // As the desktop file is named (packaging/linux), so the desktop
            // shows the window with its entry and icon.
            .with_app_id("com.tryxtek.apifast"),
        wgpu_options: WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..WgpuConfiguration::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "apifast",
        options,
        Box::new(move |cc| {
            apifast_gui::theme::install(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let core = Core::start(apifast_history::dirs::data_dir(), move || {
                ctx.request_repaint();
            });
            let dirs = apifast_history::dirs::config_dir();
            let keymap = FsKeymapStore::new(apifast_history::dirs::keymap_file_in(&dirs));
            let settings = FsSettingsStore::new(apifast_history::dirs::settings_file_in(&dirs));
            // Settings first: they tell the core its limits before a
            // collection opens.
            let mut app = App::new(core).with_settings(settings).with_keymap(keymap);
            if timings {
                app = app.with_timings(started);
            }
            // The workspace opens once the core knows the settings.
            app.core().send(Command::Restore);
            match folder {
                Some(folder) if folder.join(WORKSPACE_FILE).is_file() => {
                    app.core().send(Command::OpenWorkspace(folder));
                }
                Some(folder) => app.core().send(Command::OpenCollection(folder)),
                None => {}
            }
            Ok(Box::new(app))
        }),
    )
}
