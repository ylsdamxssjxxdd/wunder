use super::SqliteStorage;
use crate::storage::{
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, MemoryJobRecord,
    StorageBackend, UpsertMemoryTaskLogParams,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) trait SqliteMemoryStorage {
    fn get_memory_enabled_impl(&self, user_id: &str) -> Result<Option<bool>>;
    fn set_memory_enabled_impl(&self, user_id: &str, enabled: bool) -> Result<()>;
    fn load_memory_settings_impl(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn upsert_memory_record_impl(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()>;
    fn load_memory_records_impl(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_memory_record_stats_impl(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn delete_memory_record_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_records_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_memory_settings_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_task_log_impl(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()>;
    fn load_memory_task_logs_impl(&self, limit: Option<i64>)
        -> Result<Vec<HashMap<String, Value>>>;
    fn load_memory_task_log_by_task_id_impl(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>>;
    fn delete_memory_task_log_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_task_logs_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_fragment_impl(&self, record: &MemoryFragmentRecord) -> Result<()>;
    fn get_memory_fragment_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>>;
    fn list_memory_fragments_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>>;
    fn get_memory_fragment_embedding_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>>;
    fn upsert_memory_fragment_embedding_impl(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()>;
    fn delete_memory_fragment_embeddings_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64>;
    fn delete_memory_fragment_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64>;
    fn insert_memory_hit_impl(&self, record: &MemoryHitRecord) -> Result<()>;
    fn list_memory_hits_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>>;
    fn list_memory_hit_counts_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<HashMap<String, i64>>;
    fn has_memory_hit_event_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool>;
    fn upsert_memory_job_impl(&self, record: &MemoryJobRecord) -> Result<()>;
    fn list_memory_jobs_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>>;
}

impl SqliteMemoryStorage for SqliteStorage {
    fn get_memory_enabled_impl(&self, user_id: &str) -> Result<Option<bool>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let value: Option<i64> = conn
            .query_row(
                "SELECT enabled FROM memory_settings WHERE user_id = ?",
                params![user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.map(|flag| flag != 0))
    }

    fn set_memory_enabled_impl(&self, user_id: &str, enabled: bool) -> Result<()> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_settings (user_id, enabled, updated_time) VALUES (?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET enabled = excluded.enabled, updated_time = excluded.updated_time",
            params![user_id, if enabled { 1 } else { 0 }, now],
        )?;
        Ok(())
    }

    fn load_memory_settings_impl(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt =
            conn.prepare("SELECT user_id, enabled, updated_time FROM memory_settings")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut output = Vec::new();
        for (user_id, enabled, updated_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("enabled".to_string(), json!(enabled != 0));
            entry.insert("updated_time".to_string(), json!(updated_time));
            output.push(entry);
        }
        Ok(output)
    }

    fn upsert_memory_record_impl(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_summary = summary.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_summary.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = excluded.summary, updated_time = excluded.updated_time",
            params![cleaned_user, cleaned_session, cleaned_summary, now_ts, now_ts],
        )?;
        if max_records > 0 {
            let safe_limit = max_records.max(1);
            conn.execute(
                "DELETE FROM memory_records WHERE user_id = ? AND id NOT IN (\
                    SELECT id FROM memory_records WHERE user_id = ? ORDER BY updated_time DESC, id DESC LIMIT ?\
                 )",
                params![cleaned_user, cleaned_user, safe_limit],
            )?;
        }
        conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ? AND session_id NOT IN (\
                SELECT session_id FROM memory_records WHERE user_id = ?\
             )",
            params![cleaned_user, cleaned_user],
        )?;
        Ok(())
    }

    fn load_memory_records_impl(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let direction = if order_desc { "DESC" } else { "ASC" };
        let query = if limit > 0 {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = ? ORDER BY updated_time {direction}, id {direction} LIMIT ?"
            )
        } else {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = ? ORDER BY updated_time {direction}, id {direction}"
            )
        };
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = if limit > 0 {
            stmt.query_map(params![cleaned, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, f64, f64)>, _>>()?
        } else {
            stmt.query_map(params![cleaned], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, f64, f64)>, _>>()?
        };
        let mut records = Vec::new();
        for (session_id, summary, created_time, updated_time) in rows {
            let mut entry = HashMap::new();
            entry.insert("session_id".to_string(), json!(session_id));
            entry.insert("summary".to_string(), json!(summary));
            entry.insert("created_time".to_string(), json!(created_time));
            entry.insert("updated_time".to_string(), json!(updated_time));
            records.push(entry);
        }
        Ok(records)
    }

    fn get_memory_record_stats_impl(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, COUNT(*) as record_count, MAX(updated_time) as last_time FROM memory_records GROUP BY user_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = Vec::new();
        for (user_id, record_count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("record_count".to_string(), json!(record_count));
            entry.insert("last_time".to_string(), json!(last_time));
            stats.push(entry);
        }
        Ok(stats)
    }

    fn delete_memory_record_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_records_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_settings_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_settings WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn upsert_memory_task_log_impl(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = params.user_id.trim();
        let cleaned_session = params.session_id.trim();
        let cleaned_task = params.task_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_task.is_empty() {
            return Ok(());
        }
        let status_text = params.status.trim();
        let payload_text = params
            .request_payload
            .map(Self::json_to_string)
            .unwrap_or_default();
        let now = params.updated_time.unwrap_or_else(Self::now_ts);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(user_id, session_id) DO UPDATE SET                task_id = excluded.task_id, status = excluded.status, queued_time = excluded.queued_time, started_time = excluded.started_time,                finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, request_payload = excluded.request_payload, result = excluded.result,                error = excluded.error, updated_time = excluded.updated_time",
            params![
                cleaned_task,
                cleaned_user,
                cleaned_session,
                status_text,
                params.queued_time,
                params.started_time,
                params.finished_time,
                params.elapsed_s,
                payload_text,
                params.result,
                params.error,
                now
            ],
        )?;
        Ok(())
    }

    fn load_memory_task_logs_impl(
        &self,
        limit: Option<i64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut query = String::from(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, updated_time FROM memory_task_logs ORDER BY updated_time DESC, id DESC",
        );
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(limit) = limit.filter(|value| *value > 0) {
            query.push_str(" LIMIT ?");
            params_list.push(SqlValue::from(limit));
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get::<_, f64>(5).unwrap_or(0.0),
                    row.get::<_, f64>(6).unwrap_or(0.0),
                    row.get::<_, f64>(7).unwrap_or(0.0),
                    row.get::<_, f64>(8).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<
                Vec<(String, String, String, String, f64, f64, f64, f64, f64)>,
                _,
            >>()?;
        let mut logs = Vec::new();
        for (
            task_id,
            user_id,
            session_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            updated_time,
        ) in rows
        {
            let mut entry = HashMap::new();
            entry.insert("task_id".to_string(), json!(task_id));
            entry.insert("user_id".to_string(), json!(user_id));
            entry.insert("session_id".to_string(), json!(session_id));
            entry.insert("status".to_string(), json!(status));
            entry.insert("queued_time".to_string(), json!(queued_time));
            entry.insert("started_time".to_string(), json!(started_time));
            entry.insert("finished_time".to_string(), json!(finished_time));
            entry.insert("elapsed_s".to_string(), json!(elapsed_s));
            entry.insert("updated_time".to_string(), json!(updated_time));
            logs.push(entry);
        }
        Ok(logs)
    }

    fn load_memory_task_log_by_task_id_impl(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time FROM memory_task_logs WHERE task_id = ? ORDER BY updated_time DESC, id DESC LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![cleaned], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get::<_, f64>(5).unwrap_or(0.0),
                    row.get::<_, f64>(6).unwrap_or(0.0),
                    row.get::<_, f64>(7).unwrap_or(0.0),
                    row.get::<_, String>(8).unwrap_or_default(),
                    row.get::<_, String>(9).unwrap_or_default(),
                    row.get::<_, String>(10).unwrap_or_default(),
                    row.get::<_, f64>(11).unwrap_or(0.0),
                ))
            })
            .optional()?;
        let Some((
            task_id,
            user_id,
            session_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            request_payload,
            result,
            error,
            updated_time,
        )) = row
        else {
            return Ok(None);
        };
        let mut entry = HashMap::new();
        entry.insert("task_id".to_string(), json!(task_id));
        entry.insert("user_id".to_string(), json!(user_id));
        entry.insert("session_id".to_string(), json!(session_id));
        entry.insert("status".to_string(), json!(status));
        entry.insert("queued_time".to_string(), json!(queued_time));
        entry.insert("started_time".to_string(), json!(started_time));
        entry.insert("finished_time".to_string(), json!(finished_time));
        entry.insert("elapsed_s".to_string(), json!(elapsed_s));
        entry.insert("request_payload".to_string(), json!(request_payload));
        entry.insert("result".to_string(), json!(result));
        entry.insert("error".to_string(), json!(error));
        entry.insert("updated_time".to_string(), json!(updated_time));
        Ok(Some(entry))
    }

    fn delete_memory_task_log_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_task_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn upsert_memory_fragment_impl(&self, record: &MemoryFragmentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_fragments (memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(memory_id) DO UPDATE SET user_id = excluded.user_id, agent_id = excluded.agent_id, source_session_id = excluded.source_session_id, source_round_id = excluded.source_round_id, source_type = excluded.source_type, category = excluded.category, title_l0 = excluded.title_l0, summary_l1 = excluded.summary_l1, content_l2 = excluded.content_l2, fact_key = excluded.fact_key, tags = excluded.tags, entities = excluded.entities, importance = excluded.importance, confidence = excluded.confidence, tier = excluded.tier, status = excluded.status, pinned = excluded.pinned, confirmed_by_user = excluded.confirmed_by_user, access_count = excluded.access_count, hit_count = excluded.hit_count, last_accessed_at = excluded.last_accessed_at, valid_from = excluded.valid_from, invalidated_at = excluded.invalidated_at, supersedes_memory_id = excluded.supersedes_memory_id, superseded_by_memory_id = excluded.superseded_by_memory_id, embedding_model = excluded.embedding_model, vector_ref = excluded.vector_ref, created_at = excluded.created_at, updated_at = excluded.updated_at",
            params![
                record.memory_id,
                record.user_id,
                record.agent_id,
                record.source_session_id,
                record.source_round_id,
                record.source_type,
                record.category,
                record.title_l0,
                record.summary_l1,
                record.content_l2,
                record.fact_key,
                Self::string_list_to_json(&record.tags),
                Self::string_list_to_json(&record.entities),
                record.importance,
                record.confidence,
                record.tier,
                record.status,
                if record.pinned { 1 } else { 0 },
                if record.confirmed_by_user { 1 } else { 0 },
                record.access_count,
                record.hit_count,
                record.last_accessed_at,
                record.valid_from,
                record.invalidated_at,
                record.supersedes_memory_id,
                record.superseded_by_memory_id,
                record.embedding_model,
                record.vector_ref,
                record.created_at,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_memory_fragment_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at FROM memory_fragments WHERE user_id = ? AND agent_id = ? AND memory_id = ? LIMIT 1")?;
        let row = stmt
            .query_row(
                params![user_id.trim(), agent_id.trim(), memory_id.trim()],
                |row| {
                    Ok(MemoryFragmentRecord {
                        memory_id: row.get(0)?,
                        user_id: row.get(1)?,
                        agent_id: row.get(2)?,
                        source_session_id: row.get(3)?,
                        source_round_id: row.get(4)?,
                        source_type: row.get(5)?,
                        category: row.get(6)?,
                        title_l0: row.get(7)?,
                        summary_l1: row.get(8)?,
                        content_l2: row.get(9)?,
                        fact_key: row.get(10)?,
                        tags: Self::parse_string_list(row.get(11)?),
                        entities: Self::parse_string_list(row.get(12)?),
                        importance: row.get(13)?,
                        confidence: row.get(14)?,
                        tier: row.get(15)?,
                        status: row.get(16)?,
                        pinned: row.get::<_, Option<i64>>(17)?.unwrap_or(0) != 0,
                        confirmed_by_user: row.get::<_, Option<i64>>(18)?.unwrap_or(0) != 0,
                        access_count: row.get::<_, Option<i64>>(19)?.unwrap_or(0),
                        hit_count: row.get::<_, Option<i64>>(20)?.unwrap_or(0),
                        last_accessed_at: row.get::<_, Option<f64>>(21)?.unwrap_or(0.0),
                        valid_from: row.get::<_, Option<f64>>(22)?.unwrap_or(0.0),
                        invalidated_at: row.get(23)?,
                        supersedes_memory_id: row.get(24)?,
                        superseded_by_memory_id: row.get(25)?,
                        embedding_model: row.get(26)?,
                        vector_ref: row.get(27)?,
                        created_at: row.get::<_, Option<f64>>(28)?.unwrap_or(0.0),
                        updated_at: row.get::<_, Option<f64>>(29)?.unwrap_or(0.0),
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_memory_fragments_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at FROM memory_fragments WHERE user_id = ? AND agent_id = ? ORDER BY pinned DESC, updated_at DESC, created_at DESC")?;
        let rows = stmt
            .query_map(params![user_id.trim(), agent_id.trim()], |row| {
                Ok(MemoryFragmentRecord {
                    memory_id: row.get(0)?,
                    user_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    source_session_id: row.get(3)?,
                    source_round_id: row.get(4)?,
                    source_type: row.get(5)?,
                    category: row.get(6)?,
                    title_l0: row.get(7)?,
                    summary_l1: row.get(8)?,
                    content_l2: row.get(9)?,
                    fact_key: row.get(10)?,
                    tags: Self::parse_string_list(row.get(11)?),
                    entities: Self::parse_string_list(row.get(12)?),
                    importance: row.get(13)?,
                    confidence: row.get(14)?,
                    tier: row.get(15)?,
                    status: row.get(16)?,
                    pinned: row.get::<_, Option<i64>>(17)?.unwrap_or(0) != 0,
                    confirmed_by_user: row.get::<_, Option<i64>>(18)?.unwrap_or(0) != 0,
                    access_count: row.get::<_, Option<i64>>(19)?.unwrap_or(0),
                    hit_count: row.get::<_, Option<i64>>(20)?.unwrap_or(0),
                    last_accessed_at: row.get::<_, Option<f64>>(21)?.unwrap_or(0.0),
                    valid_from: row.get::<_, Option<f64>>(22)?.unwrap_or(0.0),
                    invalidated_at: row.get(23)?,
                    supersedes_memory_id: row.get(24)?,
                    superseded_by_memory_id: row.get(25)?,
                    embedding_model: row.get(26)?,
                    vector_ref: row.get(27)?,
                    created_at: row.get::<_, Option<f64>>(28)?.unwrap_or(0.0),
                    updated_at: row.get::<_, Option<f64>>(29)?.unwrap_or(0.0),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn get_memory_fragment_embedding_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT memory_id, user_id, agent_id, embedding_model, content_hash, vector_json, dimensions, updated_at FROM memory_fragment_embeddings WHERE user_id = ? AND agent_id = ? AND memory_id = ? AND embedding_model = ? AND content_hash = ? LIMIT 1")?;
        stmt.query_row(
            params![
                user_id.trim(),
                agent_id.trim(),
                memory_id.trim(),
                embedding_model.trim(),
                content_hash.trim()
            ],
            |row| {
                let vector_json: String = row.get(5)?;
                Ok(MemoryFragmentEmbeddingRecord {
                    memory_id: row.get(0)?,
                    user_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    embedding_model: row.get(3)?,
                    content_hash: row.get(4)?,
                    vector: Self::json_to_f32_vec(&vector_json),
                    dimensions: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    updated_at: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    fn upsert_memory_fragment_embedding_impl(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE memory_id = ? AND embedding_model = ? AND content_hash <> ?",
            params![record.memory_id, record.embedding_model, record.content_hash],
        )?;
        let vector_json = Self::json_to_string(&Value::Array(
            record.vector.iter().map(|value| json!(value)).collect(),
        ));
        conn.execute(
            "INSERT INTO memory_fragment_embeddings (memory_id, user_id, agent_id, embedding_model, content_hash, vector_json, dimensions, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(memory_id, embedding_model, content_hash) DO UPDATE SET user_id = excluded.user_id, agent_id = excluded.agent_id, vector_json = excluded.vector_json, dimensions = excluded.dimensions, updated_at = excluded.updated_at",
            params![record.memory_id, record.user_id, record.agent_id, record.embedding_model, record.content_hash, vector_json, record.dimensions, record.updated_at],
        )?;
        Ok(())
    }

    fn delete_memory_fragment_embeddings_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE user_id = ? AND agent_id = ? AND memory_id = ?",
            params![user_id.trim(), agent_id.trim(), memory_id.trim()],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_fragment_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let _ = conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE user_id = ? AND agent_id = ? AND memory_id = ?",
            params![user_id.trim(), agent_id.trim(), memory_id.trim()],
        )?;
        let affected = conn.execute(
            "DELETE FROM memory_fragments WHERE user_id = ? AND agent_id = ? AND memory_id = ?",
            params![user_id.trim(), agent_id.trim(), memory_id.trim()],
        )?;
        Ok(affected as i64)
    }

    fn insert_memory_hit_impl(&self, record: &MemoryHitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_hits (hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![record.hit_id, record.memory_id, record.user_id, record.agent_id, record.session_id, record.round_id, record.query_text, Self::json_to_string(&record.reason_json), record.lexical_score, record.semantic_score, record.freshness_score, record.importance_score, record.final_score, record.created_at],
        )?;
        Ok(())
    }

    fn list_memory_hits_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        let safe_limit = limit.max(1);
        let rows = if let Some(cleaned_session) =
            session_id.map(str::trim).filter(|item| !item.is_empty())
        {
            let mut stmt = conn.prepare(
                "SELECT hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at FROM memory_hits WHERE user_id = ? AND agent_id = ? AND session_id = ? ORDER BY created_at DESC LIMIT ?",
            )?;
            let mapped_rows = stmt.query_map(
                params![cleaned_user, cleaned_agent, cleaned_session, safe_limit],
                |row| {
                    let reason_json: String = row.get::<_, Option<String>>(7)?.unwrap_or_default();
                    Ok(MemoryHitRecord {
                        hit_id: row.get(0)?,
                        memory_id: row.get(1)?,
                        user_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        session_id: row.get(4)?,
                        round_id: row.get(5)?,
                        query_text: row.get(6)?,
                        reason_json: Self::json_value_or_null(Some(reason_json)),
                        lexical_score: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                        semantic_score: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                        freshness_score: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                        importance_score: row.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
                        final_score: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                        created_at: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                    })
                },
            )?;
            mapped_rows.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at FROM memory_hits WHERE user_id = ? AND agent_id = ? ORDER BY created_at DESC LIMIT ?",
            )?;
            let mapped_rows =
                stmt.query_map(params![cleaned_user, cleaned_agent, safe_limit], |row| {
                    let reason_json: String = row.get::<_, Option<String>>(7)?.unwrap_or_default();
                    Ok(MemoryHitRecord {
                        hit_id: row.get(0)?,
                        memory_id: row.get(1)?,
                        user_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        session_id: row.get(4)?,
                        round_id: row.get(5)?,
                        query_text: row.get(6)?,
                        reason_json: Self::json_value_or_null(Some(reason_json)),
                        lexical_score: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                        semantic_score: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                        freshness_score: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                        importance_score: row.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
                        final_score: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                        created_at: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                    })
                })?;
            mapped_rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    fn list_memory_hit_counts_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT memory_id, COUNT(DISTINCT CASE
                WHEN TRIM(round_id) <> '' THEN session_id || '::r::' || TRIM(round_id)
                WHEN TRIM(query_text) <> '' THEN session_id || '::q::' || TRIM(query_text)
                ELSE NULL
             END) AS hit_count
             FROM memory_hits
             WHERE user_id = ? AND agent_id = ?
             GROUP BY memory_id",
        )?;
        let rows = stmt.query_map(params![user_id.trim(), agent_id.trim()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            ))
        })?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    }

    fn has_memory_hit_event_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        let cleaned_memory = memory_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty()
            || cleaned_agent.is_empty()
            || cleaned_memory.is_empty()
            || cleaned_session.is_empty()
        {
            return Ok(false);
        }

        if let Some(cleaned_round) = round_id.map(str::trim).filter(|value| !value.is_empty()) {
            let exists = conn.query_row(
                "SELECT 1 FROM memory_hits
                 WHERE user_id = ? AND agent_id = ? AND memory_id = ? AND session_id = ? AND round_id = ?
                 LIMIT 1",
                params![cleaned_user, cleaned_agent, cleaned_memory, cleaned_session, cleaned_round],
                |_| Ok(()),
            );
            return Ok(exists.is_ok());
        }

        if let Some(cleaned_query) = query_text.map(str::trim).filter(|value| !value.is_empty()) {
            let exists = conn.query_row(
                "SELECT 1 FROM memory_hits
                 WHERE user_id = ? AND agent_id = ? AND memory_id = ? AND session_id = ?
                   AND TRIM(round_id) = '' AND query_text = ?
                 LIMIT 1",
                params![
                    cleaned_user,
                    cleaned_agent,
                    cleaned_memory,
                    cleaned_session,
                    cleaned_query
                ],
                |_| Ok(()),
            );
            return Ok(exists.is_ok());
        }

        Ok(false)
    }

    fn upsert_memory_job_impl(&self, record: &MemoryJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_jobs (job_id, user_id, agent_id, session_id, job_type, status, request_payload, result_summary, error_message, queued_at, started_at, finished_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(job_id) DO UPDATE SET user_id = excluded.user_id, agent_id = excluded.agent_id, session_id = excluded.session_id, job_type = excluded.job_type, status = excluded.status, request_payload = excluded.request_payload, result_summary = excluded.result_summary, error_message = excluded.error_message, queued_at = excluded.queued_at, started_at = excluded.started_at, finished_at = excluded.finished_at, updated_at = excluded.updated_at",
            params![record.job_id, record.user_id, record.agent_id, record.session_id, record.job_type, record.status, Self::json_to_string(&record.request_payload), record.result_summary, record.error_message, record.queued_at, record.started_at, record.finished_at, record.updated_at],
        )?;
        Ok(())
    }

    fn list_memory_jobs_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT job_id, user_id, agent_id, session_id, job_type, status, request_payload, result_summary, error_message, queued_at, started_at, finished_at, updated_at FROM memory_jobs WHERE user_id = ? AND agent_id = ? ORDER BY updated_at DESC LIMIT ?")?;
        let rows = stmt
            .query_map(
                params![user_id.trim(), agent_id.trim(), limit.max(1)],
                |row| {
                    let payload: String = row.get::<_, Option<String>>(6)?.unwrap_or_default();
                    Ok(MemoryJobRecord {
                        job_id: row.get(0)?,
                        user_id: row.get(1)?,
                        agent_id: row.get(2)?,
                        session_id: row.get(3)?,
                        job_type: row.get(4)?,
                        status: row.get(5)?,
                        request_payload: Self::json_value_or_null(Some(payload)),
                        result_summary: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                        error_message: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                        queued_at: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                        started_at: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                        finished_at: row.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
                        updated_at: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
