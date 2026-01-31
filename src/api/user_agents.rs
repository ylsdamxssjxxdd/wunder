// 用户智能体 API：创建/管理用户自定义智能体。
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::state::AppState;
use crate::user_access::{
    build_user_tool_context, compute_allowed_tool_names, filter_user_agents_by_access,
    is_agent_allowed,
};
use anyhow::Result;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/agents", get(list_agents).post(create_agent))
        .route("/wunder/agents/shared", get(list_shared_agents))
        .route("/wunder/agents/running", get(list_running_agents))
        .route(
            "/wunder/agents/{agent_id}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
}

async fn list_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    ensure_preset_agents(&state, &resolved.user).await?;
    let agents = state
        .user_store
        .list_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let items = filtered
        .iter()
        .map(|record| agent_payload(record))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_shared_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let agents = state
        .user_store
        .list_shared_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let items = filtered
        .iter()
        .map(|record| agent_payload(record))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_running_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let locks = state
        .user_store
        .list_session_locks_by_user(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::new();
    let mut seen_sessions = HashSet::new();
    for lock in locks {
        seen_sessions.insert(lock.session_id.clone());
        let agent_id = lock.agent_id.trim();
        let is_default = agent_id.is_empty();
        let agent_id = if is_default { "" } else { agent_id };
        items.push(json!({
            "agent_id": agent_id,
            "session_id": lock.session_id,
            "updated_at": format_ts(lock.updated_time),
            "expires_at": format_ts(lock.expires_at),
            "state": "running",
            "is_default": is_default,
        }));
    }
    let active_sessions = state.monitor.list_sessions(true);
    for session in active_sessions {
        let status = session.get("status").and_then(Value::as_str).unwrap_or("");
        if status != MonitorState::STATUS_WAITING
            && status != MonitorState::STATUS_RUNNING
            && status != MonitorState::STATUS_CANCELLING
        {
            continue;
        }
        let session_user_id = session.get("user_id").and_then(Value::as_str).unwrap_or("");
        if session_user_id != user_id {
            continue;
        }
        let session_id = session
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            continue;
        }
        if seen_sessions.contains(&session_id) {
            continue;
        }
        let record = state
            .user_store
            .get_chat_session(&user_id, &session_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let Some(record) = record else {
            continue;
        };
        let agent_id = record.agent_id.unwrap_or_default();
        let is_default = agent_id.trim().is_empty();
        let agent_id = if is_default { "".to_string() } else { agent_id };
        let updated_at = session
            .get("updated_time")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let is_waiting = status == MonitorState::STATUS_WAITING;
        items.push(json!({
            "agent_id": agent_id,
            "session_id": session_id,
            "updated_at": updated_at,
            "expires_at": "",
            "state": if is_waiting { "waiting" } else { "running" },
            "pending_question": is_waiting,
            "is_default": is_default,
        }));
    }
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn get_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_agent_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.agent_not_found"),
        ));
    }
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn create_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AgentCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut tool_names = normalize_tool_list(payload.tool_names);
    if !tool_names.is_empty() {
        let context = build_user_tool_context(&state, &user_id).await;
        let allowed = compute_allowed_tool_names(&resolved.user, &context);
        tool_names = filter_allowed_tools(&tool_names, &allowed);
    }
    let access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
    let status = normalize_agent_status(payload.status.as_deref());
    let is_shared = payload.is_shared.unwrap_or(false);
    let now = now_ts();
    let record = crate::storage::UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user_id.clone(),
        name,
        description: payload.description.unwrap_or_default(),
        system_prompt: payload.system_prompt.unwrap_or_default(),
        tool_names,
        access_level,
        is_shared,
        status,
        icon: payload.icon,
        created_at: now,
        updated_at: now,
    };
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn update_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Json(payload): Json<AgentUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_user_agent(&user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    if let Some(name) = payload.name {
        let cleaned = name.trim();
        if !cleaned.is_empty() {
            record.name = cleaned.to_string();
        }
    }
    if let Some(description) = payload.description {
        record.description = description;
    }
    if let Some(system_prompt) = payload.system_prompt {
        record.system_prompt = system_prompt;
    }
    if let Some(is_shared) = payload.is_shared {
        record.is_shared = is_shared;
    }
    if let Some(tool_names) = payload.tool_names {
        let mut normalized = normalize_tool_list(tool_names);
        if !normalized.is_empty() {
            let context = build_user_tool_context(&state, &user_id).await;
            let allowed = compute_allowed_tool_names(&resolved.user, &context);
            normalized = filter_allowed_tools(&normalized, &allowed);
        }
        record.tool_names = normalized;
    }
    if let Some(status) = payload.status {
        record.status = normalize_agent_status(Some(&status));
    }
    if payload.icon.is_some() {
        record.icon = payload.icon;
    }
    record.updated_at = now_ts();
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn delete_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    state
        .user_store
        .delete_user_agent(&resolved.user.user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut workspace_ids = state
        .workspace
        .scoped_user_id_variants(&resolved.user.user_id, Some(cleaned));
    workspace_ids.sort();
    workspace_ids.dedup();
    for workspace_id in workspace_ids {
        let _ = state.workspace.purge_user_data(&workspace_id);
    }
    Ok(Json(json!({ "data": { "id": cleaned } })))
}

fn agent_payload(record: &crate::storage::UserAgentRecord) -> Value {
    json!({
        "id": record.agent_id,
        "name": record.name,
        "description": record.description,
        "system_prompt": record.system_prompt,
        "tool_names": record.tool_names,
        "access_level": record.access_level,
        "is_shared": record.is_shared,
        "status": record.status,
        "icon": record.icon,
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
    })
}

fn normalize_tool_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    output
}

