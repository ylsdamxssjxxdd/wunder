#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();
mod agent_editor;
mod audio_recording;
pub(crate) mod avatar_assets;
mod avatar_ui;
mod companion_sprite;
mod channel_ui;
mod cron_ui;
mod demo;
mod demo_entities;
mod entity_state;
mod expert_ui;
mod file_dialog;
mod file_icons;
mod hotkey;
mod message_blocks;
mod timeline;
mod timeline_text;
mod turn_stats;
mod code_highlight;
mod native_chat;
mod native_interlink;
mod native_pages;
mod native_runtime;
mod native_restore;
mod native_smoke;
mod navigation_ui;
mod workspace_ui;
mod companion_pet;
mod pet_window;
mod runtime_settings;
mod screen_capture;
mod screenshot;
mod settings_search;
mod shutdown;
mod smoke;
mod subagent_detail;
mod terminal_grid;
mod thread_trajectory;
mod tool_icons;
mod tools_ui;
mod tray;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_slint_platform()?;
    register_bundled_font()?;

    let mut arguments = std::env::args_os().skip(1);
    let first_argument = arguments.next();
    let app = MainWindow::new()?;
    let close_weak = app.as_weak();
    app.window().on_close_requested(move || {
        if let Some(app) = close_weak.upgrade() {
            if crate::tray::hide(&app) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
        }
        crate::shutdown::request_exit();
        slint::CloseRequestResponse::KeepWindowShown
    });
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--smoke-check")) {
        demo::install(&app);
        let directory = arguments
            .next()
            .ok_or("missing smoke-check output directory")?;
        return smoke::run(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--timeline-probe")) {
        // §十二.2: measure the worst-case timeline on the real renderer without
        // depending on the rest of the smoke check.
        let directory = arguments
            .next()
            .ok_or("missing timeline-probe output directory")?;
        return smoke::run_timeline_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--hide-probe")) {
        // The hide verdict can only be measured on a window the screen really
        // shows, so the probe starts the same session production starts.
        native_runtime::install(&app);
        tray::install(&app);
        app.show()?;
        maximize_main_window(&app);
        return screenshot::run_hide_probe(&app);
    }
    if matches!(
        first_argument.as_deref().and_then(std::ffi::OsStr::to_str),
        Some("--native-smoke" | "--native-restore")
    ) {
        let directory = std::path::PathBuf::from(
            arguments
                .next()
                .ok_or("missing isolated native directory")?,
        );
        if !directory.join("runtime/config/wunder.yaml").is_file() {
            return Err("native smoke requires an explicitly prepared isolated runtime".into());
        }
        let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
        args.temp_root = Some(directory.join("runtime").canonicalize()?);
        let restoring = first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-restore"));
        if !restoring {
            args.workspace = Some(directory.join("workspace"));
        }
        let runtime = std::sync::Arc::new(wunder_desktop::NativeDesktop::start_with_args(args)?);
        // The restore phase deliberately runs a read-only guard: the streaming
        // phase's guard writes probe fixtures, and a check that mutates the
        // store cannot prove what survived the restart.
        if !restoring {
            native_smoke::check_runtime(&runtime)?;
        }
        native_runtime::install_ready_without_pets(&app, runtime.clone());
        app.show()?;
        return if restoring {
            native_restore::run(&app, directory, runtime)
        } else {
            native_smoke::run(&app, directory)
        };
    }
    match first_argument.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--demo") => demo::install(&app),
        None | Some("--native") => native_runtime::install(&app),
        Some(_) => return Err("旧 bridge 参数已移除；请直接启动桌面程序".into()),
    }
    tray::install(&app);
    app.show()?;
    maximize_main_window(&app);
    app.run()?;
    Ok(())
}

/// Apply the startup maximized state after the Slint window has been created.
///
/// Winit can ignore a maximized attribute while a window is still withdrawn
/// (most visibly on X11). Windows receives the request from the platform hook,
/// while other platforms retry briefly after the first map.
fn maximize_main_window(app: &MainWindow) {
    #[cfg(windows)]
    {
        app.window().set_maximized(true);
    }

    #[cfg(not(windows))]
    {
        use slint::winit_030::WinitWindowAccessor;
        use std::cell::Cell;
        use std::rc::Rc;

        let weak = app.as_weak();
        let timer = Rc::new(slint::Timer::default());
        let timer_handle = Rc::clone(&timer);
        let attempts = Rc::new(Cell::new(0u32));
        let attempts_handle = Rc::clone(&attempts);
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(50),
            move || {
                attempts_handle.set(attempts_handle.get().saturating_add(1));
                let applied = weak
                    .upgrade()
                    .map(|app| {
                        app.window().with_winit_window(|native| {
                            if native.is_maximized() {
                                true
                            } else if native.is_visible().unwrap_or(false) {
                                native.set_maximized(true);
                                true
                            } else {
                                false
                            }
                        })
                    })
                    .flatten()
                    .unwrap_or(true);
                if applied || attempts_handle.get() >= 100 {
                    timer_handle.stop();
                }
            },
        );
    }
}

/// Register the compressed Microsoft YaHei before the first text layout.
/// The bundled TTC keeps the UI independent of fonts installed on the target
/// machine while matching the system default CJK face the preview path uses.
pub(crate) fn register_bundled_font() -> Result<(), Box<dyn std::error::Error>> {
    use slint::fontique_011::fontique;

    let mut collection = slint::fontique_011::shared_collection();
    let bytes = bundled_font::yahei_ttc();
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return Err("内嵌 Microsoft YaHei 注册失败".into());
    }
    for generic in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::SystemUi,
        fontique::GenericFamily::UiSansSerif,
    ] {
        collection.set_generic_families(generic, families.iter().copied());
    }
    collection.append_fallbacks(
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None),
        families.iter().copied(),
    );
    Ok(())
}

mod bundled_font {
    static YAHEI_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/msyh.ttc.lz4"));

    pub fn yahei_ttc() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(YAHEI_LZ4)
                    .expect("内嵌 Microsoft YaHei 解压失败")
            })
            .as_slice()
    }
}

fn install_slint_platform() -> Result<(), Box<dyn std::error::Error>> {
    let maximize_first_window = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let backend = i_slint_backend_winit::Backend::builder()
        .with_renderer_name("software")
        .with_window_attributes_hook({
            let maximize_first_window = std::sync::Arc::clone(&maximize_first_window);
            move |attributes| {
                attributes
                    .with_min_inner_size(winit::dpi::LogicalSize::new(1024.0, 700.0))
                    .with_title("蜂窝")
                    .with_decorations(true)
                    // Only the first top-level is the workbench. Auxiliary
                    // windows (screenshot selector and tray notice) keep
                    // their authored sizes.
                    .with_maximized(maximize_first_window.swap(
                        false,
                        std::sync::atomic::Ordering::AcqRel,
                    ))
            }
        })
        .build()?;
    slint::platform::set_platform(Box::new(backend))
        .map_err(|error| format!("初始化 Slint Win7 软件渲染平台失败：{error:?}"))?;
    Ok(())
}
