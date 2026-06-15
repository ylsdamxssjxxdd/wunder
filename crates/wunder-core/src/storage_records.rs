use crate::schemas::AbilityDescriptor;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub preview_skill: bool,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub ability_items: Vec<AbilityDescriptor>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    #[serde(default)]
    pub visible_unit_ids: Vec<String>,
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
    pub preview_skill: bool,
    pub model_name: Option<String>,
    pub ability_items: Vec<AbilityDescriptor>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub visible_unit_ids: Vec<String>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct SessionGoalRecord {
    pub goal_id: String,
    pub session_id: String,
    pub user_id: String,
    pub objective: String,
    pub status: String,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at: f64,
    pub updated_at: f64,
    pub completed_at: Option<f64>,
    pub last_continued_at: Option<f64>,
    pub source: String,
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
