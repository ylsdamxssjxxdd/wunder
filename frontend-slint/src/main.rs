#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();
mod audio_recording;
mod demo;
mod demo_entities;
mod entity_state;
mod hotkey;
mod message_blocks;
mod native_chat;
mod native_pages;
mod native_pages_smoke;
mod native_runtime;
mod native_smoke;
mod runtime_settings;
mod screen_capture;
mod screenshot;
mod shutdown;
mod smoke;
mod subagent_pool;
mod tray;
mod workspace_ui;

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
        if first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-smoke")) {
            args.workspace = Some(directory.join("workspace"));
        }
        let runtime = std::sync::Arc::new(wunder_desktop::NativeDesktop::start_with_args(args)?);
        if first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-restore")) {
            return native_pages_smoke::check_restored(&runtime, &directory);
        }
        native_smoke::check_runtime(&runtime)?;
        native_pages_smoke::check_runtime(&runtime, &directory)?;
        native_runtime::install_ready(&app, runtime);
        app.show()?;
        return native_smoke::run(&app, directory);
    }
    match first_argument.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--demo") => demo::install(&app),
        None | Some("--native") => native_runtime::install(&app),
        Some(_) => return Err("旧 bridge 参数已移除；请直接启动桌面程序".into()),
    }
    tray::install(&app);
    app.show()?;
    app.run()?;
    Ok(())
}

/// Register the compressed SimSun before the first text layout.
/// Keeping one compressed face avoids the complete TTC pair and target font drift.
fn register_bundled_font() -> Result<(), Box<dyn std::error::Error>> {
    use slint::fontique_011::fontique;

    let mut collection = slint::fontique_011::shared_collection();
    let bytes = bundled_font::simsun_ttf();
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return Err("内嵌 SimSun注册失败".into());
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
    static SIMSUN_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/simsun.ttf.lz4"));

    pub fn simsun_ttf() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(SIMSUN_LZ4).expect("内嵌 SimSun解压失败")
            })
            .as_slice()
    }
}

fn install_slint_platform() -> Result<(), Box<dyn std::error::Error>> {
    let backend = i_slint_backend_winit::Backend::builder()
        .with_renderer_name("software")
        .with_window_attributes_hook(|attributes| {
            attributes
                .with_min_inner_size(winit::dpi::LogicalSize::new(760.0, 580.0))
                .with_title("心舰 · 蜂巢")
                .with_decorations(true)
        })
        .build()?;
    slint::platform::set_platform(Box::new(backend))
        .map_err(|error| format!("初始化 Slint Win7 软件渲染平台失败：{error:?}"))?;
    Ok(())
}
