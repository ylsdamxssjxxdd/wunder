#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();
mod demo;
mod smoke;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_slint_platform()?;
    register_complete_font()?;

    let app = MainWindow::new()?;
    demo::install(&app);
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--smoke-check")) {
        let directory = arguments
            .next()
            .ok_or("missing smoke-check output directory")?;
        return smoke::run(&app, std::path::PathBuf::from(directory));
    }
    app.show()?;
    app.run()?;
    Ok(())
}

/// Register the complete Windows TS font before the first text layout.
/// Keeping the font in the executable avoids relying on the target machine's
/// installed fonts, which is required for deterministic Win7 rendering.
fn register_complete_font() -> Result<(), Box<dyn std::error::Error>> {
    use slint::fontique_011::fontique;

    let mut collection = slint::fontique_011::shared_collection();
    let bytes: &'static [u8] = include_bytes!("../../config/fonts/msyh.ttc");
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
    // Register the complete bold face too; the software renderer must not
    // depend on a host font or synthetic weight for TS-equivalent headings.
    let bold: &'static [u8] = include_bytes!("../../config/fonts/msyhbd.ttc");
    collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bold)), None);
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return Err("内嵌字体注册失败".into());
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
