use super::SqliteStorage;
use crate::storage::*;
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter};
use serde_json::Value;

pub(super) trait SqliteMonitorStorage {
    fn upsert_monitor_record_impl(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record_impl(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records_impl(&self) -> Result<Vec<Value>>;
    fn load_recent_monitor_records_impl(&self, limit: i64) -> Result<Vec<Value>>;
    fn load_monitor_records_by_user_impl(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn sum_monitor_consumed_tokens_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_monitor_record_impl(&self, session_id: &str) -> Result<()>;
    fn delete_monitor_records_by_user_impl(&self, user_id: &str) -> Result<i64>;
}

impl SqliteMonitorStorage for SqliteStorage {
    fn upsert_monitor_record_impl(&self, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_id.is_empty() {
            return Ok(());
        }
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let updated_time = payload
            .get("updated_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, status = excluded.status, updated_time = excluded.updated_time, payload = excluded.payload \
             WHERE excluded.updated_time >= COALESCE(monitor_sessions.updated_time, 0)",
            params![session_id, user_id, status, updated_time, payload_text],
        )?;
        Ok(())
    }

    fn get_monitor_record_impl(&self, session_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM monitor_sessions WHERE session_id = ?")?;
        let mut rows = stmt.query([cleaned])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_monitor_records_impl(&self) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM monitor_sessions")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_recent_monitor_records_impl(&self, limit: i64) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT payload FROM monitor_sessions ORDER BY updated_time DESC LIMIT ?1")?;
        let rows = stmt
            .query_map([limit], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::with_capacity(rows.len());
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_monitor_records_by_user_impl(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let statuses = statuses
            .unwrap_or(&[])
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut clauses = vec!["user_id = ?".to_string()];
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];

        if !statuses.is_empty() {
            let placeholders = std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("status IN ({placeholders})"));
            params_list.extend(
                statuses
                    .iter()
                    .map(|value| SqlValue::from((*value).to_string())),
            );
        }
        if let Some(since) = since_time {
            clauses.push("updated_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        let where_clause = clauses.join(" AND ");
        let sql = format!(
            "SELECT payload FROM monitor_sessions WHERE {where_clause} ORDER BY updated_time DESC LIMIT ?"
        );
        params_list.push(SqlValue::from(limit));
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::with_capacity(rows.len());
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn sum_monitor_consumed_tokens_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COALESCE(SUM(CASE \
                 WHEN json_valid(payload) \
                 THEN MAX(COALESCE(CAST(json_extract(payload, '$.consumed_tokens') AS INTEGER), 0), 0) \
                 ELSE 0 END), 0) \
             FROM monitor_sessions WHERE user_id = ?",
            params![cleaned_user],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(total.max(0))
    }

    fn delete_monitor_record_impl(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        if session_id.trim().is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM monitor_sessions WHERE session_id = ?",
            params![session_id],
        )?;
        Ok(())
    }

    fn delete_monitor_records_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM monitor_sessions WHERE user_id = ?",
            params![user_id],
        )?;
        Ok(affected as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("monitor-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    #[test]
    fn monitor_store_filters_orders_and_deletes() {
        let (storage, _dir) = build_storage();

        for payload in [
            json!({
                "session_id": "session-a",
                "user_id": "user-a",
                "status": "running",
                "updated_time": 10.0,
                "consumed_tokens": 120
            }),
            json!({
                "session_id": "session-b",
                "user_id": "user-a",
                "status": "completed",
                "updated_time": 20.0,
                "consumed_tokens": 80
            }),
            json!({
                "session_id": "session-c",
                "user_id": "user-b",
                "status": "running",
                "updated_time": 30.0,
                "consumed_tokens": 40
            }),
        ] {
            storage
                .upsert_monitor_record(&payload)
                .expect("upsert monitor record");
        }

        assert_eq!(
            storage
                .get_monitor_record("session-a")
                .expect("get monitor")
                .and_then(|record| record.get("status").cloned()),
            Some(json!("running"))
        );
        assert_eq!(
            storage
                .load_recent_monitor_records(2)
                .expect("recent monitors")
                .iter()
                .map(|record| record["session_id"].as_str().unwrap_or(""))
                .collect::<Vec<_>>(),
            vec!["session-c", "session-b"]
        );
        assert_eq!(
            storage
                .load_monitor_records_by_user("user-a", Some(&["completed"]), Some(15.0), 8)
                .expect("filtered monitors")
                .iter()
                .map(|record| record["session_id"].as_str().unwrap_or(""))
                .collect::<Vec<_>>(),
            vec!["session-b"]
        );
        assert_eq!(
            storage
                .sum_monitor_consumed_tokens_by_user("user-a")
                .expect("sum consumed tokens"),
            200
        );

        storage
            .delete_monitor_record("session-a")
            .expect("delete one monitor");
        assert!(storage
            .get_monitor_record("session-a")
            .expect("get deleted monitor")
            .is_none());
        assert_eq!(
            storage
                .delete_monitor_records_by_user("user-a")
                .expect("delete user monitors"),
            1
        );
        assert_eq!(
            storage
                .load_monitor_records()
                .expect("remaining monitors")
                .len(),
            1
        );
    }

    #[test]
    fn monitor_store_ignores_stale_upsert_payloads() {
        let (storage, _dir) = build_storage();
        storage
            .upsert_monitor_record(&json!({
                "session_id": "session-a",
                "user_id": "user-a",
                "status": "running",
                "updated_time": 20.0,
                "user_rounds": 4
            }))
            .expect("upsert latest monitor record");
        storage
            .upsert_monitor_record(&json!({
                "session_id": "session-a",
                "user_id": "user-a",
                "status": "running",
                "updated_time": 10.0,
                "user_rounds": 1
            }))
            .expect("upsert stale monitor record");

        let record = storage
            .get_monitor_record("session-a")
            .expect("get monitor")
            .expect("record");
        assert_eq!(record["updated_time"], json!(20.0));
        assert_eq!(record["user_rounds"], json!(4));
    }
}
