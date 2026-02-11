#[cfg(feature = "desktop")]
use std::fs;
#[cfg(feature = "desktop")]
use std::path::Path;

#[cfg(feature = "desktop")]
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("missing CARGO_MANIFEST_DIR for wunder-desktop build script");
    let manifest_dir = Path::new(&manifest_dir);
    let desktop_dir = manifest_dir.join("wunder-desktop");
    let root_cargo_path = manifest_dir.join("Cargo.toml");
    let desktop_cargo_path = desktop_dir.join("Cargo.toml");

    println!(
        "cargo:rerun-if-changed={}",
        desktop_dir.join("tauri.conf.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        desktop_dir.join("capabilities").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        desktop_dir.join("icons").display()
    );
    println!("cargo:rerun-if-changed={}", root_cargo_path.display());

    let mut copied_cargo = false;
    if !desktop_cargo_path.exists() {
        fs::copy(&root_cargo_path, &desktop_cargo_path).unwrap_or_else(|err| {
            panic!(
                "copy Cargo.toml from {} to {} failed: {err}",
                root_cargo_path.display(),
                desktop_cargo_path.display()
            )
        });
        copied_cargo = true;
    }

    let original_dir = std::env::current_dir().expect("read current dir failed");
    std::env::set_current_dir(&desktop_dir)
        .unwrap_or_else(|err| panic!("set current dir to {} failed: {err}", desktop_dir.display()));

    let result = tauri_build::try_build(tauri_build::Attributes::new());

    std::env::set_current_dir(&original_dir).unwrap_or_else(|err| {
        panic!(
            "restore current dir to {} failed: {err}",
            original_dir.display()
        )
    });

    if copied_cargo {
        fs::remove_file(&desktop_cargo_path).unwrap_or_else(|err| {
            panic!(
                "remove temporary {} failed: {err}",
                desktop_cargo_path.display()
            )
        });
    }

    if let Err(err) = result {
        panic!("tauri build failed: {err:#}");
    }
}

#[cfg(not(feature = "desktop"))]
fn main() {}
