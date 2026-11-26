//! The `apifast-gui` binary. `apifast-gui <folder>` opens that collection.

// On Windows, a release build is a windowed program: it opens no console
// window beside the interface. Debug builds keep theirs, for what they print.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use apifast_gui::app::App;
use apifast_gui::core::{Command, Core};
use apifast_storage_fs::{FsKeymapStore, FsLayoutStore, FsSettingsStore, WORKSPACE_FILE};
use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::Backends;

fn main() -> eframe::Result {
    let started = Instant::now();
    // The data first: the log goes into its folder, which a migration may
    // change.
    let prepared = prepare_data();
    let (logging, log_file) = start_log();
    tracing::info!(version = apifast_gui::VERSION, "apifast starting");
    if !report(prepared) {
        std::process::exit(1);
    }
    let timings = std::env::var_os("APIFAST_TIMINGS").is_some();
    let folder = std::env::args_os().nth(1).map(PathBuf::from);
    let log = (logging, log_file);
    // The native backends first (Vulkan, Metal, DirectX 12). With OpenGL
    // as well, wgpu also starts the OpenGL driver, which on Linux loads
    // Mesa and LLVM: about 14 MB more at rest (docs/10). OpenGL is the
    // fallback for systems without the others; `WGPU_BACKEND` overrides
    // both.
    match Backends::from_env() {
        Some(backends) => run(backends, folder, started, timings, log),
        None => run(
            Backends::PRIMARY,
            folder.clone(),
            started,
            timings,
            log.clone(),
        )
        .or_else(|error| {
            tracing::warn!(%error, "no native graphics backend; trying OpenGL");
            run(
                Backends::PRIMARY | Backends::GL,
                folder,
                started,
                timings,
                log,
            )
        }),
    }
}

/// The log, and its file (docs/14, "El registro"): `logs/apifast.log` in
/// the data folder, and the terminal. `APIFAST_LOG` sets the filter; else
/// the level setting does, once the settings are read. If the file cannot
/// be opened, only the terminal; if nothing can be set up, no log.
fn start_log() -> (Option<Arc<apifast_log::Logging>>, Option<PathBuf>) {
    let file = apifast_history::dirs::data_dir().join("logs/apifast.log");
    let mut options = apifast_log::LogOptions {
        env: std::env::var("APIFAST_LOG").ok(),
        stderr: true,
        file: Some(file.clone()),
        ..apifast_log::LogOptions::default()
    };
    let (logging, file) = match apifast_log::init(&options) {
        Ok(logging) => (Some(logging), Some(file)),
        Err(apifast_log::LogError::File { .. }) => {
            options.file = None;
            (apifast_log::init(&options).ok(), None)
        }
        Err(apifast_log::LogError::AlreadySet) => (None, None),
    };
    if let Some(problem) = logging.as_ref().and_then(|l| l.problem()) {
        tracing::warn!("{problem}");
    }
    if file.is_none() {
        tracing::warn!("the log has no file: only the terminal gets it");
    }
    (logging.map(Arc::new), file)
}

/// Readies apifast's data for this version (docs/13, "Actualizar"), as the
/// command line does: the migrations since the last version, once.
fn prepare_data() -> Result<apifast_history::migrations::Outcome, String> {
    use apifast_history::migrations::{Dirs, prepare};
    let dirs = Dirs {
        data: apifast_history::dirs::data_dir(),
        config: apifast_history::dirs::config_dir(),
    };
    prepare(&dirs, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())
}

/// Logs what [`prepare_data`] did. Returns whether to go on.
fn report(prepared: Result<apifast_history::migrations::Outcome, String>) -> bool {
    use apifast_history::migrations::Outcome;
    let version = env!("CARGO_PKG_VERSION");
    match prepared {
        Ok(Outcome::Older { newest }) => tracing::warn!(
            "apifast {version} runs on data of {newest}, a newer version: what only that one knows may be lost"
        ),
        Ok(Outcome::Updated {
            from,
            ran,
            backup: Some(backup),
        }) => tracing::warn!(
            "data updated from {from}: {}; a copy of what changed is in {}",
            ran.join(", "),
            backup.display()
        ),
        Ok(_) => {}
        Err(error) => {
            tracing::error!("readying apifast's data for this version: {error}");
            // Without a log, at least the terminal.
            if !tracing::dispatcher::has_been_set() {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr(),
                    "error: readying apifast's data for this version: {error}"
                );
            }
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
    (logging, log_file): (Option<Arc<apifast_log::Logging>>, Option<PathBuf>),
) -> eframe::Result {
    tracing::info!(?backends, "opening the window");
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = backends;
    // As the settings say, before the window opens, so it never shows the
    // other title bar first.
    let settings = FsSettingsStore::new(apifast_history::dirs::settings_file_in(
        &apifast_history::dirs::config_dir(),
    ));
    let own_title_bar = (settings.load().ok())
        .map(|entries| apifast_gui::settings::Settings::from_entries(&entries).0)
        .unwrap_or_default()
        .draws_title_bar();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_decorations(!own_title_bar)
        // With its own title bar, apifast draws the window's frame: rounded
        // corners and a shadow, on a transparent window.
        .with_transparent(own_title_bar)
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
                .with_keymap(keymap)
                .with_layouts(FsLayoutStore::new(apifast_history::dirs::layouts_file_in(
                    &dirs,
                )))
                .with_folders(apifast_history::dirs::data_dir(), dirs)
                .with_transparent_window(own_title_bar);
            if let Some(logging) = logging {
                app = app.with_logging(logging, log_file, std::env::var("APIFAST_LOG").ok());
            }
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
