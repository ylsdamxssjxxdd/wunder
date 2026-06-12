use super::PostgresStorage;
use crate::storage::{
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, MemoryJobRecord,
    StorageBackend, UpsertMemoryTaskLogParams,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresMemoryStorage {
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

impl PostgresMemoryStorage for PostgresStorage {
    fn get_memory_enabled_impl(&self, _user_id: &str) -> Result<Option<bool>> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT enabled FROM memory_settings WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| row.get::<_, i32>(0) != 0))
    }

    fn set_memory_enabled_impl(&self, _user_id: &str, _enabled: bool) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let enabled_value: i32 = if _enabled { 1 } else { 0 };
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_settings (user_id, enabled, updated_time) VALUES ($1, $2, $3) \
             ON CONFLICT(user_id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_time = EXCLUDED.updated_time",
            &[&cleaned, &enabled_value, &now],
        )?;
        Ok(())
    }

    fn load_memory_settings_impl(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, enabled, updated_time FROM memory_settings",
            &[],
        )?;
        let mut output = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let enabled: i32 = row.get(1);
            let updated_time: f64 = row.try_get(2).unwrap_or(0.0);
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
        _user_id: &str,
        _session_id: &str,
        _summary: &str,
        _max_records: i64,
        _now_ts: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        let cleaned_summary = _summary.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_summary.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = EXCLUDED.summary, updated_time = EXCLUDED.updated_time",
            &[&cleaned_user, &cleaned_session, &cleaned_summary, &_now_ts, &_now_ts],
        )?;
        if _max_records > 0 {
            let safe_limit = _max_records.max(1);
            conn.execute(
                "DELETE FROM memory_records WHERE user_id = $1 AND id NOT IN (\
                    SELECT id FROM memory_records WHERE user_id = $1 ORDER BY updated_time DESC, id DESC LIMIT $2\
                 )",
                &[&cleaned_user, &safe_limit],
            )?;
        }
        conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1 AND session_id NOT IN (\
                SELECT session_id FROM memory_records WHERE user_id = $1\
             )",
            &[&cleaned_user],
        )?;
        Ok(())
    }

    fn load_memory_records_impl(
        &self,
        _user_id: &str,
        _limit: i64,
        _order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let direction = if _order_desc { "DESC" } else { "ASC" };
        let query = if _limit > 0 {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = $1 ORDER BY updated_time {direction}, id {direction} LIMIT $2"
            )
        } else {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = $1 ORDER BY updated_time {direction}, id {direction}"
            )
        };
        let mut conn = self.conn()?;
        let rows = if _limit > 0 {
            conn.query(&query, &[&cleaned, &_limit])?
        } else {
            conn.query(&query, &[&cleaned])?
        };
        let mut records = Vec::new();
        for row in rows {
            let session_id: String = row.get(0);
            let summary: String = row.get(1);
            let created_time: f64 = row.try_get(2).unwrap_or(0.0);
            let updated_time: f64 = row.try_get(3).unwrap_or(0.0);
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, COUNT(*) as record_count, MAX(updated_time) as last_time FROM memory_records GROUP BY user_id",
            &[],
        )?;
        let mut stats = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let record_count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("record_count".to_string(), json!(record_count));
            entry.insert("last_time".to_string(), json!(last_time));
            stats.push(entry);
        }
        Ok(stats)
    }

    fn delete_memory_record_impl(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_records_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected =
            conn.execute("DELETE FROM memory_records WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_memory_settings_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_settings WHERE user_id = $1",
            &[&cleaned],
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
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)              ON CONFLICT(user_id, session_id) DO UPDATE SET                task_id = EXCLUDED.task_id, status = EXCLUDED.status, queued_time = EXCLUDED.queued_time, started_time = EXCLUDED.started_time,                finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, request_payload = EXCLUDED.request_payload, result = EXCLUDED.result,                error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &cleaned_task,
                &cleaned_user,
                &cleaned_session,
                &status_text,
                &params.queued_time,
                &params.started_time,
                &params.finished_time,
                &params.elapsed_s,
                &payload_text,
                &params.result,
                &params.error,
                &now,
            ],
        )?;
        Ok(())
    }

    fn load_memory_task_logs_impl(
        &self,
        _limit: Option<i64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut query = String::from(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, updated_time FROM memory_task_logs ORDER BY updated_time DESC, id DESC",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(limit) = _limit.filter(|value| *value > 0) {
            query.push_str(" LIMIT $1");
            params.push(Box::new(limit));
        }
        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut logs = Vec::new();
        for row in rows {
            let task_id: String = row.get(0);
            let user_id: String = row.get(1);
            let session_id: String = row.get(2);
            let status: String = row.get(3);
            let queued_time: f64 = row.try_get(4).unwrap_or(0.0);
            let started_time: f64 = row.try_get(5).unwrap_or(0.0);
            let finished_time: f64 = row.try_get(6).unwrap_or(0.0);
            let elapsed_s: f64 = row.try_get(7).unwrap_or(0.0);
            let updated_time: f64 = row.try_get(8).unwrap_or(0.0);
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
        _task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = _task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time FROM memory_task_logs WHERE task_id = $1 ORDER BY updated_time DESC, id DESC LIMIT 1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let task_id: String = row.get(0);
        let user_id: String = row.get(1);
        let session_id: String = row.get(2);
        let status: String = row.get(3);
        let queued_time: f64 = row.try_get(4).unwrap_or(0.0);
        let started_time: f64 = row.try_get(5).unwrap_or(0.0);
        let finished_time: f64 = row.try_get(6).unwrap_or(0.0);
        let elapsed_s: f64 = row.try_get(7).unwrap_or(0.0);
        let request_payload: String = row.get::<_, Option<String>>(8).unwrap_or_default();
        let result: String = row.get::<_, Option<String>>(9).unwrap_or_default();
        let error: String = row.get::<_, Option<String>>(10).unwrap_or_default();
        let updated_time: f64 = row.try_get(11).unwrap_or(0.0);
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

    fn delete_memory_task_log_impl(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_task_logs_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_memory_fragment_impl(&self, record: &MemoryFragmentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_fragments (memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30) ON CONFLICT(memory_id) DO UPDATE SET user_id = EXCLUDED.user_id, agent_id = EXCLUDED.agent_id, source_session_id = EXCLUDED.source_session_id, source_round_id = EXCLUDED.source_round_id, source_type = EXCLUDED.source_type, category = EXCLUDED.category, title_l0 = EXCLUDED.title_l0, summary_l1 = EXCLUDED.summary_l1, content_l2 = EXCLUDED.content_l2, fact_key = EXCLUDED.fact_key, tags = EXCLUDED.tags, entities = EXCLUDED.entities, importance = EXCLUDED.importance, confidence = EXCLUDED.confidence, tier = EXCLUDED.tier, status = EXCLUDED.status, pinned = EXCLUDED.pinned, confirmed_by_user = EXCLUDED.confirmed_by_user, access_count = EXCLUDED.access_count, hit_count = EXCLUDED.hit_count, last_accessed_at = EXCLUDED.last_accessed_at, valid_from = EXCLUDED.valid_from, invalidated_at = EXCLUDED.invalidated_at, supersedes_memory_id = EXCLUDED.supersedes_memory_id, superseded_by_memory_id = EXCLUDED.superseded_by_memory_id, embedding_model = EXCLUDED.embedding_model, vector_ref = EXCLUDED.vector_ref, created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at",
            &[&record.memory_id, &record.user_id, &record.agent_id, &record.source_session_id, &record.source_round_id, &record.source_type, &record.category, &record.title_l0, &record.summary_l1, &record.content_l2, &record.fact_key, &Self::string_list_to_json(&record.tags), &Self::string_list_to_json(&record.entities), &record.importance, &record.confidence, &record.tier, &record.status, &record.pinned, &record.confirmed_by_user, &record.access_count, &record.hit_count, &record.last_accessed_at, &record.valid_from, &record.invalidated_at, &record.supersedes_memory_id, &record.superseded_by_memory_id, &record.embedding_model, &record.vector_ref, &record.created_at, &record.updated_at],
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
        let mut conn = self.conn()?;
        let row = conn.query_opt("SELECT memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at FROM memory_fragments WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3 LIMIT 1", &[&user_id.trim(), &agent_id.trim(), &memory_id.trim()])?;
        Ok(row.map(|row| MemoryFragmentRecord {
            memory_id: row.get(0),
            user_id: row.get(1),
            agent_id: row.get(2),
            source_session_id: row.get(3),
            source_round_id: row.get(4),
            source_type: row.get(5),
            category: row.get(6),
            title_l0: row.get(7),
            summary_l1: row.get(8),
            content_l2: row.get(9),
            fact_key: row.get(10),
            tags: Self::parse_string_list(row.get(11)),
            entities: Self::parse_string_list(row.get(12)),
            importance: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
            confidence: row.get::<_, Option<f64>>(14).unwrap_or(0.0),
            tier: row.get(15),
            status: row.get(16),
            pinned: Self::read_compat_bool(&row, 17),
            confirmed_by_user: Self::read_compat_bool(&row, 18),
            access_count: row.get::<_, Option<i64>>(19).unwrap_or(0),
            hit_count: row.get::<_, Option<i64>>(20).unwrap_or(0),
            last_accessed_at: row.get::<_, Option<f64>>(21).unwrap_or(0.0),
            valid_from: row.get::<_, Option<f64>>(22).unwrap_or(0.0),
            invalidated_at: row.get(23),
            supersedes_memory_id: row.get(24),
            superseded_by_memory_id: row.get(25),
            embedding_model: row.get(26),
            vector_ref: row.get(27),
            created_at: row.get::<_, Option<f64>>(28).unwrap_or(0.0),
            updated_at: row.get::<_, Option<f64>>(29).unwrap_or(0.0),
        }))
    }

    fn list_memory_fragments_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT memory_id, user_id, agent_id, source_session_id, source_round_id, source_type, category, title_l0, summary_l1, content_l2, fact_key, tags, entities, importance, confidence, tier, status, pinned, confirmed_by_user, access_count, hit_count, last_accessed_at, valid_from, invalidated_at, supersedes_memory_id, superseded_by_memory_id, embedding_model, vector_ref, created_at, updated_at FROM memory_fragments WHERE user_id = $1 AND agent_id = $2 ORDER BY pinned DESC, updated_at DESC, created_at DESC", &[&user_id.trim(), &agent_id.trim()])?;
        Ok(rows
            .into_iter()
            .map(|row| MemoryFragmentRecord {
                memory_id: row.get(0),
                user_id: row.get(1),
                agent_id: row.get(2),
                source_session_id: row.get(3),
                source_round_id: row.get(4),
                source_type: row.get(5),
                category: row.get(6),
                title_l0: row.get(7),
                summary_l1: row.get(8),
                content_l2: row.get(9),
                fact_key: row.get(10),
                tags: Self::parse_string_list(row.get(11)),
                entities: Self::parse_string_list(row.get(12)),
                importance: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
                confidence: row.get::<_, Option<f64>>(14).unwrap_or(0.0),
                tier: row.get(15),
                status: row.get(16),
                pinned: Self::read_compat_bool(&row, 17),
                confirmed_by_user: Self::read_compat_bool(&row, 18),
                access_count: row.get::<_, Option<i64>>(19).unwrap_or(0),
                hit_count: row.get::<_, Option<i64>>(20).unwrap_or(0),
                last_accessed_at: row.get::<_, Option<f64>>(21).unwrap_or(0.0),
                valid_from: row.get::<_, Option<f64>>(22).unwrap_or(0.0),
                invalidated_at: row.get(23),
                supersedes_memory_id: row.get(24),
                superseded_by_memory_id: row.get(25),
                embedding_model: row.get(26),
                vector_ref: row.get(27),
                created_at: row.get::<_, Option<f64>>(28).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(29).unwrap_or(0.0),
            })
            .collect())
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT memory_id, user_id, agent_id, embedding_model, content_hash, vector_json, dimensions, updated_at FROM memory_fragment_embeddings WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3 AND embedding_model = $4 AND content_hash = $5 LIMIT 1",
            &[&user_id.trim(), &agent_id.trim(), &memory_id.trim(), &embedding_model.trim(), &content_hash.trim()],
        )?;
        Ok(row.map(|row| {
            let vector_json: String = row.get(5);
            MemoryFragmentEmbeddingRecord {
                memory_id: row.get(0),
                user_id: row.get(1),
                agent_id: row.get(2),
                embedding_model: row.get(3),
                content_hash: row.get(4),
                vector: Self::json_to_f32_vec(&vector_json),
                dimensions: row.get::<_, i64>(6),
                updated_at: row.get::<_, f64>(7),
            }
        }))
    }

    fn upsert_memory_fragment_embedding_impl(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE memory_id = $1 AND embedding_model = $2 AND content_hash <> $3",
            &[&record.memory_id, &record.embedding_model, &record.content_hash],
        )?;
        let vector_json = Self::json_to_string(&Value::Array(
            record.vector.iter().map(|value| json!(value)).collect(),
        ));
        conn.execute(
            "INSERT INTO memory_fragment_embeddings (memory_id, user_id, agent_id, embedding_model, content_hash, vector_json, dimensions, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT(memory_id, embedding_model, content_hash) DO UPDATE SET user_id = EXCLUDED.user_id, agent_id = EXCLUDED.agent_id, vector_json = EXCLUDED.vector_json, dimensions = EXCLUDED.dimensions, updated_at = EXCLUDED.updated_at",
            &[&record.memory_id, &record.user_id, &record.agent_id, &record.embedding_model, &record.content_hash, &vector_json, &record.dimensions, &record.updated_at],
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
        let mut conn = self.conn()?;
        Ok(conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3",
            &[&user_id.trim(), &agent_id.trim(), &memory_id.trim()],
        )? as i64)
    }

    fn delete_memory_fragment_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let _ = conn.execute(
            "DELETE FROM memory_fragment_embeddings WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3",
            &[&user_id.trim(), &agent_id.trim(), &memory_id.trim()],
        )?;
        Ok(conn.execute(
            "DELETE FROM memory_fragments WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3",
            &[&user_id.trim(), &agent_id.trim(), &memory_id.trim()],
        )? as i64)
    }

    fn insert_memory_hit_impl(&self, record: &MemoryHitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute("INSERT INTO memory_hits (hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)", &[&record.hit_id, &record.memory_id, &record.user_id, &record.agent_id, &record.session_id, &record.round_id, &record.query_text, &Self::json_to_string(&record.reason_json), &record.lexical_score, &record.semantic_score, &record.freshness_score, &record.importance_score, &record.final_score, &record.created_at])?;
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
        let mut conn = self.conn()?;
        let rows = if let Some(session_id) =
            session_id.map(str::trim).filter(|item| !item.is_empty())
        {
            conn.query("SELECT hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at FROM memory_hits WHERE user_id = $1 AND agent_id = $2 AND session_id = $3 ORDER BY created_at DESC LIMIT $4", &[&user_id.trim(), &agent_id.trim(), &session_id, &limit.max(1)])?
        } else {
            conn.query("SELECT hit_id, memory_id, user_id, agent_id, session_id, round_id, query_text, reason_json, lexical_score, semantic_score, freshness_score, importance_score, final_score, created_at FROM memory_hits WHERE user_id = $1 AND agent_id = $2 ORDER BY created_at DESC LIMIT $3", &[&user_id.trim(), &agent_id.trim(), &limit.max(1)])?
        };
        Ok(rows
            .into_iter()
            .map(|row| MemoryHitRecord {
                hit_id: row.get(0),
                memory_id: row.get(1),
                user_id: row.get(2),
                agent_id: row.get(3),
                session_id: row.get(4),
                round_id: row.get(5),
                query_text: row.get(6),
                reason_json: Self::json_value_or_null(row.get(7)),
                lexical_score: row.get::<_, Option<f64>>(8).unwrap_or(0.0),
                semantic_score: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
                freshness_score: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
                importance_score: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                final_score: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                created_at: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
            })
            .collect())
    }

    fn list_memory_hit_counts_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT memory_id, COUNT(DISTINCT CASE
                WHEN BTRIM(round_id) <> '' THEN CONCAT(session_id, '::r::', BTRIM(round_id))
                WHEN BTRIM(query_text) <> '' THEN CONCAT(session_id, '::q::', BTRIM(query_text))
                ELSE NULL
             END) AS hit_count
             FROM memory_hits
             WHERE user_id = $1 AND agent_id = $2
             GROUP BY memory_id",
            &[&user_id.trim(), &agent_id.trim()],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, Option<i64>>(1).unwrap_or(0),
                )
            })
            .collect())
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
        let mut conn = self.conn()?;
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
            let row = conn.query_opt(
                "SELECT 1 FROM memory_hits
                 WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3 AND session_id = $4 AND round_id = $5
                 LIMIT 1",
                &[&cleaned_user, &cleaned_agent, &cleaned_memory, &cleaned_session, &cleaned_round],
            )?;
            return Ok(row.is_some());
        }

        if let Some(cleaned_query) = query_text.map(str::trim).filter(|value| !value.is_empty()) {
            let row = conn.query_opt(
                "SELECT 1 FROM memory_hits
                 WHERE user_id = $1 AND agent_id = $2 AND memory_id = $3 AND session_id = $4
                   AND BTRIM(round_id) = '' AND query_text = $5
                 LIMIT 1",
                &[
                    &cleaned_user,
                    &cleaned_agent,
                    &cleaned_memory,
                    &cleaned_session,
                    &cleaned_query,
                ],
            )?;
            return Ok(row.is_some());
        }

        Ok(false)
    }

    fn upsert_memory_job_impl(&self, record: &MemoryJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute("INSERT INTO memory_jobs (job_id, user_id, agent_id, session_id, job_type, status, request_payload, result_summary, error_message, queued_at, started_at, finished_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) ON CONFLICT(job_id) DO UPDATE SET user_id = EXCLUDED.user_id, agent_id = EXCLUDED.agent_id, session_id = EXCLUDED.session_id, job_type = EXCLUDED.job_type, status = EXCLUDED.status, request_payload = EXCLUDED.request_payload, result_summary = EXCLUDED.result_summary, error_message = EXCLUDED.error_message, queued_at = EXCLUDED.queued_at, started_at = EXCLUDED.started_at, finished_at = EXCLUDED.finished_at, updated_at = EXCLUDED.updated_at", &[&record.job_id, &record.user_id, &record.agent_id, &record.session_id, &record.job_type, &record.status, &Self::json_to_string(&record.request_payload), &record.result_summary, &record.error_message, &record.queued_at, &record.started_at, &record.finished_at, &record.updated_at])?;
        Ok(())
    }

    fn list_memory_jobs_impl(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT job_id, user_id, agent_id, session_id, job_type, status, request_payload, result_summary, error_message, queued_at, started_at, finished_at, updated_at FROM memory_jobs WHERE user_id = $1 AND agent_id = $2 ORDER BY updated_at DESC LIMIT $3", &[&user_id.trim(), &agent_id.trim(), &limit.max(1)])?;
        Ok(rows
            .into_iter()
            .map(|row| MemoryJobRecord {
                job_id: row.get(0),
                user_id: row.get(1),
                agent_id: row.get(2),
                session_id: row.get(3),
                job_type: row.get(4),
                status: row.get(5),
                request_payload: Self::json_value_or_null(row.get(6)),
                result_summary: row.get::<_, Option<String>>(7).unwrap_or_default(),
                error_message: row.get::<_, Option<String>>(8).unwrap_or_default(),
                queued_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
                started_at: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
                finished_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
            })
            .collect())
    }
}
