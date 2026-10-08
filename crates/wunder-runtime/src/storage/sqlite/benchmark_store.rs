use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, TransactionBehavior};
use serde_json::Value;

pub(super) trait SqliteBenchmarkStorage {
    fn create_benchmark_run_impl(&self, payload: &Value) -> Result<()>;
    fn update_benchmark_run_impl(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_attempt_impl(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_task_aggregate_impl(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn load_benchmark_runs_impl(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_benchmark_run_impl(&self, run_id: &str) -> Result<Option<Value>>;
    fn load_benchmark_attempts_impl(&self, run_id: &str) -> Result<Vec<Value>>;
    fn load_benchmark_task_aggregates_impl(&self, run_id: &str) -> Result<Vec<Value>>;
    fn delete_benchmark_run_impl(&self, run_id: &str) -> Result<i64>;
}

impl SqliteBenchmarkStorage for SqliteStorage {
    fn create_benchmark_run_impl(&self, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let run_id = payload
            .get("run_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if run_id.is_empty() {
            return Ok(());
        }
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let model_name = payload
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let judge_model_name = payload
            .get("judge_model_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let total_score = Self::parse_f64(payload.get("total_score")).unwrap_or(0.0);
        let started_time = Self::parse_f64(payload.get("started_time")).unwrap_or(0.0);
        let finished_time = Self::parse_f64(payload.get("finished_time")).unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO benchmark_runs (run_id, user_id, model_name, judge_model_name, status, total_score, started_time, finished_time, payload) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET user_id = excluded.user_id, model_name = excluded.model_name, \
             judge_model_name = excluded.judge_model_name, status = excluded.status, total_score = excluded.total_score, \
             started_time = excluded.started_time, finished_time = excluded.finished_time, payload = excluded.payload",
            params![
                run_id,
                user_id,
                model_name,
                judge_model_name,
                status,
                total_score,
                started_time,
                finished_time,
                payload_text
            ],
        )?;
        Ok(())
    }

    fn update_benchmark_run_impl(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut merged = payload.clone();
        if let Value::Object(ref mut map) = merged {
            map.insert("run_id".to_string(), Value::String(cleaned.to_string()));
        }
        self.create_benchmark_run_impl(&merged)
    }

    fn upsert_benchmark_attempt_impl(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let task_id = payload
            .get("task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if task_id.is_empty() {
            return Ok(());
        }
        let attempt_no = payload
            .get("attempt_no")
            .and_then(Value::as_i64)
            .or_else(|| {
                payload
                    .get("attempt_no")
                    .and_then(Value::as_u64)
                    .map(|value| value as i64)
            })
            .unwrap_or(0);
        if attempt_no <= 0 {
            return Ok(());
        }
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let final_score = Self::parse_f64(payload.get("final_score")).unwrap_or(0.0);
        let started_time = Self::parse_f64(payload.get("started_time")).unwrap_or(0.0);
        let finished_time = Self::parse_f64(payload.get("finished_time")).unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO benchmark_attempts (run_id, task_id, attempt_no, status, final_score, started_time, finished_time, payload) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id, task_id, attempt_no) DO UPDATE SET status = excluded.status, final_score = excluded.final_score, \
             started_time = excluded.started_time, finished_time = excluded.finished_time, payload = excluded.payload",
            params![
                cleaned,
                task_id,
                attempt_no,
                status,
                final_score,
                started_time,
                finished_time,
                payload_text
            ],
        )?;
        Ok(())
    }

    fn upsert_benchmark_task_aggregate_impl(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let task_id = payload
            .get("task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if task_id.is_empty() {
            return Ok(());
        }
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let mean_score = Self::parse_f64(payload.get("mean_score")).unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO benchmark_task_aggregates (run_id, task_id, status, mean_score, payload) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(run_id, task_id) DO UPDATE SET status = excluded.status, mean_score = excluded.mean_score, payload = excluded.payload",
            params![cleaned, task_id, status, mean_score, payload_text],
        )?;
        Ok(())
    }

    fn load_benchmark_runs_impl(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut conditions = Vec::new();
        let mut params: Vec<SqlValue> = Vec::new();
        if let Some(user_id) = user_id {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                conditions.push("user_id = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(status) = status {
            let cleaned = status.trim();
            if !cleaned.is_empty() {
                conditions.push("status = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(model_name) = model_name {
            let cleaned = model_name.trim();
            if !cleaned.is_empty() {
                conditions.push("model_name = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(since) = since_time {
            conditions.push("started_time >= ?".to_string());
            params.push(SqlValue::from(since));
        }
        if let Some(until) = until_time {
            conditions.push("started_time <= ?".to_string());
            params.push(SqlValue::from(until));
        }
        let mut sql = String::from("SELECT payload FROM benchmark_runs");
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY started_time DESC");
        if let Some(limit) = limit {
            if limit > 0 {
                sql.push_str(" LIMIT ?");
                params.push(SqlValue::from(limit));
            }
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_benchmark_run_impl(&self, run_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM benchmark_runs WHERE run_id = ?")?;
        let mut rows = stmt.query([cleaned])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_benchmark_attempts_impl(&self, run_id: &str) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT payload FROM benchmark_attempts WHERE run_id = ? ORDER BY task_id, attempt_no",
        )?;
        let rows = stmt
            .query_map([cleaned], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_benchmark_task_aggregates_impl(&self, run_id: &str) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT payload FROM benchmark_task_aggregates WHERE run_id = ? ORDER BY task_id",
        )?;
        let rows = stmt
            .query_map([cleaned], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn delete_benchmark_run_impl(&self, run_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let tasks_deleted = tx.execute(
            "DELETE FROM benchmark_task_aggregates WHERE run_id = ?",
            params![cleaned],
        )?;
        let attempts_deleted = tx.execute(
            "DELETE FROM benchmark_attempts WHERE run_id = ?",
            params![cleaned],
        )?;
        let runs_deleted = tx.execute(
            "DELETE FROM benchmark_runs WHERE run_id = ?",
            params![cleaned],
        )?;
        tx.commit()?;
        Ok((tasks_deleted + attempts_deleted + runs_deleted) as i64)
    }
}
