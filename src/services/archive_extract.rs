use anyhow::{anyhow, Context, Result};
use encoding_rs::GB18030;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

#[cfg(windows)]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZipLike,
    TarLike,
}

pub fn detect_archive_kind(filename: &str) -> Option<ArchiveKind> {
    let lower = filename.trim().to_ascii_lowercase();
    if lower.ends_with(".zip") || lower.ends_with(".skill") || lower.ends_with(".hivepack") {
        return Some(ArchiveKind::Zip);
    }
    if lower.ends_with(".rar") || lower.ends_with(".7z") {
        return Some(ArchiveKind::SevenZipLike);
    }
    if lower.ends_with(".tar")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tbz2")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".txz")
        || lower.ends_with(".tar.xz")
    {
        return Some(ArchiveKind::TarLike);
    }
    None
}

pub fn extract_archive_bytes(
    filename: &str,
    data: &[u8],
    output_root: &Path,
) -> Result<()> {
    let kind = detect_archive_kind(filename)
        .ok_or_else(|| anyhow!("unsupported archive format: {filename}"))?;
    match kind {
        ArchiveKind::Zip => extract_zip_bytes(data, output_root),
        ArchiveKind::SevenZipLike | ArchiveKind::TarLike => {
            extract_via_system_tool(filename, data, output_root, kind)
        }
    }
}

pub fn extract_zip_bytes(data: &[u8], output_root: &Path) -> Result<()> {
    let cursor = Cursor::new(data.to_vec());
    let mut archive = ZipArchive::new(cursor).context("invalid zip archive")?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).context("invalid zip entry")?;
        let relative = decoded_zip_entry_path(&file)?;
        let destination = output_root.join(&relative);
        if file.is_dir() {
            fs::create_dir_all(&destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        fs::write(&destination, bytes)?;
    }
    Ok(())
}

pub fn validate_archive_entry_path(raw: &str) -> Result<PathBuf> {
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with('\\') {
        return Err(anyhow!("archive entry path is absolute: {normalized}"));
    }
    let path = Path::new(&normalized);
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(anyhow!("archive entry path is unsafe: {normalized}"));
        }
    }
    Ok(path.to_path_buf())
}

pub fn collect_relative_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let relative = path.strip_prefix(root).ok()?;
            if relative.as_os_str().is_empty() {
                None
            } else {
                Some(relative.to_path_buf())
            }
        })
        .collect::<Vec<_>>();
    dirs.sort_by_key(|path| path.to_string_lossy().to_string());
    Ok(dirs)
}

fn decoded_zip_entry_path(file: &zip::read::ZipFile<'_>) -> Result<PathBuf> {
    let decoded = decode_zip_entry_name(file);
    validate_archive_entry_path(&decoded)
}

fn decode_zip_entry_name(file: &zip::read::ZipFile<'_>) -> String {
    let raw = file.name_raw();
    if raw.is_empty() {
        return file.name().replace('\\', "/");
    }
    if let Ok(decoded) = std::str::from_utf8(raw) {
        return decoded.replace('\\', "/");
    }
    let fallback = file.name().replace('\\', "/");
    let (gbk_text, _, had_errors) = GB18030.decode(raw);
    if had_errors {
        return fallback;
    }
    let candidate = gbk_text.into_owned().replace('\\', "/");
    if candidate.contains('\u{fffd}') || candidate.trim().is_empty() {
        fallback
    } else {
        candidate
    }
}

