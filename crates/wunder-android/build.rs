fn main() {
    // The shell imports 桌面版's chat components in place; an edit on either side
    // changes the generated code.
    for dir in ["ui", "../../crates/wunder-desktop/frontend/ui"] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("slint") {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
    // File icons are generated from 桌面版's shared vscode-icons asset set, so
    // a document card on the phone resolves the same texture as on the desktop.
    for dir in [
        "../../crates/wunder-desktop/frontend/assets/vscode-icons",
        "../../crates/wunder-desktop/frontend/assets/doc-icons",
    ] {
        println!("cargo:rerun-if-changed={dir}");
    }
    // 桌面版's settings dropdowns use the array search helpers, which stock Slint
    // still gates behind the experimental flag.
    std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "closures");
    slint_build::compile_with_config(
        "ui/mobile.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile the mobile Slint UI");

    // 安卓版 no longer embeds a CJK face: src/fonts.rs registers the fonts that
    // the phone already ships with, so the APK stays small and the typography
    // matches the rest of the system.
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    // The transcript's document cards carry the file type icon, exactly like
    // 桌面版; both shells build that table from the same asset set.
    wunder_icon_builder::write_workspace_icons(
        std::path::Path::new("../../crates/wunder-desktop/frontend/assets"),
        &out_dir,
    );
}
