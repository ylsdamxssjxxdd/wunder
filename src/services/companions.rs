use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const DEFAULT_ROOT: &str = "config/data/companions/global";
const MANIFEST_NAME: &str = "pet.json";
const MAX_PACKAGE_BYTES: usize = 24 * 1024 * 1024;
const MAX_SPRITESHEET_BYTES: usize = 18 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionManifest {
    pub id: String,
    #[serde(rename = "displayName", alias = "name")]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "spritesheetPath")]
    pub spritesheet_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionRecord {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub spritesheet_path: String,
    pub spritesheet_mime: String,
    pub spritesheet_data_url: String,
    pub imported_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
struct ParsedPackage {
    manifest: CompanionManifest,
    spritesheet_mime: String,
    spritesheet_bytes: Vec<u8>,
}

pub fn global_companion_root() -> PathBuf {
    PathBuf::from(DEFAULT_ROOT)
}

pub fn list_global_companions() -> Result<Vec<CompanionRecord>> {
    let root = global_companion_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&root).with_context(|| format!("read companion dir {}", root.display()))? {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        match load_global_companion(&entry.file_name().to_string_lossy()) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {}
            Err(err) => tracing::warn!("failed to load companion {}: {err}", entry.path().display()),
        }
    }
    records.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
    Ok(records)
}

pub fn load_global_companion(id: &str) -> Result<Option<CompanionRecord>> {
    let id = sanitize_id(id);
    if id.is_empty() {
        return Ok(None);
    }
    let dir = companion_dir(&id);
    let manifest_path = dir.join(MANIFEST_NAME);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read companion manifest {}", manifest_path.display()))?;
    let manifest = normalize_manifest(serde_json::from_str::<CompanionManifest>(&manifest_text)?)?;
    let spritesheet_path = safe_zip_path(&manifest.spritesheet_path)?;
    let image_path = dir.join(&spritesheet_path);
    if !image_path.is_file() {
        return Ok(None);
    }
    let image = fs::read(&image_path).with_context(|| format!("read spritesheet {}", image_path.display()))?;
    let mime = mime_from_path(&spritesheet_path);
    let metadata = fs::metadata(&manifest_path)?;
    let updated_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as f64 / 1000.0)
        .unwrap_or(0.0);
    let imported_at = metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as f64 / 1000.0)
        .unwrap_or(updated_at);
    Ok(Some(CompanionRecord {
        id: manifest.id,
        display_name: manifest.display_name,
        description: manifest.description,
        spritesheet_path: manifest.spritesheet_path,
        spritesheet_mime: mime.to_string(),
        spritesheet_data_url: data_url(&image, mime),
        imported_at,
        updated_at,
    }))
}

pub fn import_global_companion(filename: &str, bytes: &[u8]) -> Result<CompanionRecord> {
    if bytes.is_empty() {
        return Err(anyhow!("companion package is required"));
    }
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(anyhow!("companion package is too large"));
    }
    if !filename.trim().to_ascii_lowercase().ends_with(".zip") {
        return Err(anyhow!("companion package must be a zip file"));
    }
    let parsed = parse_package(bytes)?;
    persist_package(parsed)
}

pub fn update_global_companion(id: &str, display_name: Option<&str>, description: Option<&str>) -> Result<CompanionRecord> {
    let id = sanitize_id(id);
    let Some(mut record) = load_global_companion(&id)? else {
        return Err(anyhow!("companion not found"));
    };
    if let Some(value) = display_name {
        let cleaned = sanitize_text(value, 80);
        if !cleaned.is_empty() {
            record.display_name = cleaned;
        }
    }
    if let Some(value) = description {
        record.description = sanitize_text(value, 240);
    }
    let manifest = CompanionManifest {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        description: record.description.clone(),
        spritesheet_path: record.spritesheet_path.clone(),
    };
    write_manifest(&companion_dir(&record.id), &manifest)?;
    load_global_companion(&record.id)?.ok_or_else(|| anyhow!("companion not found after update"))
}

pub fn delete_global_companion(id: &str) -> Result<bool> {
    let id = sanitize_id(id);
    if id.is_empty() {
        return Ok(false);
    }
    let dir = companion_dir(&id);
    if !dir.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dir).with_context(|| format!("delete companion dir {}", dir.display()))?;
    Ok(true)
}

