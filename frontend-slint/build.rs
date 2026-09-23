fn main() {
    println!("cargo:rerun-if-changed=ui/main.slint");
    println!("cargo:rerun-if-changed=ui/theme.slint");
    println!("cargo:rerun-if-changed=ui/components.slint");
    println!("cargo:rerun-if-changed=ui/dock.slint");
    println!("cargo:rerun-if-changed=ui/entity_pages.slint");
    println!("cargo:rerun-if-changed=ui/composer.slint");
    println!("cargo:rerun-if-changed=ui/message.slint");
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Slint UI");
}
