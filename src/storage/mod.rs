// 存储模块：封装 SQLite/Postgres 持久化读写，提供统一的历史/监控/记忆接口。

mod bridge;
mod postgres;
mod sqlite;

use crate::config::StorageConfig;
use crate::schemas::AbilityDescriptor;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use bridge::*;
pub use postgres::PostgresStorage;
pub use sqlite::SqliteStorage;

pub(crate) const TOOL_LOG_SKILL_READ_MARKER: &str = "\"source\":\"skill_read\"";

pub(crate) const TOOL_LOG_EXCLUDED_NAMES: &[&str] = &[
    "final_response",
    "最终回复",
    "update_plan",
    "sessions_yield",
    "yield",
    "会话让出",
    "计划面板",
    "question_panel",
    "ask_panel",
    "问询面板",
    "a2ui",
    "a2a_observe",
    "a2a_wait",
    "a2a观察",
    "a2a等待",
    "performance_log",
];

pub const USER_PRIVATE_CONTAINER_ID: i32 = 0;
pub const DEFAULT_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MIN_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MAX_SANDBOX_CONTAINER_ID: i32 = 10;
pub const DEFAULT_HIVE_ID: &str = "default";

pub fn normalize_sandbox_container_id(value: i32) -> i32 {
    value.clamp(MIN_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

pub fn normalize_workspace_container_id(value: i32) -> i32 {
    value.clamp(USER_PRIVATE_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

pub fn normalize_hive_id(value: &str) -> String {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return DEFAULT_HIVE_ID.to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch.to_ascii_lowercase());
        }
    }
    if output.is_empty() {
        DEFAULT_HIVE_ID.to_string()
    } else {
        output
    }
}

#[derive(Debug, Clone)]
pub struct UserAccountRecord {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub roles: Vec<String>,
    pub status: String,
    pub access_level: String,
    pub unit_id: Option<String>,
    pub token_balance: i64,
    pub token_granted_total: i64,
    pub token_used_total: i64,
    pub last_token_grant_date: Option<String>,
    pub experience_total: i64,
    pub is_demo: bool,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_login_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct OrgUnitRecord {
    pub unit_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub level: i32,
    pub path: String,
    pub path_name: String,
    pub sort_order: i64,
    pub leader_ids: Vec<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserTokenBalanceStatus {
    pub balance: i64,
    pub granted_total: i64,
    pub used_total: i64,
    pub daily_grant: i64,
    pub last_grant_date: Option<String>,
    pub allowed: bool,
    pub overspent_tokens: i64,
}

#[derive(Debug, Clone)]
pub struct UserExperienceUpdateResult {
    pub previous_total: i64,
    pub current_total: i64,
}

#[derive(Debug, Clone)]
pub struct UserTokenRecord {
    pub token: String,
    pub user_id: String,
    pub session_scope: String,
    pub expires_at: f64,
    pub created_at: f64,
    pub last_used_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserSessionScopeRecord {
    pub user_id: String,
    pub session_scope: String,
    pub last_login_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentPresetSnapshot {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub ability_items: Vec<AbilityDescriptor>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub status: String,
    pub icon: Option<String>,
    pub sandbox_container_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentPresetBinding {
    pub preset_id: String,
    #[serde(default)]
    pub preset_revision: u64,
    pub last_applied: UserAgentPresetSnapshot,
}

#[derive(Debug, Clone)]
pub struct UserToolAccessRecord {
    pub user_id: String,
    pub allowed_tools: Option<Vec<String>>,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserAgentRecord {
    pub agent_id: String,
    pub user_id: String,
    pub hive_id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: Option<String>,
    pub ability_items: Vec<AbilityDescriptor>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub access_level: String,
    pub approval_mode: String,
    pub is_shared: bool,
    pub status: String,
    pub icon: Option<String>,
    pub sandbox_container_id: i32,
    pub created_at: f64,
    pub updated_at: f64,
    pub preset_binding: Option<UserAgentPresetBinding>,
    pub silent: bool,
    pub prefer_mother: bool,
}

#[derive(Debug, Clone)]
pub struct HiveRecord {
    pub hive_id: String,
    pub user_id: String,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub status: String,
    pub created_time: f64,
    pub updated_time: f64,
}

#[derive(Debug, Clone)]
pub struct TeamRunRecord {
    pub team_run_id: String,
    pub user_id: String,
    pub hive_id: String,
    pub parent_session_id: String,
    pub parent_agent_id: Option<String>,
    pub mother_agent_id: Option<String>,
    pub strategy: String,
    pub status: String,
    pub task_total: i64,
    pub task_success: i64,
    pub task_failed: i64,
    pub context_tokens_total: i64,
    pub context_tokens_peak: i64,
    pub model_round_total: i64,
    pub started_time: Option<f64>,
    pub finished_time: Option<f64>,
    pub elapsed_s: Option<f64>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub updated_time: f64,
}

#[derive(Debug, Clone)]
pub struct TeamTaskRecord {
    pub task_id: String,
    pub team_run_id: String,
    pub user_id: String,
    pub hive_id: String,
    pub agent_id: String,
    pub target_session_id: Option<String>,
    pub spawned_session_id: Option<String>,
    pub session_run_id: Option<String>,
    pub status: String,
    pub retry_count: i64,
    pub priority: i64,
    pub started_time: Option<f64>,
    pub finished_time: Option<f64>,
    pub elapsed_s: Option<f64>,
    pub result_summary: Option<String>,
    pub error: Option<String>,
    pub updated_time: f64,
}

#[derive(Debug, Clone)]
pub struct UserAgentAccessRecord {
    pub user_id: String,
    pub allowed_agent_ids: Option<Vec<String>>,
    pub blocked_agent_ids: Vec<String>,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ExternalLinkRecord {
    pub link_id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub icon: String,
    pub allowed_levels: Vec<i32>,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct VectorDocumentRecord {
    pub doc_id: String,
    pub owner_id: String,
    pub base_name: String,
    pub doc_name: String,
    pub embedding_model: String,
    pub chunk_size: i64,
    pub chunk_overlap: i64,
    pub chunk_count: i64,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub content: String,
    pub chunks_json: String,
}

#[derive(Debug, Clone)]
pub struct VectorDocumentSummaryRecord {
    pub doc_id: String,
    pub doc_name: String,
    pub status: String,
    pub chunk_count: i64,
    pub embedding_model: String,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChatSessionRecord {
    pub session_id: String,
    pub user_id: String,
    pub title: String,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub agent_id: Option<String>,
    pub tool_overrides: Vec<String>,
    pub parent_session_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub spawn_label: Option<String>,
    pub spawned_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserWorldConversationRecord {
    pub conversation_id: String,
    pub conversation_type: String,
    pub participant_a: String,
    pub participant_b: String,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub member_count: Option<i64>,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserWorldConversationSummaryRecord {
    pub conversation_id: String,
    pub conversation_type: String,
    pub peer_user_id: String,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub member_count: Option<i64>,
    pub last_read_message_id: Option<i64>,
    pub unread_count_cache: i64,
    pub pinned: bool,
    pub muted: bool,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserWorldMemberRecord {
    pub conversation_id: String,
    pub user_id: String,
    pub peer_user_id: String,
    pub last_read_message_id: Option<i64>,
    pub unread_count_cache: i64,
    pub pinned: bool,
    pub muted: bool,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserWorldGroupRecord {
    pub group_id: String,
    pub conversation_id: String,
    pub group_name: String,
    pub owner_user_id: String,
    pub announcement: Option<String>,
    pub announcement_updated_at: Option<f64>,
    pub member_count: i64,
    pub unread_count_cache: i64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserWorldMessageRecord {
    pub message_id: i64,
    pub conversation_id: String,
    pub sender_user_id: String,
    pub content: String,
    pub content_type: String,
    pub client_msg_id: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserWorldEventRecord {
    pub conversation_id: String,
    pub event_id: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_time: f64,
}

#[derive(Debug, Clone)]
pub struct UserWorldSendMessageResult {
    pub message: UserWorldMessageRecord,
    pub inserted: bool,
    pub event: Option<UserWorldEventRecord>,
}

#[derive(Debug, Clone)]
pub struct UserWorldReadResult {
    pub member: UserWorldMemberRecord,
    pub event: Option<UserWorldEventRecord>,
}

#[derive(Debug, Clone)]
pub struct BeeroomChatMessageRecord {
    pub message_id: i64,
    pub user_id: String,
    pub group_id: String,
    pub sender_kind: String,
    pub sender_name: String,
    pub sender_agent_id: Option<String>,
    pub mention_name: Option<String>,
    pub mention_agent_id: Option<String>,
    pub body: String,
    pub meta: Option<String>,
    pub tone: String,
    pub client_msg_id: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Clone)]
pub struct AgentThreadRecord {
    pub thread_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct AgentTaskRecord {
    pub task_id: String,
    pub thread_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub status: String,
    pub request_payload: Value,
    pub request_id: Option<String>,
    pub retry_count: i64,
    pub retry_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelAccountRecord {
    pub channel: String,
    pub account_id: String,
    pub config: Value,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChannelBindingRecord {
    pub binding_id: String,
    pub channel: String,
    pub account_id: String,
    pub peer_kind: Option<String>,
    pub peer_id: Option<String>,
    pub agent_id: Option<String>,
    pub tool_overrides: Vec<String>,
    pub priority: i64,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChannelUserBindingRecord {
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub user_id: String,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChannelSessionRecord {
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub thread_id: Option<String>,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub user_id: String,
    pub tts_enabled: Option<bool>,
    pub tts_voice: Option<String>,
    pub metadata: Option<Value>,
    pub last_message_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChannelMessageRecord {
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub thread_id: Option<String>,
    pub session_id: String,
    pub message_id: Option<String>,
    pub sender_id: Option<String>,
    pub message_type: String,
    pub payload: Value,
    pub raw_payload: Option<Value>,
    pub created_at: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelMessageStats {
    pub total: i64,
    pub last_message_at: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelOutboxStats {
    pub total: i64,
    pub sent: i64,
    pub retry: i64,
    pub pending: i64,
    pub failed: i64,
    pub retry_attempts: i64,
    pub last_sent_at: Option<f64>,
    pub last_failed_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ChannelOutboxRecord {
    pub outbox_id: String,
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub thread_id: Option<String>,
    pub payload: Value,
    pub status: String,
    pub retry_count: i64,
    pub retry_at: f64,
    pub last_error: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
    pub delivered_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct GatewayClientRecord {
    pub connection_id: String,
    pub role: String,
    pub user_id: Option<String>,
    pub node_id: Option<String>,
    pub scopes: Vec<String>,
    pub caps: Vec<String>,
    pub commands: Vec<String>,
    pub client_info: Option<Value>,
    pub status: String,
    pub connected_at: f64,
    pub last_seen_at: f64,
    pub disconnected_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct GatewayNodeRecord {
    pub node_id: String,
    pub name: Option<String>,
    pub device_fingerprint: Option<String>,
    pub status: String,
    pub caps: Vec<String>,
    pub commands: Vec<String>,
    pub permissions: Option<Value>,
    pub metadata: Option<Value>,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_seen_at: f64,
}

#[derive(Debug, Clone)]
pub struct GatewayNodeTokenRecord {
    pub token: String,
    pub node_id: String,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_used_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MediaAssetRecord {
    pub asset_id: String,
    pub kind: String,
    pub url: String,
    pub mime: Option<String>,
    pub size: Option<i64>,
    pub hash: Option<String>,
    pub source: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Clone)]
pub struct SpeechJobRecord {
    pub job_id: String,
    pub job_type: String,
    pub status: String,
    pub input_text: Option<String>,
    pub input_url: Option<String>,
    pub output_text: Option<String>,
    pub output_url: Option<String>,
    pub model: Option<String>,
    pub error: Option<String>,
    pub retry_count: i64,
    pub next_retry_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct SessionLockRecord {
    pub session_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub updated_time: f64,
    pub expires_at: f64,
}

#[derive(Debug, Clone)]
pub struct SessionRunRecord {
    pub run_id: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub user_id: String,
    pub dispatch_id: Option<String>,
    pub run_kind: Option<String>,
    pub requested_by: Option<String>,
    pub agent_id: Option<String>,
    pub model_name: Option<String>,
    pub status: String,
    pub queued_time: f64,
    pub started_time: f64,
    pub finished_time: f64,
    pub elapsed_s: f64,
    pub result: Option<String>,
    pub error: Option<String>,
    pub updated_time: f64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct CronJobRecord {
    pub job_id: String,
    pub user_id: String,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub session_target: String,
    pub payload: Value,
    pub deliver: Option<Value>,
    pub enabled: bool,
    pub delete_after_run: bool,
    pub schedule_kind: String,
    pub schedule_at: Option<String>,
    pub schedule_every_ms: Option<i64>,
    pub schedule_cron: Option<String>,
    pub schedule_tz: Option<String>,
    pub dedupe_key: Option<String>,
    pub next_run_at: Option<f64>,
    pub running_at: Option<f64>,
    pub runner_id: Option<String>,
    pub run_token: Option<String>,
    pub heartbeat_at: Option<f64>,
    pub lease_expires_at: Option<f64>,
    pub last_run_at: Option<f64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub consecutive_failures: i64,
    pub auto_disabled_reason: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct CronRunRecord {
    pub run_id: String,
    pub job_id: String,
    pub user_id: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub trigger: String,
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub created_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLockStatus {
    Acquired,
    UserBusy,
    SystemBusy,
}

// Parameter objects to keep storage APIs readable and avoid long argument lists.

#[derive(Debug, Clone, Copy)]
pub struct UpdateAgentTaskStatusParams<'a> {
    pub task_id: &'a str,
    pub status: &'a str,
    pub retry_count: i64,
    pub retry_at: f64,
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
    pub last_error: Option<&'a str>,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct UpsertMemoryTaskLogParams<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub task_id: &'a str,
    pub status: &'a str,
    pub queued_time: f64,
    pub started_time: f64,
    pub finished_time: f64,
    pub elapsed_s: f64,
    pub request_payload: Option<&'a Value>,
    pub result: &'a str,
    pub error: &'a str,
    pub updated_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryFragmentRecord {
    pub memory_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub source_session_id: String,
    pub source_round_id: String,
    pub source_type: String,
    pub category: String,
    pub title_l0: String,
    pub summary_l1: String,
    pub content_l2: String,
    pub fact_key: String,
    pub tags: Vec<String>,
    pub entities: Vec<String>,
    pub importance: f64,
    pub confidence: f64,
    pub tier: String,
    pub status: String,
    pub pinned: bool,
    pub confirmed_by_user: bool,
    pub access_count: i64,
    pub hit_count: i64,
    pub last_accessed_at: f64,
    pub valid_from: f64,
    pub invalidated_at: Option<f64>,
    pub supersedes_memory_id: Option<String>,
    pub superseded_by_memory_id: Option<String>,
    pub embedding_model: Option<String>,
    pub vector_ref: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryFragmentEmbeddingRecord {
    pub memory_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub embedding_model: String,
    pub content_hash: String,
    pub vector: Vec<f32>,
    pub dimensions: i64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryHitRecord {
    pub hit_id: String,
    pub memory_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub round_id: String,
    pub query_text: String,
    pub reason_json: Value,
    pub lexical_score: f64,
    pub semantic_score: f64,
    pub freshness_score: f64,
    pub importance_score: f64,
    pub final_score: f64,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryJobRecord {
    pub job_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub job_type: String,
    pub status: String,
    pub request_payload: Value,
    pub result_summary: String,
    pub error_message: String,
    pub queued_at: f64,
    pub started_at: f64,
    pub finished_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListChannelUserBindingsQuery<'a> {
    pub channel: Option<&'a str>,
    pub account_id: Option<&'a str>,
    pub peer_kind: Option<&'a str>,
    pub peer_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateChannelOutboxStatusParams<'a> {
    pub outbox_id: &'a str,
    pub status: &'a str,
    pub retry_count: i64,
    pub retry_at: f64,
    pub last_error: Option<&'a str>,
    pub delivered_at: Option<f64>,
    pub updated_at: f64,
}

/// 存储后端抽象，统一封装历史/监控/记忆的持久化读写。
pub trait StorageBackend: Send + Sync {
    fn ensure_initialized(&self) -> Result<()>;

    fn get_meta(&self, key: &str) -> Result<Option<String>>;
    fn set_meta(&self, key: &str, value: &str) -> Result<()>;
    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize>;

    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()>;

    fn load_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_chat_history_page(
        &self,
        user_id: &str,
        session_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_artifact_logs(&self, user_id: &str, session_id: &str, limit: i64)
        -> Result<Vec<Value>>;
    fn get_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>>;

    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>>;
    fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_log_usage(&self) -> Result<u64>;

    fn delete_chat_history(&self, user_id: &str) -> Result<i64>;
    fn delete_chat_history_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_tool_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;

    fn upsert_monitor_record(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records(&self) -> Result<Vec<Value>>;
    fn load_recent_monitor_records(&self, limit: i64) -> Result<Vec<Value>> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let mut records = self.load_monitor_records()?;
        records.sort_by(|left, right| {
            monitor_record_updated_time(right).total_cmp(&monitor_record_updated_time(left))
        });
        records.truncate(limit as usize);
        Ok(records)
    }
    fn load_monitor_records_by_user(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let status_set = statuses
            .unwrap_or(&[])
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<std::collections::HashSet<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut records = self
            .load_monitor_records()?
            .into_iter()
            .filter(|record| {
                record
                    .get("user_id")
                    .and_then(Value::as_str)
                    .map(|value| value.trim() == cleaned_user)
                    .unwrap_or(false)
            })
            .filter(|record| {
                if status_set.is_empty() {
                    return true;
                }
                record
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| status_set.contains(value.trim()))
                    .unwrap_or(false)
            })
            .filter(|record| {
                let Some(since) = since_time else {
                    return true;
                };
                monitor_record_updated_time(record) >= since
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            monitor_record_updated_time(right).total_cmp(&monitor_record_updated_time(left))
        });
        records.truncate(limit as usize);
        Ok(records)
    }
    fn delete_monitor_record(&self, session_id: &str) -> Result<()>;
    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64>;

    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus>;
    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()>;
    fn release_session_lock(&self, session_id: &str) -> Result<()>;
    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64>;
    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>>;

    fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()>;
    fn get_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<Option<AgentThreadRecord>>;
    fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64>;

    fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()>;
    fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>>;
    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>>;
    fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>>;
    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()>;

    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()>;
    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>>;
    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>>;
    fn delete_vector_document(&self, owner_id: &str, base_name: &str, doc_id: &str)
        -> Result<bool>;
    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64>;

    fn get_max_stream_event_id(&self, session_id: &str) -> Result<i64>;
    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()>;
    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64>;
    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64>;
    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64>;

    fn get_memory_enabled(&self, user_id: &str) -> Result<Option<bool>>;
    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()>;
    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn upsert_memory_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()>;
    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64>;
    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_task_log(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()>;
    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>>;
    fn load_memory_task_log_by_task_id(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>>;
    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_fragment(&self, record: &MemoryFragmentRecord) -> Result<()>;
    fn get_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>>;
    fn list_memory_fragments(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>>;
    fn get_memory_fragment_embedding(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>>;
    fn upsert_memory_fragment_embedding(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()>;
    fn delete_memory_fragment_embeddings(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64>;
    fn delete_memory_fragment(&self, user_id: &str, agent_id: &str, memory_id: &str)
        -> Result<i64>;
    fn insert_memory_hit(&self, record: &MemoryHitRecord) -> Result<()>;
    fn list_memory_hits(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>>;
    fn list_memory_hit_counts(&self, user_id: &str, agent_id: &str)
        -> Result<HashMap<String, i64>>;
    fn has_memory_hit_event(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool>;
    fn upsert_memory_job(&self, record: &MemoryJobRecord) -> Result<()>;
    fn list_memory_jobs(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>>;

    fn create_benchmark_run(&self, payload: &Value) -> Result<()>;
    fn update_benchmark_run(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_attempt(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_task_aggregate(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn load_benchmark_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_benchmark_run(&self, run_id: &str) -> Result<Option<Value>>;
    fn load_benchmark_attempts(&self, run_id: &str) -> Result<Vec<Value>>;
    fn load_benchmark_task_aggregates(&self, run_id: &str) -> Result<Vec<Value>>;
    fn delete_benchmark_run(&self, run_id: &str) -> Result<i64>;

    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>>;

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()>;
    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()>;
    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>>;
    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)>;
    fn add_user_experience(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult>;
    fn delete_user_account(&self, user_id: &str) -> Result<i64>;

    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>>;
    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>>;
    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()>;
    fn delete_org_unit(&self, unit_id: &str) -> Result<i64>;

    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()>;
    fn get_external_link(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>>;
    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>>;
    fn delete_external_link(&self, link_id: &str) -> Result<i64>;

    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()>;
    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>>;
    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()>;
    fn delete_user_token(&self, token: &str) -> Result<i64>;
    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()>;
    fn get_user_session_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>>;

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()>;
    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>>;
    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_sessions_by_status(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>>;
    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()>;
    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()>;
    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64>;

    fn resolve_or_create_user_world_direct_conversation(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn create_user_world_group(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn get_user_world_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>>;
    fn get_user_world_member(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>>;
    fn list_user_world_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)>;
    fn list_user_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>>;
    fn send_user_world_message(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult>;
    fn mark_user_world_read(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>>;
    fn list_user_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>>;
    fn list_user_world_groups(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)>;
    fn get_user_world_group_by_id(&self, group_id: &str) -> Result<Option<UserWorldGroupRecord>>;
    fn update_user_world_group_announcement(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>>;
    fn list_user_world_member_user_ids(&self, conversation_id: &str) -> Result<Vec<String>>;

    fn list_beeroom_chat_messages(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>>;
    #[allow(clippy::too_many_arguments)]
    fn append_beeroom_chat_message(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord>;
    fn delete_beeroom_chat_messages(&self, user_id: &str, group_id: &str) -> Result<i64>;

    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()>;
    fn get_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>>;
    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>>;
    fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<i64>;

    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()>;
    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>>;
    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64>;

    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()>;
    fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>>;
    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)>;
    fn delete_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64>;

    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()>;
    fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>>;
    fn list_channel_sessions(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)>;

    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()>;
    fn list_channel_messages(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>>;
    fn get_channel_message_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats>;
    fn get_channel_outbox_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats>;
    fn delete_channel_sessions(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_messages(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_outbox(&self, channel: &str, account_id: &str) -> Result<i64>;

    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()>;
    fn get_channel_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>>;
    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>>;
    fn update_channel_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()>;

    fn upsert_bridge_center(&self, record: &BridgeCenterRecord) -> Result<()>;
    fn get_bridge_center(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>>;
    fn get_bridge_center_by_code(&self, code: &str) -> Result<Option<BridgeCenterRecord>>;
    fn list_bridge_centers(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)>;
    fn delete_bridge_center(&self, center_id: &str) -> Result<i64>;

    fn upsert_bridge_center_account(&self, record: &BridgeCenterAccountRecord) -> Result<()>;
    fn get_bridge_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn get_bridge_center_account_by_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn list_bridge_center_accounts(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)>;
    fn delete_bridge_center_account(&self, center_account_id: &str) -> Result<i64>;
    fn delete_bridge_center_accounts_by_center(&self, center_id: &str) -> Result<i64>;

    fn upsert_bridge_user_route(&self, record: &BridgeUserRouteRecord) -> Result<()>;
    fn get_bridge_user_route(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>>;
    fn get_bridge_user_route_by_identity(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>>;
    fn list_bridge_user_routes(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)>;
    fn delete_bridge_user_route(&self, route_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center_account(&self, center_account_id: &str) -> Result<i64>;

    fn insert_bridge_delivery_log(&self, record: &BridgeDeliveryLogRecord) -> Result<()>;
    fn list_bridge_delivery_logs(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>>;
    fn delete_bridge_delivery_logs_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_delivery_logs_by_center_account(&self, center_account_id: &str)
        -> Result<i64>;

    fn insert_bridge_route_audit_log(&self, record: &BridgeRouteAuditLogRecord) -> Result<()>;
    fn list_bridge_route_audit_logs(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>>;
    fn delete_bridge_route_audit_logs_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_route_audit_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64>;

    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()>;
    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>>;

    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()>;
    fn get_gateway_node(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>>;
    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>>;

    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()>;
    fn get_gateway_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>>;
    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>>;
    fn delete_gateway_node_token(&self, token: &str) -> Result<i64>;

    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()>;
    fn get_media_asset(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>>;
    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>>;

    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()>;
    fn list_pending_speech_jobs(&self, job_type: &str, limit: i64) -> Result<Vec<SpeechJobRecord>>;

    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()>;
    fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>>;
    fn list_session_runs_by_session(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_parent(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_dispatch(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;

    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()>;
    fn get_cron_job(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>>;
    fn get_cron_job_by_dedupe_key(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>>;
    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>>;
    fn delete_cron_job(&self, user_id: &str, job_id: &str) -> Result<i64>;
    fn delete_cron_jobs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn reset_cron_jobs_running(&self) -> Result<()>;
    fn count_running_cron_jobs(&self, now: f64) -> Result<i64>;
    fn claim_due_cron_jobs(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>>;
    fn renew_cron_job_lease(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool>;
    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()>;
    fn list_cron_runs(&self, user_id: &str, job_id: &str, limit: i64)
        -> Result<Vec<CronRunRecord>>;
    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>>;

    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>>;
    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()>;
    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>>;
    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()>;
    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()>;
    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn list_user_agents_by_hive(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>>;
    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64>;

    fn upsert_hive(&self, record: &HiveRecord) -> Result<()>;
    fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>>;
    fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>>;
    fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn move_agents_to_hive(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64>;

    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()>;
    fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>>;
    fn list_team_runs(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)>;
    fn list_team_runs_by_status(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>>;
    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()>;
    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>>;
    fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>>;

    fn prepare_user_token_balance(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn consume_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn grant_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
}

// Helper for sorting monitor session records by updated_time.
fn monitor_record_updated_time(record: &Value) -> f64 {
    record
        .get("updated_time")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// Build storage backend from config, selecting SQLite/Postgres.
pub fn build_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    let backend = config.backend.trim().to_lowercase();
    let backend = if backend.is_empty() {
        "sqlite".to_string()
    } else {
        backend
    };
    match backend.as_str() {
        "sqlite" | "default" => Ok(Arc::new(SqliteStorage::new(
            config.db_path.trim().to_string(),
        ))),
        "postgres" | "postgresql" | "pg" | "auto" => Ok(Arc::new(PostgresStorage::new(
            config.postgres.dsn.clone(),
            config.postgres.connect_timeout_s,
            config.postgres.pool_size,
        )?)),
        other => Err(anyhow!("未知存储后端: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_hive_id, normalize_sandbox_container_id, normalize_workspace_container_id,
        DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID,
        MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
    };

    #[test]
    fn normalize_sandbox_container_id_clamps_to_range() {
        assert_eq!(
            normalize_sandbox_container_id(MIN_SANDBOX_CONTAINER_ID - 1),
            MIN_SANDBOX_CONTAINER_ID
        );
        assert_eq!(
            normalize_sandbox_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_sandbox_container_id_keeps_default_in_range() {
        assert_eq!(
            normalize_sandbox_container_id(DEFAULT_SANDBOX_CONTAINER_ID),
            DEFAULT_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_workspace_container_id_allows_user_private_container() {
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID - 1),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_hive_id_falls_back_to_default_when_empty_or_invalid() {
        assert_eq!(normalize_hive_id(""), DEFAULT_HIVE_ID);
        assert_eq!(normalize_hive_id("   "), DEFAULT_HIVE_ID);
        assert_eq!(normalize_hive_id("@@@"), DEFAULT_HIVE_ID);
    }

    #[test]
    fn normalize_hive_id_keeps_safe_characters_and_lowercases() {
        assert_eq!(normalize_hive_id("Hive_A-01"), "hive_a-01");
        assert_eq!(normalize_hive_id(" hive-Main "), "hive-main");
    }
}
