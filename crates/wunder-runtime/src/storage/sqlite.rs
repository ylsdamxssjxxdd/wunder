use crate::schemas::AbilityDescriptor;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, AgentTaskRecord, AgentThreadRecord,
    BeeroomChatMessageRecord, BridgeCenterAccountRecord, BridgeCenterRecord,
    BridgeDeliveryLogRecord, BridgeRouteAuditLogRecord, BridgeUserRouteRecord,
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelMessageStats,
    ChannelOutboxRecord, ChannelOutboxStats, ChannelSessionRecord, ChannelUserBindingRecord,
    ChatSessionRecord, CronJobRecord, CronRunRecord, ExternalLinkRecord, GatewayClientRecord,
    GatewayNodeRecord, GatewayNodeTokenRecord, HiveRecord, ListBridgeCenterAccountsQuery,
    ListBridgeCentersQuery, ListBridgeDeliveryLogsQuery, ListBridgeRouteAuditLogsQuery,
    ListBridgeUserRoutesQuery, ListChannelUserBindingsQuery, MediaAssetRecord,
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, MemoryJobRecord,
    OrgUnitRecord, SessionGoalRecord, SessionLockRecord, SessionLockStatus, SessionRunRecord,
    SpeechJobRecord, TeamRunRecord, TeamTaskRecord, UpdateAgentTaskStatusParams,
    UpdateChannelOutboxStatusParams, UpsertMemoryTaskLogParams, UserAccountRecord,
    UserAgentAccessRecord, UserAgentPresetBinding, UserAgentRecord, UserExperienceUpdateResult,
    UserSessionScopeRecord, UserTokenBalanceStatus, UserTokenRecord, UserToolAccessRecord,
    UserWorldConversationRecord, UserWorldConversationSummaryRecord, UserWorldEventRecord,
    UserWorldGroupRecord, UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult,
    UserWorldSendMessageResult, VectorChunkEmbeddingRecord, VectorDocumentRecord,
    VectorDocumentSummaryRecord,
};
use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

mod agent_directory_store;
mod agent_runtime_store;
mod backend_impl;
mod benchmark_store;
mod bridge_store;
mod channel_directory;
mod channel_runtime;
mod chat_session;
mod conversation_log_store;
mod cron;
mod gateway_store;
mod log_stats_store;
mod media_store;
mod memory_store;
mod meta_store;
mod monitor_store;
mod retention_store;
mod schema;
mod session_goal;
mod session_lock_store;
mod session_run;
mod token_balance_store;
mod user_account_store;
mod user_world_store;
mod vector_document_store;

use agent_directory_store::SqliteAgentDirectoryStorage;
use agent_runtime_store::SqliteAgentRuntimeStorage;
use benchmark_store::SqliteBenchmarkStorage;
use bridge_store::SqliteBridgeStorage;
use channel_directory::SqliteChannelDirectoryStorage;
use channel_runtime::SqliteChannelRuntimeStorage;
use chat_session::SqliteChatSessionStorage;
use conversation_log_store::SqliteConversationLogStorage;
use cron::SqliteCronStorage;
use gateway_store::SqliteGatewayStorage;
use log_stats_store::SqliteLogStatsStorage;
use media_store::SqliteMediaStorage;
use memory_store::SqliteMemoryStorage;
use meta_store::SqliteMetaStorage;
use monitor_store::SqliteMonitorStorage;
use retention_store::SqliteRetentionStorage;
use schema::SqliteSchemaStorage;
use session_goal::SqliteSessionGoalStorage;
use session_lock_store::SqliteSessionLockStorage;
use session_run::SqliteSessionRunStorage;
use token_balance_store::SqliteTokenBalanceStorage;
use user_account_store::SqliteUserAccountStorage;
use user_world_store::SqliteUserWorldStorage;
use vector_document_store::SqliteVectorDocumentStorage;

pub struct SqliteStorage {
    db_path: PathBuf,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
}

impl SqliteStorage {
    pub fn new(db_path: String) -> Self {
        let path = if db_path.trim().is_empty() {
            PathBuf::from("./config/data/wunder.db")
        } else {
            PathBuf::from(db_path)
        };
        Self {
            db_path: path,
            initialized: AtomicBool::new(false),
            init_guard: Mutex::new(()),
        }
    }

