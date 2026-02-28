#[cfg(feature = "desktop")]
use std::fs;
#[cfg(feature = "desktop")]
use std::path::Path;
#[cfg(feature = "desktop")]
use std::process::Command;
#[cfg(feature = "desktop")]
use std::time::SystemTime;

#[cfg(feature = "desktop")]
fn read_modified(path: &Path) -> Option<SystemTime> {
    path.metadata().ok().and_then(|meta| meta.modified().ok())
}

#[cfg(feature = "desktop")]
fn icons_need_sync(manifest_dir: &Path, icon_source_path: &Path) -> bool {
    let Some(source_mtime) = read_modified(icon_source_path) else {
        return false;
    };
    let targets = [
        manifest_dir.join("wunder-desktop-electron/build/icon.png"),
        manifest_dir.join("wunder-desktop-electron/build/icon.ico"),
        manifest_dir.join("wunder-desktop-electron/assets/icon.ico"),
        manifest_dir.join("wunder-desktop/icons/icon.ico"),
    ];
    targets.iter().any(|target| match read_modified(target) {
        Some(target_mtime) => target_mtime < source_mtime,
        None => true,
    })
}

#[cfg(feature = "desktop")]
fn resolve_icon_source(manifest_dir: &Path) -> Option<std::path::PathBuf> {
    let ico = manifest_dir.join("images/eva01-head.ico");
    if ico.exists() {
        return Some(ico);
    }
    let svg = manifest_dir.join("images/eva01-head.svg");
    if svg.exists() {
        return Some(svg);
    }
    None
}

#[cfg(feature = "desktop")]
fn sync_icons_if_needed(manifest_dir: &Path) {
    let icon_source_path = match resolve_icon_source(manifest_dir) {
        Some(path) => path,
        None => return,
    };
    let icon_sync_script = manifest_dir.join("wunder-desktop-electron/scripts/sync-icons.js");
    let icon_source_svg = manifest_dir.join("images/eva01-head.svg");
    let icon_source_ico = manifest_dir.join("images/eva01-head.ico");

    println!("cargo:rerun-if-changed={}", icon_source_svg.display());
    println!("cargo:rerun-if-changed={}", icon_source_ico.display());
    println!("cargo:rerun-if-changed={}", icon_sync_script.display());

    if !icons_need_sync(manifest_dir, &icon_source_path) {
        return;
    }

    if !icon_sync_script.exists() {
        panic!(
            "desktop icon sync script is missing: {}",
            icon_sync_script.display()
        );
    }

    let output = Command::new("node")
        .arg(&icon_sync_script)
        .current_dir(manifest_dir)
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "run icon sync script failed (need Node.js): {} ({err})",
                icon_sync_script.display()
            )
        });

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "icon sync script failed: {}\nstdout:\n{}\nstderr:\n{}",
            icon_sync_script.display(),
            stdout,
            stderr
        );
    }
}

#[cfg(feature = "desktop")]
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("missing CARGO_MANIFEST_DIR for wunder-desktop build script");
    let manifest_dir = Path::new(&manifest_dir);
    sync_icons_if_needed(manifest_dir);

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
