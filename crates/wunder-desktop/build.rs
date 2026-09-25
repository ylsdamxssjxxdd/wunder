#[cfg(feature = "desktop")]
use std::path::{Path, PathBuf};
#[cfg(feature = "desktop")]
const FAST_CHECK_TAURI_CONFIG: &str = r#"{
  "build": {
    "frontendDist": null
  },
  "bundle": {
    "createUpdaterArtifacts": false,
    "externalBin": [],
    "resources": []
  }
}"#;

#[cfg(feature = "desktop")]
fn resolve_repo_root_and_tauri_dir(manifest_dir: &Path) -> PathBuf {
    let tauri_dir = manifest_dir.to_path_buf();
    tauri_dir
}

#[cfg(feature = "desktop")]
fn truthy_env(name: &str) -> bool {
    match std::env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) => matches!(value.as_str(), "1" | "true" | "yes" | "on"),
        None => false,
    }
}

#[cfg(feature = "desktop")]
fn configure_fast_tauri_check() {
    println!("cargo:rerun-if-env-changed=WUNDER_TAURI_FULL_RESOURCES");
    if truthy_env("WUNDER_TAURI_FULL_RESOURCES") {
        return;
    }

    let profile = std::env::var("PROFILE").unwrap_or_default();
    if !matches!(profile.as_str(), "debug" | "dev") {
        return;
    }

    // `tauri-build` copies every bundle resource during cargo check. Keep dev
    // validation focused on Rust code; packaging still uses the full config.
    std::env::set_var("TAURI_CONFIG", FAST_CHECK_TAURI_CONFIG);
}

#[cfg(feature = "desktop")]
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("missing CARGO_MANIFEST_DIR for wunder-desktop build script");
    let manifest_dir = Path::new(&manifest_dir);
    let tauri_dir = resolve_repo_root_and_tauri_dir(manifest_dir);

    println!(
        "cargo:rerun-if-changed={}",
        tauri_dir.join("tauri.conf.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        tauri_dir.join("capabilities").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        tauri_dir.join("icons").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        tauri_dir.join("Cargo.toml").display()
    );

    let original_dir = std::env::current_dir().expect("read current dir failed");
    std::env::set_current_dir(&tauri_dir)
        .unwrap_or_else(|err| panic!("set current dir to {} failed: {err}", tauri_dir.display()));

    configure_fast_tauri_check();
    let result = tauri_build::try_build(tauri_build::Attributes::new());

    std::env::set_current_dir(&original_dir).unwrap_or_else(|err| {
        panic!(
            "restore current dir to {} failed: {err}",
            original_dir.display()
        )
    });

    if let Err(err) = result {
        panic!("tauri build failed: {err:#}");
    }
}

#[cfg(not(feature = "desktop"))]
fn main() {}
