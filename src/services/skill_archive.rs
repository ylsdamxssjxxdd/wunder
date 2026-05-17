use crate::services::archive_extract::extract_archive_bytes;
use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
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
    pub final_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct ImportedArchiveEntry {
    source_relative: PathBuf,
    destination_relative: PathBuf,
    top_level_dir: String,
    preferred_name: String,
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
        let entries = normalize_imported_skill_entries(&discovered_files)?;
        let mut top_level_dirs = BTreeSet::new();
        let mut final_names = BTreeSet::new();
        let mut renamed_targets = HashSet::new();
        let mut top_dir_renames: HashMap<String, String> = HashMap::new();
        for entry in entries {
            let final_name = if let Some(existing) = top_dir_renames.get(&entry.top_level_dir) {
                existing.clone()
            } else {
                let resolved = resolve_import_skill_name(
                    target_root,
                    &entry.preferred_name,
                    reserved_top_dirs,
                    &renamed_targets,
                );
                renamed_targets.insert(resolved.clone());
                top_dir_renames.insert(entry.top_level_dir.clone(), resolved.clone());
                resolved
            };
            let source = temp_root.join(&entry.source_relative);
            let relative_under_skill = entry
                .destination_relative
                .strip_prefix(&entry.top_level_dir)
                .unwrap_or(&entry.destination_relative);
            let destination_relative = if relative_under_skill.as_os_str().is_empty() {
                PathBuf::from(&final_name)
            } else {
                PathBuf::from(&final_name).join(relative_under_skill)
            };
            let destination = target_root.join(&destination_relative);
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
            top_level_dirs.insert(final_name.clone());
            final_names.insert(final_name.clone());
        }
        for final_name in &final_names {
            rewrite_skill_md_name(&target_root.join(final_name), final_name)?;
        }
        Ok(ImportedSkillArchive {
            extracted: discovered_files.len(),
            top_level_dirs: top_level_dirs.into_iter().collect(),
            final_names: final_names.into_iter().collect(),
        })
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn normalize_imported_skill_entries(files: &[PathBuf]) -> Result<Vec<ImportedArchiveEntry>> {
    let mut parsed_files = Vec::with_capacity(files.len());
    let mut has_direct_root_file = false;

    for relative in files {
        let components = normalized_path_components(relative)?;
        if components.len() < 2 {
            return Err(anyhow!(
                "skill archive must contain a dedicated top-level directory"
            ));
        }
        if components.len() == 2 {
            has_direct_root_file = true;
        }
        parsed_files.push((relative.clone(), components));
    }

    if has_direct_root_file {
        let mut direct_top_dir: Option<String> = None;
        let mut output = Vec::with_capacity(parsed_files.len());
        for (relative, components) in parsed_files {
            let top_dir = components[0].clone();
            match &direct_top_dir {
                Some(existing) if existing != &top_dir => {
                    return Err(anyhow!(
                        "skill archive must place files under a top-level skill directory"
                    ));
                }
                None => direct_top_dir = Some(top_dir.clone()),
                _ => {}
            }
            output.push(ImportedArchiveEntry {
                source_relative: relative.clone(),
                destination_relative: relative,
                top_level_dir: top_dir,
                preferred_name: components[0].clone(),
            });
        }
        return Ok(output);
    }

    let mut wrapper_dir: Option<String> = None;
    let mut top_level_dir: Option<String> = None;
    let mut output = Vec::with_capacity(parsed_files.len());

    for (relative, components) in parsed_files {
        if components.len() < 3 {
            return Err(anyhow!(
                "skill archive must contain a dedicated top-level directory"
            ));
        }
        let wrapper = components[0].clone();
        let nested_top_dir = components[1].clone();
        match &wrapper_dir {
            Some(existing) if existing != &wrapper => {
                return Err(anyhow!(
                    "skill archive must place files under a top-level skill directory"
                ));
            }
            None => wrapper_dir = Some(wrapper.clone()),
            _ => {}
        }
        match &top_level_dir {
            Some(existing) if existing != &nested_top_dir => {
                return Err(anyhow!(
                    "skill archive must place files under a top-level skill directory"
                ));
            }
            None => top_level_dir = Some(nested_top_dir.clone()),
            _ => {}
        }
        let destination_relative = build_relative_path(&components[1..]);
        output.push(ImportedArchiveEntry {
            source_relative: relative,
            destination_relative,
            top_level_dir: nested_top_dir,
            preferred_name: components[1].clone(),
        });
    }

    Ok(output)
}

fn resolve_import_skill_name(
    skill_root: &Path,
    preferred: &str,
    reserved_top_dirs: &HashSet<String>,
    renamed_targets: &HashSet<String>,
) -> String {
    let base = normalize_import_skill_name_base(preferred, "skill");
    if !reserved_top_dirs.contains(&base)
        && !renamed_targets.contains(&base)
        && !skill_root.join(&base).exists()
    {
        return base;
    }
    let mut index = 2usize;
    loop {
        let next = format!("{base}-{index}");
        if !reserved_top_dirs.contains(&next)
            && !renamed_targets.contains(&next)
            && !skill_root.join(&next).exists()
        {
            return next;
        }
        index += 1;
    }
}

fn normalize_import_skill_name_base(raw: &str, fallback: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return fallback.to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_alphanumeric() {
            if ch.is_ascii() {
                output.push(ch.to_ascii_lowercase());
            } else {
                output.push(ch);
            }
        } else if ch == '_' || ch == '-' {
            output.push(ch);
        } else if ch.is_whitespace() {
            output.push('-');
        }
    }
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

fn rewrite_skill_md_name(skill_dir: &Path, new_name: &str) -> Result<()> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(&skill_md)
        .with_context(|| format!("read {}", skill_md.display()))?;
    let rewritten = rewrite_frontmatter_name(&content, new_name);
    fs::write(&skill_md, rewritten).with_context(|| format!("write {}", skill_md.display()))?;
    Ok(())
}

fn rewrite_frontmatter_name(content: &str, new_name: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_start_matches('\u{feff}');
    let lines = trimmed.lines().collect::<Vec<_>>();
    let has_frontmatter = lines.first().is_some_and(|line| line.trim() == "---");
    if !has_frontmatter {
        return format!("---\nname: {new_name}\n---\n{trimmed}\n");
    }

    let mut result = String::with_capacity(normalized.len() + new_name.len() + 16);
    result.push_str("---\n");
    let mut i = 1usize;
    let mut name_rewritten = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim() == "---" {
            if !name_rewritten {
                result.push_str("name: ");
                result.push_str(new_name);
                result.push('\n');
            }
            result.push_str("---\n");
            for body_line in &lines[i + 1..] {
                result.push_str(body_line);
                result.push('\n');
            }
            return result;
        }
        let trimmed_line = line.trim_start();
        if trimmed_line.starts_with("name:") && !name_rewritten {
            if let Some(colon_pos) = line.find(':') {
                result.push_str(&line[..colon_pos + 1]);
                result.push(' ');
                result.push_str(new_name);
                result.push('\n');
                name_rewritten = true;
                i += 1;
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
        i += 1;
    }
    if !name_rewritten {
        result.push_str("name: ");
        result.push_str(new_name);
        result.push('\n');
    }
    result
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

#[cfg(test)]
pub(crate) fn uploaded_skill_archive_top_dir(path: &Path) -> Result<String> {
    let components = normalized_path_components(path)?;
    if components.len() < 2 {
        return Err(anyhow!(
            "skill archive must contain a dedicated top-level directory"
        ));
    }
    Ok(components[0].clone())
}
