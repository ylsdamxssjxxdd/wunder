use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    embed_windows_icon();
}

fn embed_windows_icon() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    println!("cargo:rerun-if-env-changed=WUNDER_WIN7_MINGW_BIN");
    println!("cargo:rerun-if-env-changed=WUNDER_WIN7_IA32_MINGW_BIN");
    println!("cargo:rerun-if-env-changed=WUNDER_WIN7_X64_MINGW_BIN");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon = manifest_dir
        .join("..")
        .join("..")
        .join("images")
        .join("asset-154424-transparent.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if !icon.is_file() {
        println!("cargo:warning=CLI icon not found; exe file icon not embedded");
        return;
    }

    let Some(windres) = find_windres(target.as_str()) else {
        println!("cargo:warning=windres not found; exe file icon not embedded");
        return;
    };
    let format = if target.contains("i686") {
        "pe-i386"
    } else {
        "pe-x86-64"
    };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc_path = out_dir.join("cli-icon.rc");
    let obj_path = out_dir.join("cli-icon.o");
    let icon_path = icon.display().to_string().replace('\\', "/");
    std::fs::write(&rc_path, format!("1 ICON \"{icon_path}\"\r\n"))
        .expect("write CLI icon resource script");

    let status = Command::new(&windres)
        .arg("-F")
        .arg(format)
        .arg("-i")
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj_path)
        .status()
        .expect("run windres");
    if !status.success() {
        println!("cargo:warning=windres failed; exe file icon not embedded");
        return;
    }
    println!("cargo:rustc-link-arg={}", obj_path.display());
}

fn find_windres(target: &str) -> Option<PathBuf> {
    let (arch_override, prefix) = if target.contains("i686") {
        ("WUNDER_WIN7_IA32_MINGW_BIN", "i686-w64-mingw32")
    } else {
        ("WUNDER_WIN7_X64_MINGW_BIN", "x86_64-w64-mingw32")
    };
    // 开发机上的 MinGW 直接叫 windres(.exe)，离线交叉的 portable 包只暴露三元组前缀名。
    let names = [
        format!("{prefix}-windres.exe"),
        format!("{prefix}-windres"),
        "windres.exe".to_string(),
        "windres".to_string(),
    ];
    let mut candidates = Vec::new();
    for variable in [arch_override, "WUNDER_WIN7_MINGW_BIN"] {
        if let Some(bin) = std::env::var_os(variable) {
            let bin = Path::new(&bin);
            candidates.extend(names.iter().map(|name| bin.join(name)));
        }
    }
    candidates.extend(names.iter().map(PathBuf::from));

    candidates.into_iter().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
}
