//! Bounded diagnostic facts and independent resource metadata.
//!
//! Execution bodies belong to rollout. This store deliberately accepts only a
//! small field allowlist and never persists arguments, outputs or error text.
use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

const PARTITION_ROWS: i64 = 1_000;
const TOTAL_METRIC_ROWS: i64 = 16_384;
const RESOURCE_ROWS_PER_THREAD: i64 = 4_096;
const TOTAL_RESOURCE_ROWS: i64 = 16_384;
const RETENTION_SECONDS: f64 = 10.0 * 24.0 * 3_600.0;

pub(crate) struct RuntimeLogs {
    connection: Mutex<Connection>,
}

impl RuntimeLogs {
    pub(crate) fn connection(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.connection.lock()
    }

    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).context("create diagnostic storage directory")?;
        let conn = Connection::open(root.join("logs_1.sqlite"))?;
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-1024;
             CREATE TABLE IF NOT EXISTS runtime_tool_metrics (
                id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
                tool TEXT NOT NULL, ok INTEGER NOT NULL, excluded INTEGER NOT NULL,
                created_time REAL NOT NULL, facts TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS runtime_metrics_partition
                ON runtime_tool_metrics(user_id,session_id,id DESC);
             CREATE INDEX IF NOT EXISTS runtime_metrics_time
                ON runtime_tool_metrics(created_time);
             CREATE TABLE IF NOT EXISTS runtime_artifact_resources (
                id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
                created_time REAL NOT NULL, metadata TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS runtime_artifacts_partition
                ON runtime_artifact_resources(user_id,session_id,id DESC);",
        )?;
        Ok(Self {
            connection: Mutex::new(conn),
        })
    }

    pub fn append_tool(&self, user: &str, payload: &Value) -> Result<()> {
        let session = payload["session_id"].as_str().unwrap_or("").trim();
        let tool = payload["tool"].as_str().unwrap_or("").trim();
        if user.trim().is_empty() || session.is_empty() || tool.is_empty() {
            return Ok(());
        }
        let tool = bounded(tool, 256);
        let ok = payload["ok"].as_bool().unwrap_or(false);
        let excluded = payload["source"] == "skill_read"
            || payload["data"]["source"] == "skill_read"
            || super::constants::TOOL_LOG_EXCLUDED_NAMES.contains(&tool.as_str());
        let mut facts = Map::new();
        for key in ["user_round", "model_round", "request_context_tokens"] {
            copy_number(payload, key, &mut facts);
        }
        for key in ["duration_ms", "output_chars", "exit_code"] {
            copy_number(&payload["meta"], key, &mut facts);
        }
        let mut usage = Map::new();
        for key in [
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "reasoning_tokens",
        ] {
            copy_number(&payload["request_usage"], key, &mut usage);
        }
        if !usage.is_empty() {
            facts.insert("usage".into(), Value::Object(usage));
        }
        if !ok {
            facts.insert("failure_code".into(), json!("tool_failed"));
        }
        if excluded {
            facts.insert("excluded".into(), json!(true));
        }
        if payload["source"] == "skill_read" || payload["data"]["source"] == "skill_read" {
            facts.insert("source".into(), json!("skill_read"));
        }
        let mut conn = self.connection.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO runtime_tool_metrics(user_id,session_id,tool,ok,excluded,created_time,facts)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![user, session, tool, ok, excluded, now(), serde_json::to_string(&facts)?],
        )?;
        // Both bounds use indexed keyset cutoffs; no payload scan or full
        // history aggregation is needed after a small append.
        tx.execute(
            "DELETE FROM runtime_tool_metrics WHERE user_id=?1 AND session_id=?2 AND id <=
             (SELECT id FROM runtime_tool_metrics WHERE user_id=?1 AND session_id=?2 ORDER BY id DESC LIMIT 1 OFFSET ?3)",
            params![user, session, PARTITION_ROWS],
        )?;
        tx.execute(
            "DELETE FROM runtime_tool_metrics WHERE id <=
             (SELECT id FROM runtime_tool_metrics ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            [TOTAL_METRIC_ROWS],
        )?;
        tx.execute(
            "DELETE FROM runtime_tool_metrics WHERE created_time < ?1",
            [now() - RETENTION_SECONDS],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn append_artifact(&self, user: &str, payload: &Value) -> Result<()> {
        let session = payload["session_id"].as_str().unwrap_or("").trim();
        let kind = payload["kind"].as_str().unwrap_or("").trim();
        // Commands are execution history, not independently addressable
        // resources. Their text is already represented in the canonical item.
        if user.trim().is_empty()
            || session.is_empty()
            || !matches!(kind, "file" | "script" | "image" | "attachment")
        {
            return Ok(());
        }
        let mut metadata = Map::new();
        metadata.insert("kind".into(), json!(kind));
        metadata.insert("session_id".into(), json!(session));
        for key in ["action", "resource_id", "mime_type"] {
            if let Some(value) = payload[key].as_str() {
                metadata.insert(key.into(), json!(bounded(value, 2_048)));
            }
        }
        if let Some(name) = payload["name"].as_str().and_then(resource_name) {
            metadata.insert("name".into(), json!(name));
        }
        if let Some(ok) = payload["ok"].as_bool() {
            metadata.insert("ok".into(), json!(ok));
        }
        let mut resource_facts = Map::new();
        for key in [
            "bytes",
            "size",
            "returncode",
            "changed_files",
            "hunks_applied",
        ] {
            copy_number(&payload["meta"], key, &mut resource_facts);
        }
        if !resource_facts.is_empty() {
            metadata.insert("meta".into(), Value::Object(resource_facts));
        }
        let mut conn = self.connection.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO runtime_artifact_resources(user_id,session_id,created_time,metadata) VALUES(?1,?2,?3,?4)",
            params![user, session, now(), serde_json::to_string(&metadata)?],
        )?;
        tx.execute(
            "DELETE FROM runtime_artifact_resources WHERE id <=
             (SELECT id FROM runtime_artifact_resources ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            [TOTAL_RESOURCE_ROWS],
        )?;
        tx.execute(
            "DELETE FROM runtime_artifact_resources WHERE user_id=?1 AND session_id=?2 AND id <=
             (SELECT id FROM runtime_artifact_resources WHERE user_id=?1 AND session_id=?2 ORDER BY id DESC LIMIT 1 OFFSET ?3)",
            params![user, session, RESOURCE_ROWS_PER_THREAD],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_tools(&self, user: &str, session: Option<&str>) -> Result<i64> {
        if user.trim().is_empty() {
            return Ok(0);
        }
        Ok(self.connection.lock().execute(
            "DELETE FROM runtime_tool_metrics WHERE user_id=?1 AND (?2 IS NULL OR session_id=?2)",
            params![user, session],
        )? as i64)
    }

    pub fn delete_artifacts(&self, user: &str, session: Option<&str>) -> Result<i64> {
        if user.trim().is_empty() {
            return Ok(0);
        }
        Ok(self.connection.lock().execute(
            "DELETE FROM runtime_artifact_resources WHERE user_id=?1 AND (?2 IS NULL OR session_id=?2)",
            params![user,session],
        )? as i64)
    }

    pub fn load_artifacts(&self, user: &str, session: &str, limit: i64) -> Result<Vec<Value>> {
        if user.trim().is_empty() || session.trim().is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.connection.lock();
        let mut stmt = conn.prepare(
            "SELECT id,metadata FROM runtime_artifact_resources WHERE user_id=?1 AND session_id=?2 ORDER BY id DESC LIMIT ?3",
        )?;
        let mut rows = stmt.query(params![user, session, limit.min(1_000)])?;
        let mut output = Vec::new();
        while let Some(row) = rows.next()? {
            let mut value: Value = serde_json::from_str(&row.get::<_, String>(1)?)?;
            value["artifact_id"] = json!(row.get::<_, i64>(0)?);
            output.push(value);
        }
        output.reverse();
        Ok(output)
    }
}

/// Aggregate the rebuildable item index with ownership from the admission
/// store. Both backends keep this canonical projection below their data root.
pub(crate) fn projected_chat_stats(
    root: &Path,
) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, i64>>> {
    let conn = Connection::open_with_flags(
        root.join("thread_history_1.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(250))?;
    conn.execute(
        "ATTACH DATABASE ?1 AS control_state",
        [root.join("state_1.sqlite").to_string_lossy().as_ref()],
    )?;
    conn.execute(
        "ATTACH DATABASE ?1 AS runtime_queue",
        [root.join("queue_1.sqlite").to_string_lossy().as_ref()],
    )?;
    let mut statement = conn.prepare(
        "SELECT owner.user_id,COUNT(*),MAX(i.created_at_ms)/1000
         FROM thread_items i
         JOIN control_state.canonical_threads thread ON thread.rollout_id=i.rollout_id
         JOIN runtime_queue.canonical_thread_owners owner ON owner.thread_id=thread.thread_id
         WHERE i.item_type IN ('user_message','assistant_message')
         GROUP BY owner.user_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut output = std::collections::HashMap::new();
    for row in rows {
        let (user, records, last_time) = row?;
        output.insert(
            user,
            std::collections::HashMap::from([
                ("chat_records".to_owned(), records),
                ("last_time".to_owned(), last_time),
            ]),
        );
    }
    Ok(output)
}

/// Monitor persistence contains aggregate facts only. Full requests, replies,
/// questions and accumulated events stay out of diagnostic persistence.
pub(crate) fn monitor_facts(payload: &Value) -> Value {
    let mut fields = Map::new();
    for key in [
        "session_id",
        "user_id",
        "agent_id",
        "trace_id",
        "status",
        "stage",
    ] {
        if let Some(value) = payload[key].as_str() {
            fields.insert(key.into(), json!(bounded(value, 256)));
        }
    }
    for key in [
        "start_time",
        "updated_time",
        "ended_time",
        "elapsed_s",
        "user_rounds",
        "rounds",
        "model_round_high_water",
        "last_awarded_user_round",
        "context_tokens",
        "context_tokens_peak",
        "consumed_tokens",
        "tool_calls",
        "model_request_count",
        "quota_used",
        "next_event_id",
    ] {
        copy_number(payload, key, &mut fields);
    }
    for key in ["is_admin", "cancel_requested"] {
        if let Some(value) = payload[key].as_bool() {
            fields.insert(key.into(), json!(value));
        }
    }
    if let Some(rounds) = payload["model_round_high_water"].as_object() {
        let mut selected = rounds
            .iter()
            .filter_map(|(round, value)| Some((round.parse::<i64>().ok()?, value.as_i64()?)))
            .filter(|(round, value)| *round > 0 && *value >= 0)
            .collect::<Vec<_>>();
        selected.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        fields.insert(
            "model_round_high_water".into(),
            Value::Object(
                selected
                    .into_iter()
                    .take(64)
                    .map(|(round, value)| (round.to_string(), json!(value)))
                    .collect(),
            ),
        );
    }
    Value::Object(fields)
}

fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1_000.0
}

fn bounded(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn copy_number(source: &Value, key: &str, target: &mut Map<String, Value>) {
    if source[key].is_number() {
        target.insert(key.into(), source[key].clone());
    }
}

/// A diagnostic/resource index must not retain the caller's local path. File
/// names are enough to identify the resource in its owning workspace.
fn resource_name(value: &str) -> Option<String> {
    let name = value.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty()).then(|| bounded(name, 512))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_facts_keep_bounded_round_counters_without_execution_content() {
        let rounds = (1..=80)
            .map(|round| (round.to_string(), json!(round + 1)))
            .collect::<Map<_, _>>();
        let facts = monitor_facts(&json!({
            "session_id":"sample-thread", "user_rounds":80,
            "model_round_high_water":rounds,
            "question":"private-input", "events":[{"content":"private-output"}],
            "request":{"secret":"private-request"}
        }));
        let rounds = facts["model_round_high_water"].as_object().unwrap();
        assert_eq!(rounds.len(), 64);
        assert_eq!(rounds["80"], json!(81));
        assert!(!rounds.contains_key("1"));
        assert!(!facts.to_string().contains("private-"));
    }

    #[test]
    fn diagnostic_store_never_copies_execution_bodies_and_enforces_partition_limit() {
        let root = tempfile::tempdir().unwrap();
        let logs = RuntimeLogs::open(root.path()).unwrap();
        let payload = json!({"session_id":"fixture-thread","tool":"fixture-tool","ok":false,
            "args":{"body":"private-input"},"data":{"body":"private-output"},
            "error":"private-error", "meta":{"duration_ms":12,"content":"private-meta"}});
        for _ in 0..PARTITION_ROWS + 2 {
            logs.append_tool("fixture-user", &payload).unwrap();
        }
        let conn = logs.connection.lock();
        let (count, bodies): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), group_concat(facts) FROM runtime_tool_metrics",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, PARTITION_ROWS);
        assert!(!bodies.contains("private-"));
        assert!(bodies.contains("duration_ms"));
        drop(conn);
        logs.append_artifact("fixture-user", &json!({"session_id":"fixture-thread","kind":"file","name":"fixture.txt", "content":"private-content", "meta":{"bytes":7,"body":"private-meta"}})).unwrap();
        logs.append_artifact(
            "fixture-user",
            &json!({"session_id":"fixture-thread","kind":"command","name":"private-command"}),
        )
        .unwrap();
        let resources = logs
            .load_artifacts("fixture-user", "fixture-thread", 10)
            .unwrap();
        assert_eq!(resources.len(), 1);
        assert!(!serde_json::to_string(&resources)
            .unwrap()
            .contains("private-"));
    }
}
