use crate::services::archive_extract::extract_archive_bytes;
use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;
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

pub fn is_supported_skill_archive_filename(filename: &str) -> bool {
    let lower = filename.trim().to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".skill")
        || lower.ends_with(".rar")
        || lower.ends_with(".7z")
        || lower.ends_with(".tar")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tbz2")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".txz")
        || lower.ends_with(".tar.xz")
}

#[derive(Debug, Clone)]
pub struct ImportedSkillArchive {
    pub extracted: usize,
    pub top_level_dirs: Vec<String>,
}

pub fn import_skill_archive(
    filename: &str,
    data: &[u8],
    target_root: &Path,
    reserved_top_dirs: &HashSet<String>,
) -> Result<ImportedSkillArchive> {
    let temp_root = std::env::temp_dir().join(format!("wskimp-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&temp_root)?;
    let result = (|| -> Result<ImportedSkillArchive> {
        let _ = extract_archive_bytes(filename, data, &temp_root)?;
        let mut extracted = 0usize;
        let mut top_level_dirs = BTreeSet::new();
        let mut files = Vec::new();
        for entry in WalkDir::new(&temp_root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path == temp_root {
                continue;
            }
            let relative = path.strip_prefix(&temp_root).unwrap_or(path).to_path_buf();
            if entry.file_type().is_dir() {
                continue;
            }
            let top_dir = uploaded_skill_archive_top_dir(&relative)?;
            if reserved_top_dirs.contains(&top_dir) {
                return Err(anyhow!(
                    "skill archive conflicts with builtin skill directory"
                ));
            }
            top_level_dirs.insert(top_dir);
            files.push(relative);
            extracted += 1;
        }
        for relative in files {
            let source = temp_root.join(&relative);
            let destination = target_root.join(&relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "copy extracted skill file failed: {} -> {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
        Ok(ImportedSkillArchive {
            extracted,
            top_level_dirs: top_level_dirs.into_iter().collect(),
        })
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

pub fn uploaded_skill_archive_top_dir(path: &Path) -> Result<String> {
    let mut components = path.components();
    let top = components
        .next()
        .ok_or_else(|| anyhow!("skill archive entry is empty"))?;
    if components.next().is_none() {
        return Err(anyhow!(
            "skill archive must contain a dedicated top-level directory"
        ));
    }
    match top {
        std::path::Component::Normal(value) => {
            let text = value.to_string_lossy().trim().to_string();
            if text.is_empty() {
                Err(anyhow!("skill archive top-level directory is empty"))
            } else {
                Ok(text)
            }
        }
        _ => Err(anyhow!("skill archive top-level path is invalid")),
    }
}
