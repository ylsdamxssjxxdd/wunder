use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn create_skill_archive(skill_root: &Path, top_dir: &str, target_zip: &Path) -> Result<()> {
    if let Some(parent) = target_zip.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(target_zip)?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut entries = WalkDir::new(skill_root)
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path().to_string_lossy().to_string());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(skill_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let target_relative = if relative.is_empty() {
            top_dir.to_string()
        } else {
            format!("{top_dir}/{relative}")
        };
        if entry.file_type().is_dir() {
            writer.add_directory(format!("{target_relative}/"), options)?;
            continue;
        }
        writer.start_file(target_relative, options)?;
        let bytes = fs::read(path)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}
