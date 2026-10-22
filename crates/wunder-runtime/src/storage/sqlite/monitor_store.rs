use super::SqliteStorage;
use crate::storage::*;
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::Value;

pub(super) trait SqliteMonitorStorage {
    fn upsert_monitor_record_impl(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record_impl(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records_by_session_ids_impl(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<Value>>;
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
    fn sum_monitor_quota_used_by_day_impl(
        &self,
        user_id: &str,
        since_at: f64,
    ) -> Result<Vec<(String, i64)>>;
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
        let payload_text =
            Self::json_to_string(&crate::storage::runtime_logs::monitor_facts(payload));
        let mut conn = self.open_logs()?;
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, status = excluded.status, updated_time = excluded.updated_time, payload = excluded.payload \
             WHERE excluded.updated_time >= COALESCE(monitor_sessions.updated_time, 0)",
            params![session_id, user_id, status, updated_time, payload_text],
        )?;
        if changed > 0 {
            tx.execute(
                "INSERT INTO monitor_session_stats(session_id,payload) VALUES(?,?) ON CONFLICT(session_id) DO UPDATE SET payload=excluded.payload",
                params![session_id, serde_json::to_string(&MonitorSessionStats::from_payload(payload))?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get_monitor_record_impl(&self, session_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.read_logs()?;
        let mut stmt =
            conn.prepare_cached("SELECT payload FROM monitor_sessions WHERE session_id = ?")?;
        let mut rows = stmt.query([cleaned])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_monitor_records_by_session_ids_impl(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let ids = session_ids
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql =
            format!("SELECT payload FROM monitor_sessions WHERE session_id IN ({placeholders})");
        let conn = self.open_logs()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(ids.iter().copied()), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|payload| Self::json_from_str(&payload))
            .collect())
    }

    fn load_monitor_records_impl(&self) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open_logs()?;
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
        let conn = self.open_logs()?;
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
        let conn = self.open_logs()?;
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
        let conn = self.open_logs()?;
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

    fn sum_monitor_quota_used_by_day_impl(
        &self,
        user_id: &str,
        since_at: f64,
    ) -> Result<Vec<(String, i64)>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || !since_at.is_finite() || since_at <= 0.0 {
            return Ok(Vec::new());
        }
        let conn = self.open_logs()?;
        let mut stmt = conn.prepare(
            "SELECT strftime('%Y-%m-%d', updated_time, 'unixepoch', 'localtime'), \
                 COALESCE(SUM(CASE \
                     WHEN json_valid(payload) \
                     THEN MAX(COALESCE(CAST(json_extract(payload, '$.quota_used') AS INTEGER), 0), 0) \
                     ELSE 0 END), 0) \
             FROM monitor_sessions WHERE user_id = ? AND updated_time >= ? \
             GROUP BY 1 ORDER BY 1",
        )?;
        let rows = stmt.query_map(params![cleaned_user, since_at], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut days = Vec::new();
        for row in rows {
            let (day, credits) = row?;
            if !day.is_empty() && credits > 0 {
                days.push((day, credits));
            }
        }
        Ok(days)
    }

    fn delete_monitor_record_impl(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        if session_id.trim().is_empty() {
            return Ok(());
        }
        let conn = self.open_logs()?;
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
        let conn = self.open_logs()?;
        let affected = conn.execute(
            "DELETE FROM monitor_sessions WHERE user_id = ?",
            params![user_id],
        )?;
        Ok(affected as i64)
    }
}

impl SqliteStorage {
    pub(super) fn monitor_session_stats(
        &self,
        session: &str,
    ) -> Result<Option<MonitorSessionStats>> {
        self.ensure_initialized()?;
        let conn = self.read_logs()?;
        let stored: Option<String> = conn
            .prepare_cached("SELECT payload FROM monitor_session_stats WHERE session_id=?")?
            .query_row([session.trim()], |row| row.get(0))
            .optional()?;
        if let Some(stored) = stored {
            return Ok(Some(serde_json::from_str(&stored)?));
        }
        // Let the monitor's on-demand compatibility decoder handle records
        // written before this small projection existed. It retains counters
        // derived from legacy events without a startup-wide backfill.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use chrono::{Local, TimeZone};
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

    #[test]
    fn navigation_stats_are_small_atomic_and_cleaned_with_the_monitor() {
        let (db, _dir) = build_storage();
        let mut payload = json!({
            "session_id":"fixture-thread", "status":"completed", "updated_time":2.0,
            "consumed_tokens":120, "tool_calls":7, "model_request_count":3,
            "events":[{"data":{"content":"fixture text ".repeat(100_000)}}]
        });
        db.upsert_monitor_record(&payload).unwrap();
        let expected = MonitorSessionStats::from_payload(&payload);
        assert_eq!(
            db.get_monitor_session_stats("fixture-thread").unwrap(),
            Some(expected.clone())
        );
        let conn = db.open_logs().unwrap();
        let size: i64 = conn.query_row("SELECT length(payload) FROM monitor_session_stats WHERE session_id='fixture-thread'", [], |r| r.get(0)).unwrap();
        assert!(size < 200);
        payload["updated_time"] = json!(1.0);
        payload["tool_calls"] = json!(0);
        db.upsert_monitor_record(&payload).unwrap();
        assert_eq!(
            db.get_monitor_session_stats("fixture-thread").unwrap(),
            Some(expected.clone())
        );
        // An old record is readable immediately, without a startup backfill.
        conn.execute("DELETE FROM monitor_session_stats", [])
            .unwrap();
        assert_eq!(
            db.get_monitor_session_stats("fixture-thread").unwrap(),
            None
        );
        payload["updated_time"] = json!(3.0);
        db.upsert_monitor_record(&payload).unwrap();
        // All cleanup paths deleting the source also remove its projection.
        conn.execute("DELETE FROM monitor_sessions", []).unwrap();
        assert_eq!(
            db.get_monitor_session_stats("fixture-thread").unwrap(),
            None
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM monitor_session_stats", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn quota_used_buckets_by_local_calendar_day() {
        let (storage, _dir) = build_storage();
        let day = |offset: i64| -> (f64, String) {
            let naive = (Local::now().date_naive() - chrono::Duration::days(offset))
                .and_hms_opt(9, 0, 0)
                .expect("local day");
            let at = Local
                .from_local_datetime(&naive)
                .single()
                .expect("unambiguous local day");
            (at.timestamp() as f64, naive.format("%Y-%m-%d").to_string())
        };
        let (today_at, today_key) = day(0);
        let (yesterday_at, yesterday_key) = day(1);
        let (long_ago_at, _) = day(400);

        for (session_id, user_id, updated_at, credits) in [
            ("session-today-a", "user-a", today_at, 12),
            ("session-today-b", "user-a", today_at, 8),
            ("session-yesterday", "user-a", yesterday_at, 4),
            ("session-long-ago", "user-a", long_ago_at, 99),
            ("session-other-user", "user-b", today_at, 7),
        ] {
            storage
                .upsert_monitor_record(&json!({
                    "session_id": session_id,
                    "user_id": user_id,
                    "status": "completed",
                    "updated_time": updated_at,
                    "quota_used": credits
                }))
                .expect("upsert monitor record");
        }

        let (since_at, _) = day(30);
        assert_eq!(
            storage
                .sum_monitor_quota_used_by_day("user-a", since_at)
                .expect("daily credits"),
            vec![(yesterday_key, 4_i64), (today_key, 20_i64)]
        );
        assert!(storage
            .sum_monitor_quota_used_by_day("  ", since_at)
            .expect("blank user")
            .is_empty());
        assert!(storage
            .sum_monitor_quota_used_by_day("user-a", -1.0)
            .expect("unset window")
            .is_empty());
    }
}
