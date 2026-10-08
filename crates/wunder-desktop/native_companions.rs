//! Companion (dynamic avatar) management for the native clients. Reuses the
//! runtime companion service — the same on-disk store the web global library
//! reads — so packages imported here are visible to every form factor.

use super::NativeDesktop;
use anyhow::Result;
use wunder_server::companions;

#[derive(Clone, Debug)]
pub struct CompanionSummaryRecord {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub spritesheet_mime: String,
    pub imported_at: f64,
    pub updated_at: f64,
}

fn summary_record(summary: companions::CompanionSummary) -> CompanionSummaryRecord {
    CompanionSummaryRecord {
        id: summary.id,
        display_name: summary.display_name,
        description: summary.description,
        spritesheet_mime: summary.spritesheet_mime,
        imported_at: summary.imported_at,
        updated_at: summary.updated_at,
    }
}

impl NativeDesktop {
    pub fn list_companions(&self) -> Result<Vec<CompanionSummaryRecord>> {
        Ok(companions::list_global_companions()?
            .into_iter()
            .map(summary_record)
            .collect())
    }

    /// Import one `.zip` companion package (pet.json + spritesheet) from disk.
    pub fn import_companion(&self, path: &std::path::Path) -> Result<CompanionSummaryRecord> {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let bytes = std::fs::read(path)?;
        let record = companions::import_global_companion(&file_name, &bytes)?;
        Ok(summary_record(record.summary))
    }

    pub fn delete_companion(&self, id: &str) -> Result<bool> {
        companions::delete_global_companion(id)
    }

    /// Export one companion package into `directory`, returning the zip path.
    pub fn export_companion_to(
        &self,
        id: &str,
        directory: &std::path::Path,
    ) -> Result<std::path::PathBuf> {
        let (file_name, bytes) = companions::export_global_companion(id)?;
        std::fs::create_dir_all(directory)?;
        let path = directory.join(file_name);
        std::fs::write(&path, &bytes)?;
        Ok(path)
    }

    /// Raw spritesheet bytes for rendering; callers decode off the UI thread.
    pub fn companion_spritesheet(&self, id: &str) -> Result<Option<(String, Vec<u8>)>> {
        companions::load_global_companion_spritesheet(id)
    }
}