fn extract_via_system_tool(
    filename: &str,
    data: &[u8],
    output_root: &Path,
    kind: ArchiveKind,
) -> Result<()> {
    fs::create_dir_all(output_root)?;
    let temp_root = output_root.join(format!(".extract-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&temp_root)?;
    let extension = archive_temp_suffix(filename, kind);
    let archive_path = temp_root.join(format!("archive{extension}"));
    fs::write(&archive_path, data)?;
    let result = (|| -> Result<()> {
        let (program, args, extractor_name) =
            build_extractor_command(&archive_path, output_root, kind)?;
        run_extractor(&program, &args, output_root).with_context(|| {
            format!(
                "extract archive with {extractor_name} failed: {}",
                archive_path.display()
            )
        })?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn archive_temp_suffix(filename: &str, kind: ArchiveKind) -> String {
    let lower = filename.trim().to_ascii_lowercase();
    for suffix in [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".tbz2", ".txz", ".hivepack", ".skill", ".zip",
        ".rar", ".7z", ".tar",
    ] {
        if lower.ends_with(suffix) {
            return suffix.to_string();
        }
    }
    match kind {
        ArchiveKind::Zip => ".zip".to_string(),
        ArchiveKind::SevenZipLike => ".7z".to_string(),
        ArchiveKind::TarLike => ".tar".to_string(),
    }
}

fn build_extractor_command(
    archive_path: &Path,
    output_root: &Path,
    kind: ArchiveKind,
) -> Result<(String, Vec<String>, &'static str)> {
    let archive = archive_path.to_string_lossy().to_string();
    let output = output_root.to_string_lossy().to_string();
    for program in ["7z", "7zz", "7zr"] {
        if binary_available(program) {
            let args = vec![
                "x".to_string(),
                "-y".to_string(),
                format!("-o{output}"),
                archive.clone(),
            ];
            return Ok((program.to_string(), args, "7z"));
        }
    }

    if kind == ArchiveKind::SevenZipLike {
        for program in ["unrar", "rar"] {
            if binary_available(program) {
                let args = vec![
                    "x".to_string(),
                    "-o+".to_string(),
                    archive.clone(),
                    output.clone(),
                ];
                return Ok((program.to_string(), args, "unrar"));
            }
        }
    }

    if kind == ArchiveKind::TarLike && binary_available("tar") {
        let args = vec![
            "-xf".to_string(),
            archive.clone(),
            "-C".to_string(),
            output.clone(),
        ];
        return Ok(("tar".to_string(), args, "tar"));
    }

    if cfg!(windows) && binary_available("powershell.exe") {
        let command = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            ps_escape(archive_path),
            ps_escape(output_root)
        );
        return Ok((
            "powershell.exe".to_string(),
            vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command,
            ],
            "powershell-expand-archive",
        ));
    }

    if cfg!(windows) && binary_available("tar.exe") {
        let args = vec![
            "-xf".to_string(),
            archive.clone(),
            "-C".to_string(),
            output.clone(),
        ];
        return Ok(("tar.exe".to_string(), args, "tar"));
    }

    let hint = match kind {
        ArchiveKind::Zip => "zip",
        ArchiveKind::SevenZipLike => "7z/unrar",
        ArchiveKind::TarLike => "tar/7z",
    };
    Err(anyhow!(
        "no available extractor found for this archive format; install a common extractor such as {hint}"
    ))
}

fn run_extractor(program: &str, args: &[String], cwd: &Path) -> Result<()> {
    let mut command = Command::new(program);
    command.args(args);
    command.current_dir(cwd);
    apply_platform_spawn_options_std(&mut command);
    let output = command
        .output()
        .with_context(|| format!("spawn extractor {program} failed"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    Err(anyhow!("{detail}"))
}

fn binary_available(program: &str) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .any(|dir| binary_exists_in_dir(&dir, program))
}

fn binary_exists_in_dir(dir: &Path, program: &str) -> bool {
    let candidate = dir.join(program);
    if candidate.is_file() {
        return true;
    }
    #[cfg(windows)]
    {
        if Path::new(program).extension().is_none() {
            for ext in [".exe", ".cmd", ".bat", ".com"] {
                if dir.join(format!("{program}{ext}")).is_file() {
                    return true;
                }
            }
        }
    }
    false
}

fn ps_escape(value: &Path) -> String {
    value
        .as_os_str()
        .to_string_lossy()
        .replace('\'', "''")
}

fn apply_platform_spawn_options_std(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}
