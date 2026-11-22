//! The `apifast-gui` binary. `apifast-gui <folder>` opens that collection.

// On Windows, a release build is a windowed program: it opens no console
// window beside the interface. Debug builds keep theirs, for what they print.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
    if !prepare_data() {
        std::process::exit(1);
    }
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

/// Readies apifast's data for this version (docs/13, "Actualizar"), as the
/// command line does: the migrations since the last version, once. What it
/// did, or why it could not, goes to stderr. Returns whether to go on.
fn prepare_data() -> bool {
    use apifast_history::migrations::{Dirs, Outcome, prepare};
    use std::io::Write as _;
    let dirs = Dirs {
        data: apifast_history::dirs::data_dir(),
        config: apifast_history::dirs::config_dir(),
    };
    let version = env!("CARGO_PKG_VERSION");
    let mut stderr = std::io::stderr();
    match prepare(&dirs, version) {
        Ok(Outcome::Older { newest }) => {
            let _ = writeln!(
                stderr,
                "warning: apifast {version} runs on data of {newest}, a newer version: what only that one knows may be lost"
            );
        }
        Ok(Outcome::Updated {
            from,
            ran,
            backup: Some(backup),
        }) => {
            let _ = writeln!(
                stderr,
                "apifast: data updated from {from}: {}; a copy of what changed is in {}",
                ran.join(", "),
                backup.display()
            );
        }
        Ok(_) => {}
        Err(error) => {
            let _ = writeln!(
                stderr,
                "error: readying apifast's data for this version: {error}"
            );
            return false;
        }
    }
    true
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_title("apifast")
        // As the desktop file is named (packaging/linux), so the desktop
        // shows the window with its entry and icon.
        .with_app_id("com.tryxtek.apifast");
    if let Some(icon) = apifast_gui::app::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport,
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
            let system = apifast_gui::strings::system_language(|name| std::env::var(name).ok());
            let mut app = App::new(core)
                .with_system_language(system)
                .with_settings(settings)
                .with_keymap(keymap);
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