pub fn export_global_companion(id: &str) -> Result<(String, Vec<u8>)> {
    let id = sanitize_id(id);
    let Some(record) = load_global_companion(&id)? else {
        return Err(anyhow!("companion not found"));
    };
    let dir = companion_dir(&id);
    let image_path = dir.join(safe_zip_path(&record.spritesheet_path)?);
    let image = fs::read(&image_path).with_context(|| format!("read spritesheet {}", image_path.display()))?;
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(MANIFEST_NAME, options)?;
        writer.write_all(serde_json::to_string_pretty(&CompanionManifest {
            id: record.id.clone(),
            display_name: record.display_name.clone(),
            description: record.description.clone(),
            spritesheet_path: record.spritesheet_path.clone(),
        })?.as_bytes())?;
        writer.start_file(&record.spritesheet_path, options)?;
        writer.write_all(&image)?;
        writer.finish()?;
    }
    Ok((format!("{}.zip", record.id), cursor.into_inner()))
}

fn parse_package(bytes: &[u8]) -> Result<ParsedPackage> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("read companion zip failed")?;
    let manifest_bytes = read_zip_entry(&mut archive, MANIFEST_NAME, MAX_PACKAGE_BYTES)?;
    let manifest = normalize_manifest(serde_json::from_slice::<CompanionManifest>(&manifest_bytes)?)?;
    let spritesheet_path = safe_zip_path(&manifest.spritesheet_path)?;
    let spritesheet_zip_path = spritesheet_path.to_string_lossy().replace('\\', "/");
    let spritesheet_bytes =
        read_zip_entry(&mut archive, &spritesheet_zip_path, MAX_SPRITESHEET_BYTES)?;
    let spritesheet_mime = mime_from_path(&spritesheet_path).to_string();
    Ok(ParsedPackage {
        manifest,
        spritesheet_mime,
        spritesheet_bytes,
    })
}

fn persist_package(parsed: ParsedPackage) -> Result<CompanionRecord> {
    let target_dir = companion_dir(&parsed.manifest.id);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;
    let image_path = target_dir.join(safe_zip_path(&parsed.manifest.spritesheet_path)?);
    if let Some(parent) = image_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&image_path, &parsed.spritesheet_bytes)?;
    write_manifest(&target_dir, &parsed.manifest)?;
    load_global_companion(&parsed.manifest.id)?
        .map(|mut record| {
            record.spritesheet_mime = parsed.spritesheet_mime;
            record
        })
        .ok_or_else(|| anyhow!("companion not found after import"))
}

fn read_zip_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let normalized = normalize_zip_path(path);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if normalize_zip_path(file.name()).eq_ignore_ascii_case(&normalized) {
            if file.size() as usize > max_bytes {
                return Err(anyhow!("companion zip entry is too large"));
            }
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut bytes)?;
            if bytes.len() > max_bytes {
                return Err(anyhow!("companion zip entry is too large"));
            }
            return Ok(bytes);
        }
    }
    Err(anyhow!("{path} not found"))
}

fn normalize_manifest(manifest: CompanionManifest) -> Result<CompanionManifest> {
    let id = sanitize_id(&manifest.id);
    let display_name = sanitize_text(&manifest.display_name, 80);
    let description = sanitize_text(&manifest.description, 240);
    let spritesheet_path = safe_zip_path(&manifest.spritesheet_path)?
        .to_string_lossy()
        .replace('\\', "/");
    if id.is_empty() {
        return Err(anyhow!("missing companion id"));
    }
    if display_name.is_empty() {
        return Err(anyhow!("missing companion display name"));
    }
    Ok(CompanionManifest {
        id,
        display_name,
        description,
        spritesheet_path,
    })
}

fn write_manifest(dir: &Path, manifest: &CompanionManifest) -> Result<()> {
    fs::create_dir_all(dir)?;
    let text = format!("{}\n", serde_json::to_string_pretty(manifest)?);
    fs::write(dir.join(MANIFEST_NAME), text)?;
    Ok(())
}

fn companion_dir(id: &str) -> PathBuf {
    global_companion_root().join(sanitize_id(id))
}

fn sanitize_id(value: &str) -> String {
    sanitize_text(value, 80)
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn sanitize_text(value: &str, max_len: usize) -> String {
    value
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_len)
        .collect()
}

fn normalize_zip_path(value: &str) -> String {
    value
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn safe_zip_path(value: &str) -> Result<PathBuf> {
    let normalized = normalize_zip_path(value);
    if normalized.is_empty() || normalized.contains("..") {
        return Err(anyhow!("invalid companion spritesheet path"));
    }
    let path = PathBuf::from(normalized);
    if path.is_absolute() {
        return Err(anyhow!("invalid companion spritesheet path"));
    }
    Ok(path)
}

fn mime_from_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "webp" => "image/webp",
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

fn data_url(bytes: &[u8], mime: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{encoded}")
}

pub fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
