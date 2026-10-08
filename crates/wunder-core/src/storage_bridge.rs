use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeCenterRecord {
    pub center_id: String,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub owner_user_id: String,
    pub status: String,
    pub default_preset_agent_name: String,
    pub target_unit_id: Option<String>,
    pub default_identity_strategy: String,
    pub username_policy: String,
    pub password_policy: String,
    pub settings: Value,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeCenterAccountRecord {
    pub center_account_id: String,
    pub center_id: String,
    pub channel: String,
    pub account_id: String,
    pub enabled: bool,
    pub default_preset_agent_name_override: Option<String>,
    pub identity_strategy: Option<String>,
    pub thread_strategy: Option<String>,
    pub reply_strategy: Option<String>,
    pub fallback_policy: String,
    pub provider_caps: Option<Value>,
    pub status_reason: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeUserRouteRecord {
    pub route_id: String,
    pub center_id: String,
    pub center_account_id: String,
    pub channel: String,
    pub account_id: String,
    pub external_identity_key: String,
    pub external_user_key: Option<String>,
    pub external_display_name: Option<String>,
    pub external_peer_id: Option<String>,
    pub external_sender_id: Option<String>,
    pub external_thread_id: Option<String>,
    pub external_profile: Option<Value>,
    pub wunder_user_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub user_created: bool,
    pub agent_created: bool,
    pub status: String,
    pub last_session_id: Option<String>,
    pub last_error: Option<String>,
    pub first_seen_at: f64,
    pub last_seen_at: f64,
    pub last_inbound_at: Option<f64>,
    pub last_outbound_at: Option<f64>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeDeliveryLogRecord {
    pub delivery_id: String,
    pub center_id: String,
    pub center_account_id: String,
    pub route_id: Option<String>,
    pub direction: String,
    pub stage: String,
    pub provider_message_id: Option<String>,
    pub session_id: Option<String>,
    pub status: String,
    pub summary: String,
    pub payload: Option<Value>,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRouteAuditLogRecord {
    pub audit_id: String,
    pub center_id: String,
    pub route_id: Option<String>,
    pub actor_type: String,
    pub actor_id: String,
    pub action: String,
    pub detail: Option<Value>,
    pub created_at: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListBridgeCentersQuery<'a> {
    pub status: Option<&'a str>,
    pub keyword: Option<&'a str>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListBridgeCenterAccountsQuery<'a> {
    pub center_id: Option<&'a str>,
    pub channel: Option<&'a str>,
    pub account_id: Option<&'a str>,
    pub enabled: Option<bool>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListBridgeUserRoutesQuery<'a> {
    pub center_id: Option<&'a str>,
    pub center_account_id: Option<&'a str>,
    pub channel: Option<&'a str>,
    pub account_id: Option<&'a str>,
    pub status: Option<&'a str>,
    pub keyword: Option<&'a str>,
    pub wunder_user_id: Option<&'a str>,
    pub agent_id: Option<&'a str>,
    pub external_identity_key: Option<&'a str>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListBridgeDeliveryLogsQuery<'a> {
    pub center_id: Option<&'a str>,
    pub center_account_id: Option<&'a str>,
    pub route_id: Option<&'a str>,
    pub direction: Option<&'a str>,
    pub status: Option<&'a str>,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ListBridgeRouteAuditLogsQuery<'a> {
    pub center_id: Option<&'a str>,
    pub route_id: Option<&'a str>,
    pub limit: i64,
}