    fn ensure_db_dir(&self) -> Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        self.ensure_db_dir()?;
        let conn = Connection::open(&self.db_path)?;
        // Parallel swarm workers can briefly contend on SQLite writes.
        conn.busy_timeout(Duration::from_secs(5)).ok();
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        Ok(conn)
    }

    fn now_ts() -> f64 {
        Utc::now().timestamp_millis() as f64 / 1000.0
    }

    fn json_to_string(value: &Value) -> String {
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    }

    fn json_from_str(text: &str) -> Option<Value> {
        if text.trim().is_empty() {
            return None;
        }
        serde_json::from_str::<Value>(text).ok()
    }

    fn parse_string(value: Option<&Value>) -> Option<String> {
        match value {
            Some(Value::String(text)) => Some(text.clone()),
            Some(other) => Some(other.to_string()),
            None => None,
        }
    }

    fn parse_bool(value: Option<&Value>) -> Option<i64> {
        match value {
            Some(Value::Bool(flag)) => Some(if *flag { 1 } else { 0 }),
            Some(Value::Number(num)) => num.as_i64(),
            Some(Value::String(text)) => text.parse::<i64>().ok(),
            _ => None,
        }
    }

    fn parse_f64(value: Option<&Value>) -> Option<f64> {
        match value {
            Some(Value::Number(num)) => num.as_f64(),
            Some(Value::String(text)) => text.parse::<f64>().ok(),
            Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn parse_string_list(value: Option<String>) -> Vec<String> {
        let Some(raw) = value else {
            return Vec::new();
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Ok(items) = serde_json::from_str::<Vec<String>>(trimmed) {
            return items
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect();
        }
        trimmed
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect()
    }

    fn json_to_f32_vec(text: &str) -> Vec<f32> {
        serde_json::from_str::<Vec<f32>>(text).unwrap_or_default()
    }

    fn parse_i32_list(value: Option<String>) -> Vec<i32> {
        let Some(raw) = value else {
            return Vec::new();
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Ok(items) = serde_json::from_str::<Vec<i32>>(trimmed) {
            return items.into_iter().filter(|item| *item > 0).collect();
        }
        trimmed
            .split(',')
            .filter_map(|item| item.trim().parse::<i32>().ok())
            .filter(|item| *item > 0)
            .collect()
    }

    fn string_list_to_json(list: &[String]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    }

    fn parse_declared_tool_names(value: Option<String>) -> Vec<String> {
        Self::parse_string_list(value)
    }

    fn parse_ability_items(value: Option<String>) -> Vec<AbilityDescriptor> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| serde_json::from_str::<Vec<AbilityDescriptor>>(raw).ok())
            .unwrap_or_default()
    }

    fn parse_preset_binding(value: Option<String>) -> Option<UserAgentPresetBinding> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| serde_json::from_str::<UserAgentPresetBinding>(raw).ok())
    }

    fn read_user_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserAgentRecord> {
        let tool_names = Self::parse_string_list(row.get(7)?);
        Ok(UserAgentRecord {
            agent_id: row.get(0)?,
            user_id: row.get(1)?,
            hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
            name: row.get(3)?,
            description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            system_prompt: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            preview_skill: row.get::<_, Option<i64>>(23)?.unwrap_or(0) != 0,
            model_name: row.get::<_, Option<String>>(6)?,
            ability_items: Self::parse_ability_items(row.get(10)?),
            tool_names: tool_names.clone(),
            declared_tool_names: Self::parse_declared_tool_names(row.get(8)?),
            declared_skill_names: Self::parse_string_list(row.get(9)?),
            visible_unit_ids: Self::parse_string_list(row.get(24)?),
            preset_questions: Self::parse_string_list(row.get(19)?),
            access_level: row.get(11)?,
            approval_mode: row.get(12)?,
            is_shared: row.get::<_, Option<i64>>(13)?.unwrap_or(0) != 0,
            status: row.get(14)?,
            icon: row.get(15)?,
            sandbox_container_id: normalize_sandbox_container_id(
                row.get::<_, Option<i64>>(16)?.unwrap_or(1) as i32,
            ),
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
            preset_binding: Self::parse_preset_binding(row.get(20)?),
            silent: row.get::<_, Option<i64>>(21)?.unwrap_or(0) != 0,
            prefer_mother: row.get::<_, Option<i64>>(22)?.unwrap_or(0) != 0,
        })
    }

    fn i32_list_to_json(list: &[i32]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    }

    fn json_value_or_null(value: Option<String>) -> Value {
        value
            .as_deref()
            .and_then(Self::json_from_str)
            .unwrap_or(Value::Null)
    }

    fn normalize_channel_thread_id(value: Option<&str>) -> String {
        value.unwrap_or("").trim().to_string()
    }

    fn normalize_channel_thread_value(value: Option<String>) -> Option<String> {
        value
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use chrono::Local;
    use rusqlite::params;
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_user(
        user_id: &str,
        token_balance: i64,
        token_granted_total: i64,
        token_used_total: i64,
        last_token_grant_date: Option<&str>,
    ) -> UserAccountRecord {
        UserAccountRecord {
            user_id: user_id.to_string(),
            username: user_id.to_string(),
            email: None,
            password_hash: "hash".to_string(),
            roles: vec!["user".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: None,
            token_balance,
            token_granted_total,
            token_used_total,
            last_token_grant_date: last_token_grant_date.map(str::to_string),
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn legacy_daily_quota_rows_migrate_to_token_account_fields() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("legacy-user-accounts.db");
        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE user_accounts (
                user_id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                email TEXT,
                password_hash TEXT NOT NULL,
                roles TEXT NOT NULL,
                status TEXT NOT NULL,
                access_level TEXT NOT NULL,
                unit_id TEXT,
                daily_quota INTEGER NOT NULL DEFAULT 0,
                daily_quota_used INTEGER NOT NULL DEFAULT 0,
                daily_quota_date TEXT,
                experience_total INTEGER NOT NULL DEFAULT 0,
                is_demo INTEGER NOT NULL DEFAULT 0,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL,
                last_login_at REAL
            );",
        )
        .expect("create legacy user_accounts");
        let today = Local::now().format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO user_accounts (
                user_id, username, email, password_hash, roles, status, access_level, unit_id,
                daily_quota, daily_quota_used, daily_quota_date, experience_total, is_demo,
                created_at, updated_at, last_login_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "alice",
                "alice",
                Option::<String>::None,
                "hash",
                "[\"user\"]",
                "active",
                "A",
                Option::<String>::None,
                10_000_i64,
                2_500_i64,
                today,
                0_i64,
                0_i64,
                1.0_f64,
                1.0_f64,
                Option::<f64>::None,
            ],
        )
        .expect("insert legacy row");
        drop(conn);

        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 7_500);
        assert_eq!(account.token_granted_total, 10_000);
        assert_eq!(account.token_used_total, 2_500);
        assert_eq!(
            account.last_token_grant_date.as_deref(),
            Some(today.as_str())
        );
    }

    #[test]
    fn prepare_user_token_balance_grants_once_per_day() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("prepare-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 5, 15, 2, Some("2026-04-09")))
            .expect("insert user");

        let first = storage
            .prepare_user_token_balance("alice", "2026-04-10", 100)
            .expect("prepare first")
            .expect("status");
        assert_eq!(first.balance, 105);
        assert_eq!(first.granted_total, 115);
        assert_eq!(first.used_total, 2);
        assert_eq!(first.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(first.allowed);

        let second = storage
            .prepare_user_token_balance("alice", "2026-04-10", 100)
            .expect("prepare second")
            .expect("status");
        assert_eq!(second.balance, 105);
        assert_eq!(second.granted_total, 115);
        assert_eq!(second.used_total, 2);
        assert_eq!(second.last_grant_date.as_deref(), Some("2026-04-10"));

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 105);
        assert_eq!(account.token_granted_total, 115);
        assert_eq!(account.token_used_total, 2);
    }

    #[test]
    fn consume_user_tokens_deducts_usage_and_reports_overspend() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("consume-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 50, 50, 10, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .consume_user_tokens("alice", "2026-04-10", 100, 180)
            .expect("consume tokens")
            .expect("status");
        assert_eq!(status.balance, 0);
        assert_eq!(status.granted_total, 150);
        assert_eq!(status.used_total, 190);
        assert_eq!(status.daily_grant, 100);
        assert_eq!(status.overspent_tokens, 30);
        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(!status.allowed);

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 0);
        assert_eq!(account.token_granted_total, 150);
        assert_eq!(account.token_used_total, 190);
        assert_eq!(account.last_token_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn grant_user_tokens_updates_balance_and_granted_total() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("grant-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 7, 20, 3, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .grant_user_tokens("alice", "2026-04-10", 100, 30, 123.0)
            .expect("grant tokens")
            .expect("status");
        assert_eq!(status.balance, 137);
        assert_eq!(status.granted_total, 150);
        assert_eq!(status.used_total, 3);
        assert_eq!(status.daily_grant, 100);
        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(status.allowed);

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 137);
        assert_eq!(account.token_granted_total, 150);
        assert_eq!(account.token_used_total, 3);
        assert_eq!(account.last_token_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn model_context_entries_append_and_replace_in_order() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("model-context.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        storage
            .append_model_context_entry(
                "user-a",
                "session-a",
                &json!({
                    "role": "user",
                    "content": "first"
                }),
            )
            .expect("append first");
        storage
            .append_model_context_entry(
                "user-a",
                "session-a",
                &json!({
                    "role": "assistant",
                    "content": "second"
                }),
            )
            .expect("append second");

        let loaded = storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load appended");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["content"], json!("first"));
        assert_eq!(loaded[1]["content"], json!("second"));

        storage
            .replace_model_context_entries(
                "user-a",
                "session-a",
                &[
                    json!({ "role": "user", "content": "compacted" }),
                    json!({ "role": "assistant", "content": "baseline" }),
                ],
            )
            .expect("replace entries");

        let replaced = storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load replaced");
        assert_eq!(
            replaced,
            vec![
                json!({ "role": "user", "content": "compacted" }),
                json!({ "role": "assistant", "content": "baseline" }),
            ]
        );
    }

    #[test]
    fn legacy_inline_image_payloads_are_sanitized_and_repaired_on_load() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("legacy-inline-image.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let legacy_payload = json!({
            "session_id": "session-a",
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ]
        })
        .to_string();

        {
            let conn = Connection::open(&db_path).expect("open sqlite");
            conn.execute(
                "INSERT INTO chat_history (user_id, session_id, role, payload, created_time)
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    "user-a",
                    "session-a",
                    "user",
                    legacy_payload.as_str(),
                    1.0_f64
                ],
            )
            .expect("insert legacy chat payload");
            conn.execute(
                "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time)
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    "user-a",
                    "session-a",
                    "user",
                    legacy_payload.as_str(),
                    1.0_f64
                ],
            )
            .expect("insert legacy context payload");
        }

        let chat_history = storage
            .load_chat_history("user-a", "session-a", None)
            .expect("load chat history");
        let model_context = storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load model context");

        assert_eq!(chat_history.len(), 1);
        assert_eq!(model_context.len(), 1);
        assert!(!chat_history[0]
            .to_string()
            .contains("data:image/png;base64"));
        assert!(!model_context[0]
            .to_string()
            .contains("data:image/png;base64"));
        assert!(chat_history[0].to_string().contains("inline image omitted"));
        assert!(model_context[0]
            .to_string()
            .contains("inline image omitted"));

        let conn = Connection::open(&db_path).expect("open sqlite");
        let repaired_chat: String = conn
            .query_row("SELECT payload FROM chat_history", [], |row| row.get(0))
            .expect("load repaired chat payload");
        let repaired_context: String = conn
            .query_row("SELECT payload FROM model_context_entries", [], |row| {
                row.get(0)
            })
            .expect("load repaired context payload");
        assert!(!repaired_chat.contains("data:image/png;base64"));
        assert!(!repaired_context.contains("data:image/png;base64"));
    }
}
