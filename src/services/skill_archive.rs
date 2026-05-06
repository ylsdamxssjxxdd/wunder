use crate::services::archive_extract::extract_archive_bytes;
use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
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

#[derive(Debug, Clone)]
struct ImportedArchiveEntry {
    source_relative: PathBuf,
    destination_relative: PathBuf,
    top_level_dir: String,
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
        let mut discovered_files = Vec::new();
        for entry in WalkDir::new(&temp_root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path == temp_root {
                continue;
            }
            let relative = path.strip_prefix(&temp_root).unwrap_or(path).to_path_buf();
            if entry.file_type().is_dir() {
                continue;
            }
            discovered_files.push(relative);
        }
        let entries = normalize_imported_skill_entries(&discovered_files, reserved_top_dirs)?;
        let mut top_level_dirs = BTreeSet::new();
        for entry in entries {
            let source = temp_root.join(&entry.source_relative);
            let destination = target_root.join(&entry.destination_relative);
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
            top_level_dirs.insert(entry.top_level_dir);
        }
        Ok(ImportedSkillArchive {
            extracted: discovered_files.len(),
            top_level_dirs: top_level_dirs.into_iter().collect(),
        })
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn normalize_imported_skill_entries(
    files: &[PathBuf],
    reserved_top_dirs: &HashSet<String>,
) -> Result<Vec<ImportedArchiveEntry>> {
    let mut direct_entries = Vec::with_capacity(files.len());
    let mut direct_top_dirs = BTreeSet::new();
    let mut nested_groups: BTreeMap<String, Vec<(PathBuf, PathBuf)>> = BTreeMap::new();
    let mut nested_wrappers = BTreeSet::new();

    for relative in files {
        let components = normalized_path_components(relative)?;
        if components.len() < 2 {
            return Err(anyhow!(
                "skill archive must contain a dedicated top-level directory"
            ));
        }
        if components.len() == 2 {
            let top_dir = components[0].clone();
            ensure_skill_top_dir_allowed(&top_dir, reserved_top_dirs)?;
            direct_top_dirs.insert(top_dir.clone());
            direct_entries.push(ImportedArchiveEntry {
                source_relative: relative.clone(),
                destination_relative: relative.clone(),
                top_level_dir: top_dir,
            });
            continue;
        }

        let wrapper = components[0].clone();
        let nested_top_dir = components[1].clone();
        ensure_skill_top_dir_allowed(&nested_top_dir, reserved_top_dirs)?;
        nested_wrappers.insert(wrapper.clone());
        let destination_relative = build_relative_path(&components[1..]);
        nested_groups
            .entry(wrapper)
            .or_default()
            .push((relative.clone(), destination_relative));
    }

    if direct_entries.is_empty() && nested_groups.len() == 1 && nested_wrappers.len() == 1 {
        let mut output = Vec::with_capacity(files.len());
        for (_, items) in nested_groups {
            for (source_relative, destination_relative) in items {
                let top_level_dir = uploaded_skill_archive_top_dir(&destination_relative)?;
                output.push(ImportedArchiveEntry {
                    source_relative,
                    destination_relative,
                    top_level_dir,
                });
            }
        }
        return Ok(output);
    }

    if !nested_groups.is_empty() {
        return Err(anyhow!(
            "skill archive must place files under a top-level skill directory"
        ));
    }

    if direct_top_dirs.is_empty() {
        return Err(anyhow!(
            "skill archive must contain a dedicated top-level directory"
        ));
    }

    Ok(direct_entries)
}

fn ensure_skill_top_dir_allowed(top_dir: &str, reserved_top_dirs: &HashSet<String>) -> Result<()> {
    if reserved_top_dirs.contains(top_dir) {
        return Err(anyhow!(
            "skill archive conflicts with builtin skill directory"
        ));
    }
    Ok(())
}

fn normalized_path_components(path: &Path) -> Result<Vec<String>> {
    let mut output = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let text = value.to_string_lossy().trim().to_string();
                if text.is_empty() {
                    return Err(anyhow!("skill archive top-level directory is empty"));
                }
                output.push(text);
            }
            _ => return Err(anyhow!("skill archive top-level path is invalid")),
        }
    }
    if output.is_empty() {
        return Err(anyhow!("skill archive entry is empty"));
    }
    Ok(output)
}

fn build_relative_path(components: &[String]) -> PathBuf {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    path
}

pub fn uploaded_skill_archive_top_dir(path: &Path) -> Result<String> {
    let components = normalized_path_components(path)?;
    if components.len() < 2 {
        return Err(anyhow!(
            "skill archive must contain a dedicated top-level directory"
        ));
    }
    Ok(components[0].clone())
}
