use super::PostgresStorage;
use crate::storage::{MediaAssetRecord, SpeechJobRecord, StorageLifecycle};
use anyhow::Result;
use tokio_postgres::Row;

pub(super) trait PostgresMediaStorage {
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

impl PostgresMediaStorage for PostgresStorage {
    fn upsert_media_asset_impl(&self, record: &MediaAssetRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO media_assets (asset_id, kind, url, mime, size, hash, source, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT(asset_id) DO UPDATE SET kind = EXCLUDED.kind, url = EXCLUDED.url, mime = EXCLUDED.mime, size = EXCLUDED.size, hash = EXCLUDED.hash, source = EXCLUDED.source",
            &[
                &record.asset_id,
                &record.kind,
                &record.url,
                &record.mime,
                &record.size,
                &record.hash,
                &record.source,
                &record.created_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE asset_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_media_asset_row(&row)))
    }

    fn get_media_asset_by_hash_impl(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = hash.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE hash = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_media_asset_row(&row)))
    }

    fn upsert_speech_job_impl(&self, record: &SpeechJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO speech_jobs (job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(job_id) DO UPDATE SET status = EXCLUDED.status, input_text = EXCLUDED.input_text, input_url = EXCLUDED.input_url, output_text = EXCLUDED.output_text, \
             output_url = EXCLUDED.output_url, model = EXCLUDED.model, error = EXCLUDED.error, retry_count = EXCLUDED.retry_count, next_retry_at = EXCLUDED.next_retry_at, \
             updated_at = EXCLUDED.updated_at, metadata = EXCLUDED.metadata",
            &[
                &record.job_id,
                &record.job_type,
                &record.status,
                &record.input_text,
                &record.input_url,
                &record.output_text,
                &record.output_url,
                &record.model,
                &record.error,
                &record.retry_count,
                &record.next_retry_at,
                &record.created_at,
                &record.updated_at,
                &metadata,
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
        let mut conn = self.conn()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let rows = conn.query(
            "SELECT job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata \
             FROM speech_jobs WHERE job_type = $1 AND (status = 'queued' OR status = 'retry') AND next_retry_at <= $2 ORDER BY next_retry_at ASC LIMIT $3",
            &[&cleaned, &now, &limit_value],
        )?;
        Ok(rows.iter().map(map_speech_job_row).collect())
    }
}

fn map_media_asset_row(row: &Row) -> MediaAssetRecord {
    MediaAssetRecord {
        asset_id: row.get(0),
        kind: row.get(1),
        url: row.get(2),
        mime: row.get(3),
        size: row.get(4),
        hash: row.get(5),
        source: row.get(6),
        created_at: row.get(7),
    }
}

fn map_speech_job_row(row: &Row) -> SpeechJobRecord {
    SpeechJobRecord {
        job_id: row.get(0),
        job_type: row.get(1),
        status: row.get(2),
        input_text: row.get(3),
        input_url: row.get(4),
        output_text: row.get(5),
        output_url: row.get(6),
        model: row.get(7),
        error: row.get(8),
        retry_count: row.get(9),
        next_retry_at: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
        created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
        updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
        metadata: row
            .get::<_, Option<String>>(13)
            .and_then(|value| PostgresStorage::json_from_str(&value)),
    }
}
