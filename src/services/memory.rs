// 长期记忆存储：基于持久化存储封装读写与提示构建。
use crate::i18n;
use crate::storage::StorageBackend;
use chrono::{Datelike, Local, TimeZone, Timelike};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, warn};

const DEFAULT_MAX_RECORDS: i64 = 30;

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecord {
    pub session_id: String,
    pub summary: String,
    pub created_time: f64,
    pub updated_time: f64,
}

#[derive(Debug, Clone)]
pub struct MemorySetting {
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct MemoryRecordStat {
    pub record_count: i64,
    pub last_time: f64,
}

pub struct MemoryStore {
    storage: Arc<dyn StorageBackend>,
    max_records: i64,
}

impl MemoryStore {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            max_records: DEFAULT_MAX_RECORDS,
        }
    }

    pub fn normalize_summary(text: &str) -> String {
        let raw = text.trim();
        if raw.is_empty() {
            return String::new();
        }
        if let Some(tagged) = extract_tagged_summary(raw) {
            if let Some(parsed) = parse_summary_payload(&tagged) {
                return parsed;
            }
            return tagged;
        }
        if let Some(parsed) = parse_summary_payload(raw) {
            return parsed;
        }
        let mut segments = Vec::new();
        let bullet_re = bullet_regex();
        for line in raw.lines() {
            let cleaned = match bullet_re {
                Some(regex) => regex.replace(line.trim(), "").trim().to_string(),
                None => line.trim().to_string(),
            };
            if !cleaned.is_empty() {
                segments.push(cleaned);
            }
        }
        join_segments(&segments)
    }

    pub fn build_prompt_block(&self, records: &[MemoryRecord]) -> String {
        if records.is_empty() {
            return String::new();
        }
        let mut chunks = Vec::new();
        for record in records {
            let summary = Self::normalize_summary(&record.summary);
            if summary.is_empty() {
                continue;
            }
            let prefix_ts = if record.updated_time > 0.0 {
                record.updated_time
            } else {
                record.created_time
            };
            let prefix = format_memory_time_prefix(prefix_ts);
            if prefix.is_empty() {
                chunks.push(summary);
            } else {
                chunks.push(format!("{prefix} {summary}"));
            }
        }
        if chunks.is_empty() {
            return String::new();
        }
        format!("{}\n{}", i18n::t("memory.block_prefix"), chunks.join("\n"))
    }

    pub async fn is_enabled_async(&self, user_id: &str) -> bool {
        let user_id = user_id.to_string();
        let storage = self.storage.clone();
        let max_records = self.max_records;
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.is_enabled(&user_id)
        })
        .await
        .unwrap_or_else(|err| {
            warn!("memory is_enabled async failed: {err}");
            false
        })
    }

    pub fn is_enabled(&self, user_id: &str) -> bool {
        self.storage
            .get_memory_enabled(user_id)
            .unwrap_or(None)
            .unwrap_or(false)
    }

    pub fn set_enabled(&self, user_id: &str, enabled: bool) {
        if let Err(err) = self.storage.set_memory_enabled(user_id, enabled) {
            warn!("memory set_enabled failed for {user_id}: {err}");
        }
    }

    pub fn list_settings(&self) -> HashMap<String, MemorySetting> {
        let rows = self.storage.load_memory_settings().unwrap_or_default();
        let mut output = HashMap::new();
        for row in rows {
            let user_id = row
                .get("user_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if user_id.is_empty() {
                continue;
            }
            let enabled = row.get("enabled").and_then(parse_bool).unwrap_or(false);
            output.insert(user_id.to_string(), MemorySetting { enabled });
        }
        output
    }

    pub fn list_record_stats(&self) -> HashMap<String, MemoryRecordStat> {
        let rows = self.storage.get_memory_record_stats().unwrap_or_default();
        let mut output = HashMap::new();
        for row in rows {
            let user_id = row
                .get("user_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if user_id.is_empty() {
                continue;
            }
            let record_count = row.get("record_count").and_then(Value::as_i64).unwrap_or(0);
            let last_time = row.get("last_time").and_then(Value::as_f64).unwrap_or(0.0);
            output.insert(
                user_id.to_string(),
                MemoryRecordStat {
                    record_count,
                    last_time,
                },
            );
        }
        output
    }

    pub fn list_records(
        &self,
        user_id: &str,
        limit: Option<i64>,
        order_desc: bool,
    ) -> Vec<MemoryRecord> {
        let safe_limit = limit.unwrap_or(self.max_records);
        let rows = self
            .storage
            .load_memory_records(user_id, safe_limit, order_desc)
            .unwrap_or_default();
        rows.into_iter()
            .map(|row| MemoryRecord {
                session_id: row
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                summary: row
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                created_time: row
                    .get("created_time")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                updated_time: row
                    .get("updated_time")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            })
            .collect()
    }

    pub async fn list_records_async(
        &self,
        user_id: &str,
        limit: Option<i64>,
        order_desc: bool,
    ) -> Vec<MemoryRecord> {
        let user_id = user_id.to_string();
        let storage = self.storage.clone();
        let max_records = self.max_records;
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.list_records(&user_id, limit, order_desc)
        })
        .await
        .unwrap_or_else(|err| {
            warn!("memory list_records async failed: {err}");
            Vec::new()
        })
    }

    pub fn list_task_logs(&self, limit: Option<i64>) -> Vec<HashMap<String, Value>> {
        let rows = self
            .storage
            .load_memory_task_logs(limit)
            .unwrap_or_default();
        rows.into_iter().map(format_task_log).collect()
    }

    pub async fn list_task_logs_async(&self, limit: Option<i64>) -> Vec<HashMap<String, Value>> {
        let storage = self.storage.clone();
        let max_records = self.max_records;
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.list_task_logs(limit)
        })
        .await
        .unwrap_or_else(|err| {
            warn!("memory list_task_logs async failed: {err}");
            Vec::new()
        })
    }

    pub fn get_task_log(&self, task_id: &str) -> Option<HashMap<String, Value>> {
        let row = self
            .storage
            .load_memory_task_log_by_task_id(task_id)
            .ok()
            .flatten()?;
        let mut payload = format_task_log(row.clone());
        let request_payload = row
            .get("request_payload")
            .and_then(Value::as_str)
            .unwrap_or("");
        payload.insert(
            "request".to_string(),
            json_to_value(parse_task_request(request_payload)),
        );
        payload.insert(
            "result".to_string(),
            Value::String(
                row.get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
        );
        payload.insert(
            "error".to_string(),
            Value::String(
                row.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
        );
        Some(payload)
    }

    pub async fn get_task_log_async(&self, task_id: &str) -> Option<HashMap<String, Value>> {
        let task_id = task_id.to_string();
        let storage = self.storage.clone();
        let max_records = self.max_records;
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.get_task_log(&task_id)
        })
        .await
        .unwrap_or_else(|err| {
            warn!("memory get_task_log async failed: {err}");
            None
        })
    }

    pub fn upsert_task_log(
        &self,
        user_id: &str,
        session_id: &str,
        task_id: &str,
        status: &str,
        queued_time: f64,
        started_time: f64,
        finished_time: f64,
        elapsed_s: f64,
        request_payload: Option<&Value>,
        result: &str,
        error: &str,
        now_ts: Option<f64>,
    ) {
        if let Err(err) = self.storage.upsert_memory_task_log(
            user_id,
            session_id,
            task_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            request_payload,
            result,
            error,
            now_ts,
        ) {
            warn!("memory upsert task log failed for {user_id}/{session_id}: {err}");
        }
    }

    pub async fn upsert_task_log_async(
        &self,
        user_id: &str,
        session_id: &str,
        task_id: &str,
        status: &str,
        queued_time: f64,
        started_time: f64,
        finished_time: f64,
        elapsed_s: f64,
        request_payload: Option<&Value>,
        result: &str,
        error: &str,
        now_ts: Option<f64>,
    ) {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let task_id = task_id.to_string();
        let status = status.to_string();
        let result = result.to_string();
        let error = error.to_string();
        let request_payload = request_payload.cloned();
        let storage = self.storage.clone();
        let max_records = self.max_records;
        let _ = tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.upsert_task_log(
                &user_id,
                &session_id,
                &task_id,
                &status,
                queued_time,
                started_time,
                finished_time,
                elapsed_s,
                request_payload.as_ref(),
                &result,
                &error,
                now_ts,
            );
        })
        .await;
    }

    pub fn upsert_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        now_ts: Option<f64>,
        max_records_override: Option<i64>,
    ) -> bool {
        let normalized = Self::normalize_summary(summary);
        if normalized.is_empty() {
            return false;
        }
        let now = now_ts.unwrap_or_else(|| now_ts_value());
        let max_records = max_records_override.unwrap_or(self.max_records);
        if let Err(err) =
            self.storage
                .upsert_memory_record(user_id, session_id, &normalized, max_records, now)
        {
            warn!("memory upsert record failed for {user_id}/{session_id}: {err}");
        }
        true
    }

    pub async fn upsert_record_async(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        now_ts: Option<f64>,
        max_records_override: Option<i64>,
    ) -> bool {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let summary = summary.to_string();
        let storage = self.storage.clone();
        let max_records = self.max_records;
        tokio::task::spawn_blocking(move || {
            let store = MemoryStore {
                storage,
                max_records,
            };
            store.upsert_record(
                &user_id,
                &session_id,
                &summary,
                now_ts,
                max_records_override,
            )
        })
        .await
        .unwrap_or_else(|err| {
            warn!("memory upsert_record async failed: {err}");
            false
        })
    }

    pub fn update_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        now_ts: Option<f64>,
    ) -> bool {
        self.upsert_record(user_id, session_id, summary, now_ts, None)
    }

    pub fn delete_record(&self, user_id: &str, session_id: &str) -> i64 {
        let deleted = self
            .storage
            .delete_memory_record(user_id, session_id)
            .unwrap_or(0);
        let _ = self.storage.delete_memory_task_log(user_id, session_id);
        deleted
    }

    pub fn clear_records(&self, user_id: &str) -> i64 {
        let deleted = self
            .storage
            .delete_memory_records_by_user(user_id)
            .unwrap_or(0);
        let _ = self.storage.delete_memory_task_logs_by_user(user_id);
        deleted
    }
}

