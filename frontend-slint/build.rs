fn main() {
    println!("cargo:rerun-if-changed=ui/main.slint");
    println!("cargo:rerun-if-changed=ui/theme.slint");
    println!("cargo:rerun-if-changed=ui/components.slint");
    println!("cargo:rerun-if-changed=ui/dock.slint");
    println!("cargo:rerun-if-changed=ui/entity_pages.slint");
    println!("cargo:rerun-if-changed=ui/composer.slint");
    println!("cargo:rerun-if-changed=assets/icons/microphone.svg");
    println!("cargo:rerun-if-changed=ui/message.slint");
    println!("cargo:rerun-if-changed=ui/tray.slint");
    println!("cargo:rerun-if-changed=ui/screenshot_overlay.slint");
    println!("cargo:rerun-if-changed=assets/fonts/simsun.ttf");
    let raw = std::fs::read("assets/fonts/simsun.ttf").expect("read bundled SimSun");
    let compressed = lz4_flex::compress_prepend_size(&raw);
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("simsun.ttf.lz4"), compressed).expect("write compressed SimSun");
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Slint UI");
}
