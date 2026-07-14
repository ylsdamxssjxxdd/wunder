use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::Value;

pub(super) trait PostgresMonitorStorage {
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

impl PostgresMonitorStorage for PostgresStorage {
    fn upsert_monitor_record_impl(&self, _payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = _payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_id.is_empty() {
            return Ok(());
        }
        let user_id = _payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let status = _payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let updated_time = _payload
            .get("updated_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let payload_text = Self::json_to_string(_payload);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = EXCLUDED.user_id, status = EXCLUDED.status, updated_time = EXCLUDED.updated_time, payload = EXCLUDED.payload \
             WHERE EXCLUDED.updated_time >= monitor_sessions.updated_time",
            &[&session_id, &user_id, &status, &updated_time, &payload_text],
        )?;
        Ok(())
    }

    fn get_monitor_record_impl(&self, session_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM monitor_sessions WHERE session_id = $1",
            &[&cleaned],
        )?;
        if let Some(row) = rows.first() {
            let payload: String = row.get(0);
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_monitor_records_impl(&self) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT payload FROM monitor_sessions", &[])?;
        let mut records = Vec::new();
        for row in rows {
            let payload: String = row.get(0);
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM monitor_sessions ORDER BY updated_time DESC LIMIT $1",
            &[&limit],
        )?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.get(0);
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
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut conn = self.conn()?;
        let rows = match (!statuses.is_empty(), since_time.is_some()) {
            (true, true) => {
                let since = since_time.unwrap_or(0.0);
                conn.query(
                    "SELECT payload FROM monitor_sessions \
                     WHERE user_id = $1 AND status = ANY($2) AND updated_time >= $3 \
                     ORDER BY updated_time DESC LIMIT $4",
                    &[&cleaned_user, &statuses, &since, &limit],
                )?
            }
            (true, false) => conn.query(
                "SELECT payload FROM monitor_sessions \
                 WHERE user_id = $1 AND status = ANY($2) \
                 ORDER BY updated_time DESC LIMIT $3",
                &[&cleaned_user, &statuses, &limit],
            )?,
            (false, true) => {
                let since = since_time.unwrap_or(0.0);
                conn.query(
                    "SELECT payload FROM monitor_sessions \
                     WHERE user_id = $1 AND updated_time >= $2 \
                     ORDER BY updated_time DESC LIMIT $3",
                    &[&cleaned_user, &since, &limit],
                )?
            }
            (false, false) => conn.query(
                "SELECT payload FROM monitor_sessions WHERE user_id = $1 \
                 ORDER BY updated_time DESC LIMIT $2",
                &[&cleaned_user, &limit],
            )?,
        };
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.get(0);
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
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT COALESCE(SUM(CASE \
                 WHEN consumed_tokens ~ '^[0-9]+$' THEN consumed_tokens::BIGINT \
                 ELSE 0 END), 0)::BIGINT \
             FROM ( \
                 SELECT payload::jsonb ->> 'consumed_tokens' AS consumed_tokens \
                 FROM monitor_sessions WHERE user_id = $1 \
             ) AS monitor_usage",
            &[&cleaned_user],
        )?;
        Ok(row.get::<_, i64>(0).max(0))
    }

    fn delete_monitor_record_impl(&self, _session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = _session_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM monitor_sessions WHERE session_id = $1",
            &[&cleaned],
        )?;
        Ok(())
    }

    fn delete_monitor_records_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM monitor_sessions WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }
}
