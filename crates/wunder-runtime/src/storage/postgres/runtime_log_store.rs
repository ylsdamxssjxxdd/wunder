use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::{json, Map, Value};

const PARTITION_ROWS: i64 = 1_000;
const TOTAL_METRIC_ROWS: i64 = 16_384;
const RESOURCE_ROWS_PER_THREAD: i64 = 4_096;
const TOTAL_RESOURCE_ROWS: i64 = 16_384;
const RETENTION_SECONDS: f64 = 10.0 * 24.0 * 3_600.0;

pub(super) trait PostgresRuntimeLogStorage {
    fn append_tool_metric_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_resource_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_artifact_resources_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn delete_tool_metrics_impl(&self, user_id: &str, session_id: Option<&str>) -> Result<i64>;
    fn delete_artifact_resources_impl(
        &self,
        user_id: &str,
        session_id: Option<&str>,
    ) -> Result<i64>;
}

impl PostgresRuntimeLogStorage for PostgresStorage {
    fn append_tool_metric_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        let user_id = user_id.trim();
        let session_id = payload["session_id"].as_str().unwrap_or("").trim();
        let tool = payload["tool"].as_str().unwrap_or("").trim();
        if user_id.is_empty() || session_id.is_empty() || tool.is_empty() {
            return Ok(());
        }
        self.ensure_initialized()?;
        let tool = bounded(tool, 256);
        let ok = payload["ok"].as_bool().unwrap_or(false);
        let excluded = payload["source"] == "skill_read"
            || payload["data"]["source"] == "skill_read"
            || super::super::constants::TOOL_LOG_EXCLUDED_NAMES.contains(&tool.as_str());
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
        let facts = serde_json::to_string(&facts)?;
        let created_time = Self::now_ts();
        let retention_before = created_time - RETENTION_SECONDS;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("INSERT INTO runtime_tool_metrics(user_id,session_id,tool,ok,excluded,created_time,facts) VALUES($1,$2,$3,$4,$5,$6,$7)", &[&user_id, &session_id, &tool, &ok, &excluded, &created_time, &facts])?;
        tx.execute("WITH cutoff AS (SELECT id FROM runtime_tool_metrics WHERE user_id=$1 AND session_id=$2 ORDER BY id DESC OFFSET $3 LIMIT 1) DELETE FROM runtime_tool_metrics WHERE user_id=$1 AND session_id=$2 AND id <= (SELECT id FROM cutoff)", &[&user_id, &session_id, &PARTITION_ROWS])?;
        tx.execute("WITH cutoff AS (SELECT id FROM runtime_tool_metrics ORDER BY id DESC OFFSET $1 LIMIT 1) DELETE FROM runtime_tool_metrics WHERE id <= (SELECT id FROM cutoff)", &[&TOTAL_METRIC_ROWS])?;
        tx.execute(
            "DELETE FROM runtime_tool_metrics WHERE created_time < $1",
            &[&retention_before],
        )?;
        tx.commit()
    }

    fn append_artifact_resource_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        let user_id = user_id.trim();
        let session_id = payload["session_id"].as_str().unwrap_or("").trim();
        let kind = payload["kind"].as_str().unwrap_or("").trim();
        if user_id.is_empty()
            || session_id.is_empty()
            || !matches!(kind, "file" | "script" | "image" | "attachment")
        {
            return Ok(());
        }
        self.ensure_initialized()?;
        let mut metadata = Map::new();
        metadata.insert("kind".into(), json!(kind));
        metadata.insert("session_id".into(), json!(session_id));
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
        let metadata = serde_json::to_string(&metadata)?;
        let created_time = Self::now_ts();
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("INSERT INTO runtime_artifact_resources(user_id,session_id,created_time,metadata) VALUES($1,$2,$3,$4)", &[&user_id, &session_id, &created_time, &metadata])?;
        tx.execute("WITH cutoff AS (SELECT id FROM runtime_artifact_resources WHERE user_id=$1 AND session_id=$2 ORDER BY id DESC OFFSET $3 LIMIT 1) DELETE FROM runtime_artifact_resources WHERE user_id=$1 AND session_id=$2 AND id <= (SELECT id FROM cutoff)", &[&user_id, &session_id, &RESOURCE_ROWS_PER_THREAD])?;
        tx.execute("WITH cutoff AS (SELECT id FROM runtime_artifact_resources ORDER BY id DESC OFFSET $1 LIMIT 1) DELETE FROM runtime_artifact_resources WHERE id <= (SELECT id FROM cutoff)", &[&TOTAL_RESOURCE_ROWS])?;
        tx.commit()
    }

    fn load_artifact_resources_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let user_id = user_id.trim();
        let session_id = session_id.trim();
        if user_id.is_empty() || session_id.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT id,metadata FROM runtime_artifact_resources WHERE user_id=$1 AND session_id=$2 ORDER BY id DESC LIMIT $3", &[&user_id, &session_id, &limit.min(1_000)])?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let mut value = serde_json::from_str::<Value>(&row.get::<_, String>(1))?;
            value["artifact_id"] = json!(row.get::<_, i64>(0));
            output.push(value);
        }
        output.reverse();
        Ok(output)
    }

    fn delete_tool_metrics_impl(&self, user_id: &str, session_id: Option<&str>) -> Result<i64> {
        self.ensure_initialized()?;
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let count = match session_id.map(str::trim).filter(|v| !v.is_empty()) {
            Some(session_id) => conn.execute(
                "DELETE FROM runtime_tool_metrics WHERE user_id=$1 AND session_id=$2",
                &[&user_id, &session_id],
            )?,
            None => conn.execute(
                "DELETE FROM runtime_tool_metrics WHERE user_id=$1",
                &[&user_id],
            )?,
        };
        Ok(count as i64)
    }

    fn delete_artifact_resources_impl(
        &self,
        user_id: &str,
        session_id: Option<&str>,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let count = match session_id.map(str::trim).filter(|v| !v.is_empty()) {
            Some(session_id) => conn.execute(
                "DELETE FROM runtime_artifact_resources WHERE user_id=$1 AND session_id=$2",
                &[&user_id, &session_id],
            )?,
            None => conn.execute(
                "DELETE FROM runtime_artifact_resources WHERE user_id=$1",
                &[&user_id],
            )?,
        };
        Ok(count as i64)
    }
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
fn resource_name(value: &str) -> Option<String> {
    let name = value.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty()).then(|| bounded(name, 512))
}
