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
    // 桌面版's settings dropdowns use the array search helpers, which stock Slint
    // still gates behind the experimental flag.
    std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    slint_build::compile_with_config(
        "ui/mobile.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile the mobile Slint UI");

    // 桌面版 registers Microsoft YaHei at startup because Win7 has no CJK face.
    // 安卓版 must show the same typography, so the same face is compressed into
    // the binary and registered before the first frame.
    let raw = std::fs::read("../../crates/wunder-desktop/frontend/assets/fonts/msyh.ttc")
        .expect("read the bundled Microsoft YaHei");
    let compressed = lz4_flex::compress_prepend_size(&raw);
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("msyh.ttc.lz4"), compressed)
        .expect("write compressed Microsoft YaHei");
}