fn extract_tagged_summary(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let regex = tagged_summary_regex()?;
    let mut parts = Vec::new();
    for caps in regex.captures_iter(trimmed) {
        if let Some(content) = caps.get(1) {
            let cleaned = content.as_str().trim();
            if !cleaned.is_empty() {
                parts.push(cleaned.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn parse_summary_payload(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return None;
    };
    let values = match value {
        Value::Object(map) => map.values().cloned().collect::<Vec<_>>(),
        Value::Array(items) => items,
        _ => return None,
    };
    let mut segments = Vec::new();
    for item in values {
        let cleaned = match item {
            Value::String(text) => text.trim().to_string(),
            other => other.to_string().trim().to_string(),
        };
        if !cleaned.is_empty() && cleaned != "null" {
            segments.push(cleaned);
        }
    }
    Some(join_segments(&segments))
}

fn join_segments(segments: &[String]) -> String {
    if segments.is_empty() {
        return String::new();
    }
    if segments.len() == 1 {
        return segments[0].clone();
    }
    segments.join("；").trim().to_string()
}

fn format_memory_time_prefix(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() else {
        return String::new();
    };
    let params = HashMap::from([
        ("year".to_string(), dt.year().to_string()),
        ("month".to_string(), dt.month().to_string()),
        ("day".to_string(), dt.day().to_string()),
        ("hour".to_string(), dt.hour().to_string()),
        ("minute".to_string(), dt.minute().to_string()),
    ]);
    i18n::t_with_params("memory.time_prefix", &params)
}

fn format_task_log(item: HashMap<String, Value>) -> HashMap<String, Value> {
    let queued_ts = item
        .get("queued_time")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let started_ts = item
        .get("started_time")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let finished_ts = item
        .get("finished_time")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let status_raw = item.get("status").and_then(Value::as_str).unwrap_or("");
    let status_lower = status_raw.to_lowercase();
    let normalized = match status_lower.as_str() {
        "queued" | "排队中" => "queued",
        "running" | "processing" | "正在处理" => "running",
        "done" | "completed" | "已完成" => "done",
        "failed" | "失败" => "failed",
        _ => "",
    };
    let status = match normalized {
        "queued" => i18n::t("memory.status.queued"),
        "running" => i18n::t("memory.status.running"),
        "done" => i18n::t("memory.status.done"),
        "failed" => i18n::t("memory.status.failed"),
        _ => status_raw.to_string(),
    };
    let mut payload = HashMap::new();
    payload.insert(
        "task_id".to_string(),
        item.get("task_id")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    payload.insert(
        "user_id".to_string(),
        item.get("user_id")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    payload.insert(
        "session_id".to_string(),
        item.get("session_id")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    payload.insert("status".to_string(), Value::String(status));
    payload.insert(
        "queued_time".to_string(),
        Value::String(format_ts(queued_ts)),
    );
    payload.insert("queued_time_ts".to_string(), json_to_value(queued_ts));
    payload.insert(
        "started_time".to_string(),
        Value::String(format_ts(started_ts)),
    );
    payload.insert("started_time_ts".to_string(), json_to_value(started_ts));
    payload.insert(
        "finished_time".to_string(),
        Value::String(format_ts(finished_ts)),
    );
    payload.insert("finished_time_ts".to_string(), json_to_value(finished_ts));
    payload.insert(
        "elapsed_s".to_string(),
        item.get("elapsed_s").cloned().unwrap_or(Value::Null),
    );
    payload
}

fn format_ts(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let dt = Local.timestamp_opt(ts as i64, 0).single();
    dt.map(|value| value.to_rfc3339()).unwrap_or_default()
}

fn parse_task_request(payload_text: &str) -> HashMap<String, Value> {
    let trimmed = payload_text.trim();
    if trimmed.is_empty() {
        return HashMap::new();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => map.into_iter().map(|(k, v)| (k, v)).collect(),
        _ => HashMap::new(),
    }
}

fn parse_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(num) => num.as_i64().map(|value| value != 0),
        Value::String(text) => text.parse::<i64>().ok().map(|value| value != 0),
        _ => None,
    }
}

fn json_to_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn now_ts_value() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn bullet_regex() -> Option<&'static Regex> {
    static REGEX: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"^[-*\u2022]\s*", "bullet"))
        .as_ref()
}

fn tagged_summary_regex() -> Option<&'static Regex> {
    static REGEX: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"(?is)<memory_summary>(.*?)</memory_summary>", "summary"))
        .as_ref()
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid memory regex {label}: {err}");
            None
        }
    }
}
