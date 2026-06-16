use super::SqliteStorage;
use crate::storage::{MediaAssetRecord, SpeechJobRecord, StorageLifecycle};
use anyhow::Result;
use rusqlite::{params, OptionalExtension, Row};

pub(super) trait SqliteMediaStorage {
    fn upsert_media_asset_impl(&self, record: &MediaAssetRecord) -> Result<()>;
    fn get_media_asset_impl(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>>;
    fn get_media_asset_by_hash_impl(&self, hash: &str) -> Result<Option<MediaAssetRecord>>;
    fn upsert_speech_job_impl(&self, record: &SpeechJobRecord) -> Result<()>;
    fn list_pending_speech_jobs_impl(
        &self,
        job_type: &str,
        limit: i64,
    ) -> Result<Vec<SpeechJobRecord>>;
}

impl SqliteMediaStorage for SqliteStorage {
    fn upsert_media_asset_impl(&self, record: &MediaAssetRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO media_assets (asset_id, kind, url, mime, size, hash, source, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(asset_id) DO UPDATE SET kind = excluded.kind, url = excluded.url, mime = excluded.mime, size = excluded.size, hash = excluded.hash, source = excluded.source",
            params![
                record.asset_id,
                record.kind,
                record.url,
                record.mime,
                record.size,
                record.hash,
                record.source,
                record.created_at
            ],
        )?;
        Ok(())
    }

    fn get_media_asset_impl(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = asset_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE asset_id = ?",
                params![cleaned],
                map_media_asset_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_media_asset_by_hash_impl(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = hash.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE hash = ?",
                params![cleaned],
                map_media_asset_row,
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_speech_job_impl(&self, record: &SpeechJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO speech_jobs (job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(job_id) DO UPDATE SET status = excluded.status, input_text = excluded.input_text, input_url = excluded.input_url, output_text = excluded.output_text, \
             output_url = excluded.output_url, model = excluded.model, error = excluded.error, retry_count = excluded.retry_count, next_retry_at = excluded.next_retry_at, \
             updated_at = excluded.updated_at, metadata = excluded.metadata",
            params![
                record.job_id,
                record.job_type,
                record.status,
                record.input_text,
                record.input_url,
                record.output_text,
                record.output_url,
                record.model,
                record.error,
                record.retry_count,
                record.next_retry_at,
                record.created_at,
                record.updated_at,
                metadata
            ],
        )?;
        Ok(())
    }

    fn list_pending_speech_jobs_impl(
        &self,
        job_type: &str,
        limit: i64,
    ) -> Result<Vec<SpeechJobRecord>> {
        self.ensure_initialized()?;
        let cleaned = job_type.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let mut stmt = conn.prepare(
            "SELECT job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata \
             FROM speech_jobs WHERE job_type = ? AND (status = 'queued' OR status = 'retry') AND next_retry_at <= ? ORDER BY next_retry_at ASC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![cleaned, now, limit_value], map_speech_job_row)?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }
}

fn map_media_asset_row(row: &Row<'_>) -> rusqlite::Result<MediaAssetRecord> {
    Ok(MediaAssetRecord {
        asset_id: row.get(0)?,
        kind: row.get(1)?,
        url: row.get(2)?,
        mime: row.get(3)?,
        size: row.get(4)?,
        hash: row.get(5)?,
        source: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn map_speech_job_row(row: &Row<'_>) -> rusqlite::Result<SpeechJobRecord> {
    let metadata_text: Option<String> = row.get(13)?;
    Ok(SpeechJobRecord {
        job_id: row.get(0)?,
        job_type: row.get(1)?,
        status: row.get(2)?,
        input_text: row.get(3)?,
        input_url: row.get(4)?,
        output_text: row.get(5)?,
        output_url: row.get(6)?,
        model: row.get(7)?,
        error: row.get(8)?,
        retry_count: row.get(9)?,
        next_retry_at: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        metadata: metadata_text.and_then(|value| SqliteStorage::json_from_str(&value)),
    })
}