fn filter_allowed_tools(values: &[String], allowed: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect()
}

fn normalize_agent_status(raw: Option<&str>) -> String {
    let status = raw.unwrap_or("active").trim();
    if status.is_empty() {
        "active".to_string()
    } else {
        status.to_string()
    }
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

const PRESET_META_PREFIX: &str = "user_agent_presets_v1:";
const LEGACY_EMAIL_PRESET_NAME: &str = "邮件写作";
const PRESET_OFFICIAL_NAME: &str = "公文写作";
const PRESET_OFFICIAL_DESCRIPTION: &str = "公文起草、格式规范与措辞润色";
const PRESET_OFFICIAL_PROMPT: &str = "你是公文写作助手。根据需求输出规范公文：1) 明确文种与适用场景；2) 按标准结构组织（标题、主送、正文、落款/日期、附件/抄送）；3) 用词准确、语气正式，避免口语化；4) 如信息不完整，先给出待补充清单。";
const PRESET_OFFICIAL_ICON: &str = "shield";
const PRESET_OFFICIAL_COLOR: &str = "#94a3b8";
const LEGACY_MEETING_NAME: &str = "会议纪要";
const LEGACY_PLAN_NAME: &str = "方案策划";
const PRESET_SCI_DRAW_NAME: &str = "科学绘图";
const PRESET_SCI_DRAW_DESCRIPTION: &str = "生成科学图表方案与绘图建议";
const PRESET_SCI_DRAW_PROMPT: &str = "你是科学绘图助手。根据研究问题与数据，给出推荐图表类型、数据整理方式、坐标轴/标注/配色方案，并提供绘图步骤（可用 Python/Matplotlib 或 R/ggplot2 模板）。如信息不足，先提出澄清问题。";
const PRESET_SCI_DRAW_ICON: &str = "chart";
const PRESET_SCI_DRAW_COLOR: &str = "#22d3ee";
const PRESET_POLICY_ANALYSIS_NAME: &str = "政策分析";
const PRESET_POLICY_ANALYSIS_DESCRIPTION: &str = "解读政策要点并评估影响";
const PRESET_POLICY_ANALYSIS_PROMPT: &str = "你是政策分析助手。请输出政策摘要、适用对象与范围、执行路径、潜在影响（经济/社会/行业）、风险与对策以及可执行建议。必要时先提问补充信息。";
const PRESET_POLICY_ANALYSIS_ICON: &str = "briefcase";
const PRESET_POLICY_ANALYSIS_COLOR: &str = "#f97316";

struct PresetAgent {
    name: &'static str,
    description: &'static str,
    system_prompt: &'static str,
    icon_name: &'static str,
    icon_color: &'static str,
}

async fn ensure_preset_agents(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> Result<(), Response> {
    let meta_key = format!("{PRESET_META_PREFIX}{}", user.user_id);
    let existing = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut existing_names: HashSet<String> = existing
        .iter()
        .map(|record| record.name.trim().to_string())
        .collect();
    let now = now_ts();
    for record in existing {
        let trimmed_name = record.name.trim();
        if trimmed_name == LEGACY_EMAIL_PRESET_NAME {
            let mut updated = record.clone();
            updated.name = PRESET_OFFICIAL_NAME.to_string();
            updated.description = PRESET_OFFICIAL_DESCRIPTION.to_string();
            updated.system_prompt = PRESET_OFFICIAL_PROMPT.to_string();
            updated.icon = Some(build_icon_payload(
                PRESET_OFFICIAL_ICON,
                PRESET_OFFICIAL_COLOR,
            ));
            updated.updated_at = now;
            let _ = state.user_store.upsert_user_agent(&updated);
            existing_names.remove(LEGACY_EMAIL_PRESET_NAME);
            existing_names.insert(PRESET_OFFICIAL_NAME.to_string());
            continue;
        }
        if trimmed_name == LEGACY_MEETING_NAME {
            let mut updated = record.clone();
            updated.name = PRESET_SCI_DRAW_NAME.to_string();
            updated.description = PRESET_SCI_DRAW_DESCRIPTION.to_string();
            updated.system_prompt = PRESET_SCI_DRAW_PROMPT.to_string();
            updated.icon = Some(build_icon_payload(
                PRESET_SCI_DRAW_ICON,
                PRESET_SCI_DRAW_COLOR,
            ));
            updated.updated_at = now;
            let _ = state.user_store.upsert_user_agent(&updated);
            existing_names.remove(LEGACY_MEETING_NAME);
            existing_names.insert(PRESET_SCI_DRAW_NAME.to_string());
            continue;
        }
        if trimmed_name == LEGACY_PLAN_NAME {
            let mut updated = record.clone();
            updated.name = PRESET_POLICY_ANALYSIS_NAME.to_string();
            updated.description = PRESET_POLICY_ANALYSIS_DESCRIPTION.to_string();
            updated.system_prompt = PRESET_POLICY_ANALYSIS_PROMPT.to_string();
            updated.icon = Some(build_icon_payload(
                PRESET_POLICY_ANALYSIS_ICON,
                PRESET_POLICY_ANALYSIS_COLOR,
            ));
            updated.updated_at = now;
            let _ = state.user_store.upsert_user_agent(&updated);
            existing_names.remove(LEGACY_PLAN_NAME);
            existing_names.insert(PRESET_POLICY_ANALYSIS_NAME.to_string());
        }
    }
    let seeded = state
        .user_store
        .get_meta(&meta_key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if seeded.is_some() {
        return Ok(());
    }
    let context = build_user_tool_context(state, &user.user_id).await;
    let mut tool_names = compute_allowed_tool_names(user, &context)
        .into_iter()
        .collect::<Vec<_>>();
    tool_names.sort();
    let access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
    for preset in preset_agents() {
        if existing_names.contains(preset.name) {
            continue;
        }
        let record = crate::storage::UserAgentRecord {
            agent_id: format!("agent_{}", Uuid::new_v4().simple()),
            user_id: user.user_id.clone(),
            name: preset.name.to_string(),
            description: preset.description.to_string(),
            system_prompt: preset.system_prompt.to_string(),
            tool_names: tool_names.clone(),
            access_level: access_level.clone(),
            is_shared: false,
            status: "active".to_string(),
            icon: Some(build_icon_payload(preset.icon_name, preset.icon_color)),
            created_at: now,
            updated_at: now,
        };
        let _ = state.user_store.upsert_user_agent(&record);
    }
    let _ = state.user_store.set_meta(&meta_key, "1");
    Ok(())
}

fn preset_agents() -> Vec<PresetAgent> {
    vec![
        PresetAgent {
            name: "文稿校对",
            description: "语病检查、错别字修正与语气优化",
            system_prompt: "你是专业的中文文稿校对助手。收到文本后：1) 保持原意，给出校对后的完整版本；2) 列出关键修改点（错别字、语病、标点、格式）；3) 如需调整语气，给出替换建议。不要新增未提供的事实。",
            icon_name: "spark",
            icon_color: "#fbbf24",
        },
        PresetAgent {
            name: "数据分析",
            description: "结构化分析数据并输出结论与建议",
            system_prompt: "你是数据分析助手。先确认分析目标、字段含义与数据范围，必要时提出澄清问题；分析时给出步骤、关键指标和可视化建议；结论用要点输出，并提供可执行的改进建议。",
            icon_name: "chart",
            icon_color: "#60a5fa",
        },
        PresetAgent {
            name: PRESET_SCI_DRAW_NAME,
            description: PRESET_SCI_DRAW_DESCRIPTION,
            system_prompt: PRESET_SCI_DRAW_PROMPT,
            icon_name: PRESET_SCI_DRAW_ICON,
            icon_color: PRESET_SCI_DRAW_COLOR,
        },
        PresetAgent {
            name: PRESET_POLICY_ANALYSIS_NAME,
            description: PRESET_POLICY_ANALYSIS_DESCRIPTION,
            system_prompt: PRESET_POLICY_ANALYSIS_PROMPT,
            icon_name: PRESET_POLICY_ANALYSIS_ICON,
            icon_color: PRESET_POLICY_ANALYSIS_COLOR,
        },
        PresetAgent {
            name: PRESET_OFFICIAL_NAME,
            description: PRESET_OFFICIAL_DESCRIPTION,
            system_prompt: PRESET_OFFICIAL_PROMPT,
            icon_name: PRESET_OFFICIAL_ICON,
            icon_color: PRESET_OFFICIAL_COLOR,
        },
    ]
}

fn build_icon_payload(name: &str, color: &str) -> String {
    serde_json::json!({ "name": name, "color": color }).to_string()
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}

#[derive(Debug, Deserialize)]
struct AgentCreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tool_names: Option<Vec<String>>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}
