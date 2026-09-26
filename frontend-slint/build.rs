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
    embed_windows_exe_icon();
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Slint UI");
}

/// Embed the app icon (`images/eva01-head.ico`, multi-size BMP-style entries
/// that old binutils windres handles) as the exe's icon resource plus a
/// VERSIONINFO block built from the crate version so Explorer's file
/// properties show the release number. Resource-only COFF objects from
/// windres link cleanly into the GNU (Win7 i686) binary. This must key off
/// the *target* OS, not the host: cross builds (ARM64 Linux -> i686 Win7) run
/// this build script on Linux. If windres is unavailable the build continues
/// without file resources; the runtime window icon from app-icon.rgba still
/// covers the title bar and taskbar.
fn embed_windows_exe_icon() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    println!("cargo:rerun-if-env-changed=WUNDER_WIN7_MINGW_BIN");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir");
    let ico = std::path::Path::new(&manifest_dir)
        .join("..")
        .join("images")
        .join("eva01-head.ico");
    println!("cargo:rerun-if-changed={}", ico.display());

    // windres -F takes a BFD target name; the "pei-*" names are PE *image*
    // formats (MZ/DOS stub included) and cannot be linked as objects. The
    // linkable COFF object formats are "pe-*".
    let format = if target.contains("i686") {
        "pe-i386"
    } else {
        "pe-x86-64"
    };
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let rc_path = std::path::Path::new(&out_dir).join("app-icon.rc");
    let obj_path = std::path::Path::new(&out_dir).join("app-icon.o");

    // Keep the strings ASCII-only: the GNU windres in the Win7 kit reads the
    // script in the console codepage, so non-ASCII text would be mangled.
    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let version_commas = format!(
        "{},{},{},0",
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0)
    );
    let rc = format!(
        "1 ICON \"{icon}\"\r\n\
         1 VERSIONINFO\r\n\
         FILEVERSION {version_commas}\r\n\
         PRODUCTVERSION {version_commas}\r\n\
         FILEOS 0x40004\r\n\
         FILETYPE 0x1\r\n\
         BEGIN\r\n\
           BLOCK \"StringFileInfo\"\r\n\
           BEGIN\r\n\
             BLOCK \"080404b0\"\r\n\
             BEGIN\r\n\
               VALUE \"CompanyName\", \"wunder\"\r\n\
               VALUE \"FileDescription\", \"wunder Slint Workbench\"\r\n\
               VALUE \"FileVersion\", \"{version}\"\r\n\
               VALUE \"InternalName\", \"wunder-frontend-slint\"\r\n\
               VALUE \"OriginalFilename\", \"wunder-frontend-slint.exe\"\r\n\
               VALUE \"ProductName\", \"wunder-frontend-slint\"\r\n\
               VALUE \"ProductVersion\", \"{version}\"\r\n\
             END\r\n\
           END\r\n\
           BLOCK \"VarFileInfo\"\r\n\
           BEGIN\r\n\
             VALUE \"Translation\", 0x804, 0x4b0\r\n\
           END\r\n\
         END\r\n",
        icon = ico.display().to_string().replace('\\', "/"),
        version_commas = version_commas,
        version = version
    );
    std::fs::write(&rc_path, rc).expect("write resource rc");

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(mingw) = std::env::var("WUNDER_WIN7_MINGW_BIN") {
        candidates.push(std::path::Path::new(&mingw).join("windres.exe"));
        // The Linux cross SDK installs this wrapper name (no .exe suffix).
        candidates.push(std::path::Path::new(&mingw).join("i686-w64-mingw32-windres"));
    }
    candidates.push(std::path::PathBuf::from("i686-w64-mingw32-windres"));
    candidates.push(std::path::PathBuf::from("windres"));
    candidates.push(std::path::PathBuf::from(r"C:\mingw64\bin\windres.exe"));
    candidates.push(std::path::PathBuf::from(
        r"C:\mingw32-12.2-winlibs\mingw32\bin\windres.exe",
    ));
    let windres = candidates.iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    });
    let Some(windres) = windres else {
        println!("cargo:warning=windres not found; exe file resources not embedded");
        return;
    };
    let status = std::process::Command::new(windres)
        .arg("-F")
        .arg(format)
        .arg("-i")
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj_path)
        .status()
        .expect("run windres");
    if !status.success() {
        println!("cargo:warning=windres failed; exe file resources not embedded");
        return;
    }
    println!("cargo:rustc-link-arg={}", obj_path.display());
}
