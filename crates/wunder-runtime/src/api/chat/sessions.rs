use super::{
    error_response, fetch_agent_record, format_ts, now_ts, reject_locked_orchestration_session,
    resolve_chat_model_name, CHAT_SESSION_STATUS_ACTIVE, CHAT_SESSION_STATUS_ARCHIVED,
    DEFAULT_SESSION_TITLE,
};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::services::chat_transcript::build_chat_transcript;
use crate::services::llm::is_llm_model;
use crate::services::orchestration_context::{
    active_orchestration_for_agent, build_locked_thread_message, load_round_state,
    load_session_context, session_orchestration_lock_info, ORCHESTRATION_THREAD_LOCKED_CODE,
};
use crate::state::AppState;
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_MESSAGE_LIMIT: i64 = 500;
const SESSION_DETAIL_MAX_LIMIT: i64 = 500;
const MAX_VISIBLE_HISTORY_PAGE_FETCHES: usize = 6;

struct RawHistoryPage {
    history: Vec<Value>,
    has_more: bool,
    before_id: Option<i64>,
}

struct VisibleTranscriptPage {
    transcript: Vec<Value>,
    history_has_more: bool,
    history_before_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct RunningTurnHint {
    user_round: Option<i64>,
    model_round: Option<i64>,
}

#[derive(Debug, Clone)]
struct SessionModelRuntime {
    config_key: Option<String>,
    display_name: Option<String>,
    max_context: Option<u32>,
}

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions",
            post(create_session).get(list_sessions),
        )
        .route(
            "/wunder/chat/sessions/{session_id}",
            get(get_session).delete(delete_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/archive",
            post(archive_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/restore",
            post(restore_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/title",
            post(update_session_title),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/history",
            get(get_session_history),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/messages/{history_id}/feedback",
            post(submit_message_feedback),
        )
}

#[derive(Debug, Deserialize)]
struct SessionListQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(
        default,
        alias = "parent_id",
        alias = "parentId",
        alias = "parentSessionId"
    )]
    parent_session_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionDetailQuery {
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct HistoryPageQuery {
    #[serde(default)]
    before_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionTitleUpdateRequest {
    title: String,
}

#[derive(Debug, Deserialize)]
struct MessageFeedbackRequest {
    vote: String,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let now = now_ts();
    let session_id = format!("sess_{}", Uuid::new_v4().simple());
    let title = payload
        .title
        .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string())
        .trim()
        .to_string();
    let agent_id = payload
        .agent_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(agent_id) = agent_id.as_deref() {
        if let Some((lock_state, lock_binding)) =
            active_orchestration_for_agent(state.storage.as_ref(), &resolved.user.user_id, agent_id)
        {
            return Err(crate::api::errors::error_response_with_detail(
                StatusCode::CONFLICT,
                Some(ORCHESTRATION_THREAD_LOCKED_CODE),
                build_locked_thread_message(&lock_state, &lock_binding),
                Some("Use the orchestration page to continue this orchestration thread."),
                Some(json!({
                    "group_id": lock_state.group_id,
                    "orchestration_id": lock_state.orchestration_id,
                    "run_id": lock_state.run_id,
                    "session_id": lock_binding.session_id,
                    "agent_id": lock_binding.agent_id,
                    "role": lock_binding.role,
                })),
            ));
        }
    }
    let agent_record =
        fetch_agent_record(&state, &resolved.user, agent_id.as_deref(), false).await?;
    let record = crate::storage::ChatSessionRecord {
        session_id: session_id.clone(),
        user_id: resolved.user.user_id.clone(),
        title: if title.is_empty() {
            DEFAULT_SESSION_TITLE.to_string()
        } else {
            title
        },
        status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id,
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    };
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let is_main = state
        .kernel
        .thread_runtime
        .set_main_session(
            &resolved.user.user_id,
            record.agent_id.as_deref().unwrap_or(""),
            &session_id,
            "create",
        )
        .await
        .is_ok();
    let config = state.config_store.get().await;
    let runtime = resolve_session_model_runtime(&config, agent_record.as_ref());
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let mut payload = session_payload_with_main(&record, is_main);
    insert_session_orchestration_lock_fields(
        &mut payload,
        &state,
        &resolved.user.user_id,
        &session_id,
    );
    insert_session_goal_payload(&mut payload, goal.as_ref());
    insert_session_runtime_fields(
        &mut payload,
        runtime.as_ref(),
        state
            .workspace
            .load_session_context_tokens(&resolved.user.user_id, &session_id),
    );
    Ok(Json(json!({ "data": payload })))
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = resolve_pagination(&query);
    let agent_id = query.agent_id.as_deref().map(str::trim);
    let parent_session_id = query.parent_session_id.as_deref().map(str::trim);
    let status_filter = match query.status.as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED) => {
            Some(CHAT_SESSION_STATUS_ARCHIVED)
        }
        Some(value) if value.eq_ignore_ascii_case("all") => None,
        _ => Some(CHAT_SESSION_STATUS_ACTIVE),
    };
    let (sessions, total) = state
        .user_store
        .list_chat_sessions_by_status(
            &resolved.user.user_id,
            agent_id,
            parent_session_id,
            status_filter,
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut main_map: HashMap<String, Option<String>> = HashMap::new();
    for record in &sessions {
        let agent_key = record.agent_id.clone().unwrap_or_default();
        if main_map.contains_key(&agent_key) {
            continue;
        }
        let main = state
            .user_store
            .get_agent_thread(&resolved.user.user_id, &agent_key)
            .ok()
            .flatten()
            .map(|item| item.session_id);
        main_map.insert(agent_key, main);
    }
    let config = state.config_store.get().await;
    let mut agent_record_map: HashMap<String, Option<crate::storage::UserAgentRecord>> =
        HashMap::new();
    let session_ids = sessions
        .iter()
        .map(|record| record.session_id.clone())
        .collect::<Vec<_>>();
    let goal_map = crate::services::goal::list_goals(
        state.storage.clone(),
        &resolved.user.user_id,
        &session_ids,
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .into_iter()
    .map(|record| (record.session_id.clone(), record))
    .collect::<HashMap<_, _>>();
    let mut items = Vec::with_capacity(sessions.len());
    for record in &sessions {
        let agent_key = record.agent_id.clone().unwrap_or_default();
        let is_main = main_map
            .get(&agent_key)
            .and_then(|value| value.as_ref())
            .map(|session_id| session_id == &record.session_id)
            .unwrap_or(false);
        let mut payload = session_payload_with_main(record, is_main);
        insert_session_orchestration_lock_fields(
            &mut payload,
            &state,
            &resolved.user.user_id,
            &record.session_id,
        );
        insert_session_goal_payload(&mut payload, goal_map.get(&record.session_id));
        let runtime = resolve_cached_session_model_runtime(
            &config,
            &mut agent_record_map,
            &state,
            &resolved.user,
            record.agent_id.as_deref(),
        )
        .await?;
        insert_session_runtime_fields(
            &mut payload,
            runtime.as_ref(),
            state
                .workspace
                .load_session_context_tokens(&resolved.user.user_id, &record.session_id),
        );
        items.push(payload);
    }
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<SessionDetailQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let is_admin = UserStore::is_admin(&resolved.user);
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, record.agent_id.as_deref(), true).await?;
    let agent_name = agent_record
        .as_ref()
        .map(|item| item.name.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let monitor_record = state.monitor.get_record(&session_id);
    let session_status = monitor_record.as_ref().and_then(|record| {
        record
            .get("status")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    });
    let message_feedback = extract_monitor_message_feedback_map(monitor_record.as_ref());
    let limit = normalize_session_detail_limit(
        query.limit,
        if is_admin { 0 } else { DEFAULT_MESSAGE_LIMIT },
    );
    let mut history_incomplete = false;
    let mut transcript_page = match load_visible_transcript_page(
        state.as_ref(),
        &resolved.user.user_id,
        &session_id,
        None,
        limit,
        &message_feedback,
    ) {
        Ok(page) => page,
        Err(err) => {
            warn!(
                "load history failed: user_id={}, session_id={}, error={err}",
                resolved.user.user_id, session_id
            );
            history_incomplete = true;
            VisibleTranscriptPage {
                transcript: Vec::new(),
                history_has_more: false,
                history_before_id: None,
            }
        }
    };
    let monitor_active = session_status
        .as_deref()
        .map(is_session_stream_active)
        .unwrap_or(false);
    let active_queue_tasks = list_active_queue_tasks(&state.user_store, &session_id);
    let pure_queue_phase = !monitor_active && !active_queue_tasks.is_empty();
    let session_running = monitor_active || !active_queue_tasks.is_empty();
    let mut transcript = std::mem::take(&mut transcript_page.transcript);
    project_queued_session_messages(&mut transcript, &active_queue_tasks, pure_queue_phase);
    apply_session_running_state(
        &mut transcript,
        session_running,
        running_turn_hint(monitor_record.as_ref()),
    );
    let transcript = transcript
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            if let Value::Object(mut map) = item {
                map.insert("turn_index".to_string(), json!((index + 1) as i64));
                return Value::Object(map);
            }
            item
        })
        .collect::<Vec<_>>();

    let config = state.config_store.get().await;
    let runtime = resolve_session_model_runtime(&config, agent_record.as_ref());
    let context_tokens = state
        .workspace
        .load_session_context_tokens(&resolved.user.user_id, &session_id)
        .max(0);
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    Ok(Json(json!({
        "data": {
            "id": record.session_id,
            "title": record.title,
            "created_at": format_ts(record.created_at),
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "agent_id": record.agent_id,
            "agent_name": agent_name,
            "tool_overrides": record.tool_overrides,
            "history_incomplete": history_incomplete,
            "transcript": transcript,
            "model_name": runtime.as_ref().and_then(|runtime| runtime.display_name.clone()),
            "model_key": runtime.as_ref().and_then(|runtime| runtime.config_key.clone()),
            "context_max_tokens": runtime.as_ref().and_then(|runtime| runtime.max_context),
            "context_total_tokens": runtime.as_ref().and_then(|runtime| runtime.max_context),
            "goal": goal.as_ref().map(crate::services::goal::goal_payload),
            "context_tokens": context_tokens,
            "context_occupancy_tokens": context_tokens,
            "history_has_more": transcript_page.history_has_more,
            "history_before_id": transcript_page.history_before_id
        }
    })))
}

fn normalize_session_detail_limit(raw: Option<i64>, fallback: i64) -> i64 {
    let value = raw.unwrap_or(fallback);
    if value <= 0 {
        0
    } else {
        value.min(SESSION_DETAIL_MAX_LIMIT)
    }
}

fn oldest_history_id_from_transcript(transcript: &[Value]) -> Option<i64> {
    transcript
        .iter()
        .find_map(|item| item.get("history_id").and_then(Value::as_i64))
}

fn merge_visible_transcript_page(
    transcript: &mut Vec<Value>,
    page_transcript: Vec<Value>,
    limit: i64,
) -> bool {
    if page_transcript.is_empty() {
        return false;
    }
    let mut merged = page_transcript;
    merged.extend(std::mem::take(transcript));
    let trimmed = limit > 0 && merged.len() > limit as usize;
    if limit > 0 && merged.len() > limit as usize {
        let overflow = merged.len() - limit as usize;
        merged.drain(0..overflow);
    }
    *transcript = merged;
    trimmed
}

fn raw_history_page_from_loaded_history(mut history: Vec<Value>, limit: i64) -> RawHistoryPage {
    let has_more = limit > 0 && history.len() as i64 > limit;
    if has_more && !history.is_empty() {
        history.remove(0);
    }
    let before_id = history
        .first()
        .and_then(|item| item.get("_history_id"))
        .and_then(Value::as_i64);
    RawHistoryPage {
        history,
        has_more,
        before_id,
    }
}

fn history_page_cursor_after_merge(
    transcript_was_trimmed: bool,
    transcript: &[Value],
    raw_before_id: Option<i64>,
) -> Option<i64> {
    if transcript_was_trimmed {
        oldest_history_id_from_transcript(transcript).or(raw_before_id)
    } else {
        raw_before_id
    }
}

fn load_visible_transcript_page(
    state: &AppState,
    user_id: &str,
    session_id: &str,
    before_id: Option<i64>,
    limit: i64,
    message_feedback: &HashMap<i64, Value>,
) -> anyhow::Result<VisibleTranscriptPage> {
    if limit <= 0 {
        let history = state.workspace.load_history(user_id, session_id, limit)?;
        let history = filter_orchestration_suppressed_history(state, user_id, session_id, history);
        return Ok(VisibleTranscriptPage {
            transcript: build_chat_transcript(session_id, history, message_feedback),
            history_has_more: false,
            history_before_id: None,
        });
    }

    let mut cursor = before_id;
    let mut raw_has_more = true;
    let mut visible_window_trimmed = false;
    let mut transcript = Vec::new();
    let mut fetch_count = 0usize;
    while transcript.len() < limit as usize
        && raw_has_more
        && fetch_count < MAX_VISIBLE_HISTORY_PAGE_FETCHES
    {
        fetch_count += 1;
        let page = raw_history_page_from_loaded_history(
            state.storage.load_chat_history_page(
                user_id,
                session_id,
                cursor,
                limit.saturating_add(1),
            )?,
            limit,
        );
        raw_has_more = page.has_more;
        let next_cursor = page.before_id;
        if page.history.is_empty() {
            cursor = next_cursor;
            break;
        }
        let history =
            filter_orchestration_suppressed_history(state, user_id, session_id, page.history);
        let page_transcript = build_chat_transcript(session_id, history, message_feedback);
        if page_transcript.is_empty() {
            if next_cursor.is_none() || next_cursor == cursor {
                cursor = next_cursor;
                break;
            }
            cursor = next_cursor;
            continue;
        }
        let trimmed = merge_visible_transcript_page(&mut transcript, page_transcript, limit);
        visible_window_trimmed = visible_window_trimmed || trimmed;
        cursor = history_page_cursor_after_merge(trimmed, &transcript, next_cursor);
    }

    let history_before_id = cursor.or_else(|| oldest_history_id_from_transcript(&transcript));
    Ok(VisibleTranscriptPage {
        transcript,
        history_has_more: (raw_has_more || visible_window_trimmed) && history_before_id.is_some(),
        history_before_id,
    })
}

async fn get_session_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<HistoryPageQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let limit = normalize_history_page_limit(query.limit);
    let before_id = normalize_history_before_id(query.before_id.as_deref());
    let monitor_record = state.monitor.get_record(&session_id);
    let message_feedback = extract_monitor_message_feedback_map(monitor_record.as_ref());
    let transcript_page = match load_visible_transcript_page(
        state.as_ref(),
        &resolved.user.user_id,
        &session_id,
        before_id,
        limit,
        &message_feedback,
    ) {
        Ok(page) => page,
        Err(err) => {
            warn!(
                "load history page failed: user_id={}, session_id={}, error={err}",
                resolved.user.user_id, session_id
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
            ));
        }
    };
    let transcript = transcript_page
        .transcript
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            if let Value::Object(mut map) = item {
                map.insert("turn_index".to_string(), json!((index + 1) as i64));
                return Value::Object(map);
            }
            item
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "id": session_id,
            "transcript": transcript,
            "history_has_more": transcript_page.history_has_more,
            "history_before_id": transcript_page.history_before_id
        }
    })))
}

async fn submit_message_feedback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, history_id)): AxumPath<(String, i64)>,
    Json(payload): Json<MessageFeedbackRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() || history_id <= 0 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let vote = normalize_message_feedback_vote(&payload.vote).ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid vote, expected up/down".to_string(),
        )
    })?;
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let target_history = state
        .workspace
        .load_history_page(
            &resolved.user.user_id,
            &session_id,
            Some(history_id.saturating_add(1)),
            1,
        )
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .into_iter()
        .find(|item| {
            item.get("_history_id")
                .and_then(Value::as_i64)
                .map(|value| value == history_id)
                .unwrap_or(false)
        })
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    let target_role = target_history
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("");
    if target_role != "assistant" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "feedback only supports assistant messages".to_string(),
        ));
    }

    let outcome =
        state
            .monitor
            .set_message_feedback(&session_id, history_id, vote, &resolved.user.user_id);
    match outcome {
        crate::monitor::SetMessageFeedbackResult::Applied(item) => Ok(Json(json!({
            "data": {
                "session_id": session_id,
                "history_id": history_id,
                "feedback": {
                    "vote": item.vote,
                    "created_at": format_ts(item.created_time),
                    "locked": true,
                }
            }
        }))),
        crate::monitor::SetMessageFeedbackResult::AlreadyExists(_item) => Err(error_response(
            StatusCode::CONFLICT,
            "feedback already submitted".to_string(),
        )),
        crate::monitor::SetMessageFeedbackResult::SessionNotFound => Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.session_not_found"),
        )),
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    reject_locked_orchestration_session(state.as_ref(), &resolved.user.user_id, &session_id)?;
    state
        .workspace
        .purge_session_data(&resolved.user.user_id, &session_id);
    let _ = state
        .storage
        .delete_cron_jobs_by_session(&resolved.user.user_id, &session_id);
    let _ = state
        .storage
        .delete_session_goal(&resolved.user.user_id, &session_id);
    let _ = state
        .memory
        .delete_record(&resolved.user.user_id, &session_id);
    let _ = state.monitor.purge_session(&session_id);
    let _ = state
        .user_store
        .delete_chat_session(&resolved.user.user_id, &session_id);
    Ok(Json(json!({ "data": { "id": session_id } })))
}

async fn update_session_title(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SessionTitleUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let title = payload.title.trim();
    if title.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let now = now_ts();
    record.title = title.to_string();
    record.updated_at = now;
    if !record
        .status
        .trim()
        .eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED)
    {
        record.status = CHAT_SESSION_STATUS_ACTIVE.to_string();
    }
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let is_main = resolve_session_main_flag(
        &state,
        &resolved.user.user_id,
        record.agent_id.as_deref(),
        &session_id,
    );
    let config = state.config_store.get().await;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, record.agent_id.as_deref(), true).await?;
    let runtime = resolve_session_model_runtime(&config, agent_record.as_ref());
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let mut payload = session_payload_with_main(&record, is_main);
    insert_session_orchestration_lock_fields(
        &mut payload,
        &state,
        &resolved.user.user_id,
        &session_id,
    );
    insert_session_goal_payload(&mut payload, goal.as_ref());
    insert_session_runtime_fields(
        &mut payload,
        runtime.as_ref(),
        state
            .workspace
            .load_session_context_tokens(&resolved.user.user_id, &session_id),
    );
    Ok(Json(json!({ "data": payload })))
}

async fn archive_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    reject_locked_orchestration_session(state.as_ref(), &resolved.user.user_id, &session_id)?;
    let now = now_ts();
    record.status = CHAT_SESSION_STATUS_ARCHIVED.to_string();
    record.updated_at = now;
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let agent_key = record.agent_id.as_deref().unwrap_or("").trim().to_string();
    let is_current_main = resolve_session_main_flag(
        &state,
        &resolved.user.user_id,
        Some(&agent_key),
        &session_id,
    );
    if is_current_main {
        let (fallback_sessions, _) = state
            .user_store
            .list_chat_sessions_by_status(
                &resolved.user.user_id,
                Some(agent_key.as_str()),
                None,
                Some(CHAT_SESSION_STATUS_ACTIVE),
                0,
                32,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if let Some(fallback) = fallback_sessions
            .into_iter()
            .find(|item| item.session_id != session_id)
        {
            let _ = state
                .kernel
                .thread_runtime
                .set_main_session(
                    &resolved.user.user_id,
                    agent_key.as_str(),
                    &fallback.session_id,
                    "archive",
                )
                .await;
        }
    }
    let is_main = resolve_session_main_flag(
        &state,
        &resolved.user.user_id,
        record.agent_id.as_deref(),
        &session_id,
    );
    let config = state.config_store.get().await;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, record.agent_id.as_deref(), true).await?;
    let runtime = resolve_session_model_runtime(&config, agent_record.as_ref());
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let mut payload = session_payload_with_main(&record, is_main);
    insert_session_orchestration_lock_fields(
        &mut payload,
        &state,
        &resolved.user.user_id,
        &session_id,
    );
    insert_session_goal_payload(&mut payload, goal.as_ref());
    insert_session_runtime_fields(
        &mut payload,
        runtime.as_ref(),
        state
            .workspace
            .load_session_context_tokens(&resolved.user.user_id, &session_id),
    );
    Ok(Json(json!({ "data": payload })))
}

async fn restore_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    reject_locked_orchestration_session(state.as_ref(), &resolved.user.user_id, &session_id)?;
    let now = now_ts();
    record.status = CHAT_SESSION_STATUS_ACTIVE.to_string();
    record.updated_at = now;
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let agent_key = record.agent_id.as_deref().unwrap_or("").trim().to_string();
    let main_thread = state
        .user_store
        .get_agent_thread(&resolved.user.user_id, agent_key.as_str())
        .ok()
        .flatten();
    let should_rebind_main = match main_thread {
        None => true,
        Some(thread) => {
            let thread_session_id = thread.session_id.trim().to_string();
            if thread_session_id.is_empty() {
                true
            } else {
                let thread_is_active = state
                    .user_store
                    .get_chat_session(&resolved.user.user_id, thread_session_id.as_str())
                    .ok()
                    .flatten()
                    .map(|item| {
                        !item
                            .status
                            .trim()
                            .eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED)
                    })
                    .unwrap_or(false);
                !thread_is_active
            }
        }
    };
    if should_rebind_main {
        let _ = state
            .kernel
            .thread_runtime
            .set_main_session(
                &resolved.user.user_id,
                agent_key.as_str(),
                &record.session_id,
                "restore",
            )
            .await;
    }

    let is_main = resolve_session_main_flag(
        &state,
        &resolved.user.user_id,
        record.agent_id.as_deref(),
        &session_id,
    );
    let config = state.config_store.get().await;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, record.agent_id.as_deref(), true).await?;
    let runtime = resolve_session_model_runtime(&config, agent_record.as_ref());
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let mut payload = session_payload_with_main(&record, is_main);
    insert_session_orchestration_lock_fields(
        &mut payload,
        &state,
        &resolved.user.user_id,
        &session_id,
    );
    insert_session_runtime_fields(
        &mut payload,
        runtime.as_ref(),
        state
            .workspace
            .load_session_context_tokens(&resolved.user.user_id, &session_id),
    );
    insert_session_goal_payload(&mut payload, goal.as_ref());
    Ok(Json(json!({ "data": payload })))
}

fn normalize_message_feedback_vote(raw: &str) -> Option<&'static str> {
    let value = raw.trim().to_ascii_lowercase();
    match value.as_str() {
        "up" | "like" | "thumb_up" | "thumbs_up" => Some("up"),
        "down" | "dislike" | "thumb_down" | "thumbs_down" => Some("down"),
        _ => None,
    }
}

fn normalize_history_page_limit(raw: Option<i64>) -> i64 {
    let value = raw.unwrap_or(80);
    if value <= 0 {
        80
    } else {
        value.min(200)
    }
}

fn normalize_history_before_id(raw: Option<&str>) -> Option<i64> {
    raw.and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
}

fn is_session_stream_active(status: &str) -> bool {
    matches!(
        status,
        MonitorState::STATUS_RUNNING
            | MonitorState::STATUS_CANCELLING
            | MonitorState::STATUS_WAITING
    )
}

fn is_active_queue_task_status(status: &str) -> bool {
    matches!(status, "pending" | "retry" | "running")
}

fn list_active_queue_tasks(
    user_store: &UserStore,
    session_id: &str,
) -> Vec<crate::storage::AgentTaskRecord> {
    let cleaned_session = session_id.trim();
    if cleaned_session.is_empty() {
        return Vec::new();
    }
    let thread_id = format!("thread_{cleaned_session}");
    let mut tasks = user_store
        .list_agent_tasks_by_thread(&thread_id, None, 16)
        .map(|items| {
            items
                .into_iter()
                .filter(|task| is_active_queue_task_status(task.status.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tasks.sort_by(|left, right| {
        left.created_at
            .partial_cmp(&right.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.updated_at
                    .partial_cmp(&right.updated_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    tasks
}

fn has_active_queue_task(user_store: &UserStore, session_id: &str) -> bool {
    !list_active_queue_tasks(user_store, session_id).is_empty()
}

pub(super) fn is_session_stream_active_or_queued(
    user_store: &UserStore,
    monitor_status: Option<&str>,
    session_id: &str,
) -> bool {
    monitor_status
        .map(is_session_stream_active)
        .unwrap_or(false)
        || has_active_queue_task(user_store, session_id)
}

pub(super) fn is_session_runtime_active(runtime: Option<&Value>) -> bool {
    matches!(
        runtime
            .and_then(|snapshot| snapshot.get("thread_status"))
            .and_then(Value::as_str),
        Some("running" | "waiting_approval" | "waiting_user_input")
    )
}

fn normalize_queue_task_attachments(value: Option<&Value>) -> Option<Value> {
    match value {
        Some(Value::Array(items)) if items.is_empty() => None,
        Some(Value::Null) | None => None,
        Some(other) => Some(other.clone()),
    }
}

fn latest_trailing_user_matches_queue_task(
    messages: &[Value],
    task: &crate::storage::AgentTaskRecord,
) -> bool {
    let Some(last_message) = messages.last() else {
        return false;
    };
    if last_message.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let task_question = task
        .request_payload
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let message_content = last_message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if task_question != Some(message_content) {
        return false;
    }
    normalize_queue_task_attachments(last_message.get("attachments"))
        == normalize_queue_task_attachments(task.request_payload.get("attachments"))
}

fn build_projected_queue_user_message(task: &crate::storage::AgentTaskRecord) -> Option<Value> {
    let content = task
        .request_payload
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut message = json!({
        "role": "user",
        "content": content,
        "created_at": format_ts(task.created_at),
        "message_id": format!("queue:{}:user", task.task_id),
        "user_turn_id": format!("queue-turn:{}:user", task.task_id),
        "turn_index": i64::MAX - 1,
        "status": "queued",
    });
    if let Some(attachments) =
        normalize_queue_task_attachments(task.request_payload.get("attachments"))
    {
        if let Value::Object(ref mut map) = message {
            map.insert("attachments".to_string(), attachments);
        }
    }
    Some(message)
}

fn build_projected_queue_assistant_message(task: &crate::storage::AgentTaskRecord) -> Value {
    let workflow_events = build_projected_queue_workflow_events(task);
    let mut message = json!({
        "role": "assistant",
        "content": "",
        "created_at": format_ts(task.created_at),
        "message_id": format!("queue:{}:assistant", task.task_id),
        "user_turn_id": format!("queue-turn:{}:user", task.task_id),
        "model_turn_id": format!("queue-turn:{}:model", task.task_id),
        "turn_index": i64::MAX,
        "status": "streaming",
        "stream_incomplete": true,
    });
    if !workflow_events.is_empty() {
        if let Value::Object(ref mut map) = message {
            map.insert("workflow_events".to_string(), Value::Array(workflow_events));
        }
    }
    message
}

fn build_projected_queue_workflow_events(task: &crate::storage::AgentTaskRecord) -> Vec<Value> {
    let mut events = vec![build_projected_queue_workflow_event(
        task,
        "queue_enter",
        task.created_at,
    )];
    if task.status == "running" {
        let queue_start_ts = task
            .started_at
            .unwrap_or(task.updated_at.max(task.created_at));
        events.push(build_projected_queue_workflow_event(
            task,
            "queue_start",
            queue_start_ts,
        ));
    }
    events
}

fn build_projected_queue_workflow_event(
    task: &crate::storage::AgentTaskRecord,
    event_type: &str,
    timestamp: f64,
) -> Value {
    let queue_ahead = task
        .request_payload
        .get("queue_ahead")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let queue_total = task
        .request_payload
        .get("queue_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut data = json!({
        "queue_id": task.task_id,
        "thread_id": task.thread_id,
        "session_id": task.session_id,
        "agent_id": task.agent_id,
        "user_id": task.user_id,
        "retry_count": task.retry_count,
        "queue_ahead": queue_ahead,
        "queue_total": queue_total,
    });
    if let Value::Object(ref mut map) = data {
        map.insert("status".to_string(), json!(task.status));
    }
    json!({
        "event": event_type,
        "timestamp": format_ts(timestamp),
        "data": data,
    })
}

fn project_queued_session_messages(
    messages: &mut Vec<Value>,
    active_queue_tasks: &[crate::storage::AgentTaskRecord],
    pure_queue_phase: bool,
) {
    if !pure_queue_phase || active_queue_tasks.is_empty() {
        return;
    }
    for task in active_queue_tasks {
        if latest_trailing_user_matches_queue_task(messages, task) {
            messages.push(build_projected_queue_assistant_message(task));
            continue;
        }
        let Some(user_message) = build_projected_queue_user_message(task) else {
            continue;
        };
        messages.push(user_message);
        messages.push(build_projected_queue_assistant_message(task));
    }
}

fn apply_session_running_state(
    messages: &mut Vec<Value>,
    session_running: bool,
    turn_hint: RunningTurnHint,
) {
    if !session_running {
        return;
    }
    if let Some(user_turn_id) = resolve_running_user_turn_id(messages, turn_hint) {
        let model_turn_id = resolve_running_model_turn_id(user_turn_id.as_str(), turn_hint);
        if mark_existing_running_message(messages, user_turn_id.as_str(), model_turn_id.as_str()) {
            return;
        }
        let mut placeholder = json!({
            "role": "assistant",
            "content": "",
            "created_at": format_ts(now_ts()),
            "message_id": format!("runtime:{user_turn_id}:assistant"),
            "user_turn_id": user_turn_id,
            "model_turn_id": model_turn_id,
            "turn_index": i64::MAX,
            "status": "streaming",
            "stream_incomplete": true,
        });
        if let Value::Object(ref mut map) = placeholder {
            if let Some(user_round) = turn_hint.user_round {
                map.insert("user_turn_index".to_string(), json!(user_round));
                map.insert("user_round".to_string(), json!(user_round));
            }
            if let Some(model_round) = turn_hint.model_round {
                map.insert("model_turn_index".to_string(), json!(model_round));
                map.insert("model_round".to_string(), json!(model_round));
            }
        }
        let insert_at = running_placeholder_insert_index(messages, user_turn_id.as_str());
        messages.insert(insert_at, placeholder);
        return;
    }

    let last_role = messages
        .last()
        .and_then(|item| item.get("role").and_then(Value::as_str))
        .unwrap_or("");
    if last_role == "user" || messages.is_empty() {
        let user_turn_id = messages
            .last()
            .and_then(|item| item.get("user_turn_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "runtime-turn:orphan".to_string());
        messages.push(json!({
            "role": "assistant",
            "content": "",
            "created_at": format_ts(now_ts()),
            "message_id": format!("runtime:{user_turn_id}:assistant"),
            "user_turn_id": user_turn_id,
            "model_turn_id": format!("runtime:{user_turn_id}:model"),
            "turn_index": i64::MAX,
            "status": "streaming",
            "stream_incomplete": true,
        }));
    } else if let Some(Value::Object(map)) = messages.iter_mut().rev().find(|item| {
        item.get("role")
            .and_then(Value::as_str)
            .map(|value| value == "assistant")
            .unwrap_or(false)
    }) {
        map.insert("stream_incomplete".to_string(), json!(true));
    }
}

fn mark_existing_running_message(
    messages: &mut [Value],
    user_turn_id: &str,
    model_turn_id: &str,
) -> bool {
    let Some(Value::Object(map)) = messages.iter_mut().rev().find(|item| {
        item.get("role")
            .and_then(Value::as_str)
            .map(|value| value == "assistant")
            .unwrap_or(false)
            && item
                .get("user_turn_id")
                .and_then(Value::as_str)
                .map(|value| value == user_turn_id)
                .unwrap_or(false)
            && item
                .get("model_turn_id")
                .and_then(Value::as_str)
                .map(|value| value == model_turn_id)
                .unwrap_or(model_turn_id.is_empty())
    }) else {
        return false;
    };
    map.insert("stream_incomplete".to_string(), json!(true));
    map.insert("status".to_string(), json!("streaming"));
    true
}

fn running_placeholder_insert_index(messages: &[Value], user_turn_id: &str) -> usize {
    let mut insert_at = None;
    for (index, item) in messages.iter().enumerate() {
        let item_user_turn_id = item.get("user_turn_id").and_then(Value::as_str);
        if item_user_turn_id == Some(user_turn_id) {
            insert_at = Some(index + 1);
            continue;
        }
        if insert_at.is_some() && item_user_turn_id.is_some() {
            break;
        }
    }
    insert_at.unwrap_or(messages.len())
}

fn resolve_running_model_turn_id(user_turn_id: &str, turn_hint: RunningTurnHint) -> String {
    let Some(model_round) = turn_hint.model_round else {
        return format!("runtime:{user_turn_id}:model");
    };
    let Some(user_round) = turn_hint.user_round else {
        return format!("runtime:{user_turn_id}:model:{model_round}");
    };
    let Some(session_id) = user_turn_id
        .strip_prefix("user-turn:")
        .and_then(|rest| rest.strip_suffix(format!(":round:{user_round}").as_str()))
        .map(str::to_string)
    else {
        return format!("runtime:{user_turn_id}:model:{model_round}");
    };
    format!("model-turn:{session_id}:user:{user_round}:model:{model_round}")
}

fn resolve_running_user_turn_id(messages: &[Value], turn_hint: RunningTurnHint) -> Option<String> {
    let user_round = turn_hint.user_round?;
    messages
        .iter()
        .find(|item| {
            item.get("role")
                .and_then(Value::as_str)
                .map(|role| role == "user")
                .unwrap_or(false)
                && item
                    .get("user_turn_index")
                    .and_then(Value::as_i64)
                    .map(|index| index == user_round)
                    .unwrap_or(false)
        })
        .and_then(|item| item.get("user_turn_id").and_then(Value::as_str))
        .map(str::to_string)
}

fn running_turn_hint(record: Option<&Value>) -> RunningTurnHint {
    let mut hint = RunningTurnHint::default();
    let Some(record) = record else {
        return hint;
    };
    if let Some(events) = record.get("events").and_then(Value::as_array) {
        for event in events.iter().rev() {
            let data = event.get("data").unwrap_or(event);
            let Some(user_round) = positive_i64(data.get("user_round")) else {
                continue;
            };
            hint.user_round = Some(user_round);
            hint.model_round = positive_i64(data.get("model_round"));
            break;
        }
        for event in events.iter().rev() {
            let data = event.get("data").unwrap_or(event);
            if hint.user_round.is_some() && positive_i64(data.get("user_round")) != hint.user_round
            {
                continue;
            }
            if hint.model_round.is_none() {
                hint.model_round = positive_i64(data.get("model_round"));
            }
            if hint.model_round.is_some() {
                break;
            }
        }
    }
    if hint.user_round.is_none() {
        hint.user_round =
            positive_i64(record.get("user_rounds")).or_else(|| positive_i64(record.get("rounds")));
    }
    hint
}

fn positive_i64(value: Option<&Value>) -> Option<i64> {
    let parsed = value.and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    })?;
    (parsed > 0).then_some(parsed)
}

fn extract_monitor_message_feedback_map(record: Option<&Value>) -> HashMap<i64, Value> {
    let mut feedback_map = HashMap::new();
    let Some(record) = record else {
        return feedback_map;
    };
    let Some(items) = record.get("message_feedback").and_then(Value::as_object) else {
        return feedback_map;
    };
    for (raw_history_id, raw_feedback) in items {
        let Ok(history_id) = raw_history_id.parse::<i64>() else {
            continue;
        };
        if history_id <= 0 {
            continue;
        }
        let Some(normalized) = normalize_monitor_message_feedback(raw_feedback) else {
            continue;
        };
        feedback_map.insert(history_id, normalized);
    }
    feedback_map
}

fn normalize_monitor_message_feedback(raw: &Value) -> Option<Value> {
    let vote = normalize_message_feedback_vote(
        raw.get("vote").and_then(Value::as_str).unwrap_or_default(),
    )?;
    let mut feedback = json!({
        "vote": vote,
        "locked": true,
    });
    let created_time = raw
        .get("created_time")
        .or_else(|| raw.get("created_at"))
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        });
    if let Some(created_time) = created_time {
        if created_time > 0.0 {
            if let Value::Object(ref mut map) = feedback {
                map.insert("created_at".to_string(), json!(format_ts(created_time)));
            }
        }
    }
    Some(feedback)
}

fn filter_orchestration_suppressed_history(
    state: &AppState,
    user_id: &str,
    session_id: &str,
    history: Vec<Value>,
) -> Vec<Value> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return history;
    }
    let Some(context) =
        load_session_context(state.storage.as_ref(), cleaned_user_id, cleaned_session_id)
    else {
        return history;
    };
    let agent_id = context.mother_agent_id.trim();
    if agent_id.is_empty() {
        return history;
    }
    let Some((lock_state, binding)) =
        active_orchestration_for_agent(state.storage.as_ref(), cleaned_user_id, agent_id)
    else {
        return history;
    };
    if binding.session_id.trim() != cleaned_session_id {
        return history;
    }
    let Some(round_state) = load_round_state(
        state.storage.as_ref(),
        cleaned_user_id,
        &lock_state.orchestration_id,
    ) else {
        return history;
    };
    history
        .into_iter()
        .filter(|item| {
            let created_at = item
                .get("created_at")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            !round_state.suppressed_message_ranges.iter().any(|range| {
                created_at > 0.0 && created_at >= range.start_at && created_at <= range.end_at
            })
        })
        .collect()
}

fn resolve_session_main_flag(
    state: &Arc<AppState>,
    user_id: &str,
    agent_id: Option<&str>,
    session_id: &str,
) -> bool {
    let agent_key = agent_id.unwrap_or("").trim();
    state
        .user_store
        .get_agent_thread(user_id, agent_key)
        .ok()
        .flatten()
        .map(|thread| thread.session_id == session_id)
        .unwrap_or(false)
}

fn session_payload(record: &crate::storage::ChatSessionRecord) -> Value {
    json!({
        "id": record.session_id,
        "title": record.title,
        "status": record.status,
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
        "last_message_at": format_ts(record.last_message_at),
        "agent_id": record.agent_id,
        "tool_overrides": record.tool_overrides,
        "parent_session_id": record.parent_session_id,
        "parent_message_id": record.parent_message_id,
        "spawn_label": record.spawn_label,
        "spawned_by": record.spawned_by,
    })
}

fn session_payload_with_main(record: &crate::storage::ChatSessionRecord, is_main: bool) -> Value {
    let mut payload = session_payload(record);
    if let Value::Object(ref mut map) = payload {
        map.insert("is_main".to_string(), json!(is_main));
    }
    payload
}

fn insert_session_orchestration_lock_fields(
    payload: &mut Value,
    state: &Arc<AppState>,
    user_id: &str,
    session_id: &str,
) {
    let Value::Object(map) = payload else {
        return;
    };
    if let Some((lock_state, lock_binding)) =
        session_orchestration_lock_info(state.storage.as_ref(), user_id, session_id)
    {
        map.insert(
            "orchestration_lock".to_string(),
            json!({
                "active": true,
                "group_id": lock_state.group_id,
                "orchestration_id": lock_state.orchestration_id,
                "run_id": lock_state.run_id,
                "mother_agent_id": lock_state.mother_agent_id,
                "role": lock_binding.role,
            }),
        );
    }
}

fn insert_session_goal_payload(
    payload: &mut Value,
    goal: Option<&crate::storage::SessionGoalRecord>,
) {
    let Value::Object(map) = payload else {
        return;
    };
    map.insert(
        "goal".to_string(),
        goal.map(crate::services::goal::goal_payload)
            .unwrap_or(Value::Null),
    );
}

fn insert_session_runtime_fields(
    payload: &mut Value,
    runtime: Option<&SessionModelRuntime>,
    context_tokens: i64,
) {
    let Value::Object(map) = payload else {
        return;
    };
    if let Some(model_name) = runtime
        .and_then(|runtime| runtime.display_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("model_name".to_string(), json!(model_name));
    }
    if let Some(model_key) = runtime
        .and_then(|runtime| runtime.config_key.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("model_key".to_string(), json!(model_key));
    }
    if let Some(max_context) = runtime
        .and_then(|runtime| runtime.max_context)
        .filter(|value| *value > 0)
    {
        map.insert("context_max_tokens".to_string(), json!(max_context));
        map.insert("context_total_tokens".to_string(), json!(max_context));
        map.insert("max_context".to_string(), json!(max_context));
    }
    let context_tokens = context_tokens.max(0);
    map.insert("context_tokens".to_string(), json!(context_tokens));
    map.insert(
        "context_occupancy_tokens".to_string(),
        json!(context_tokens),
    );
}

fn resolve_session_model_runtime(
    config: &crate::config::Config,
    agent_record: Option<&crate::storage::UserAgentRecord>,
) -> Option<SessionModelRuntime> {
    let config_key = resolve_chat_model_name(config, agent_record)?;
    let model_config = config.llm.models.get(&config_key)?;
    if !is_llm_model(model_config) {
        return None;
    }
    let display_name = model_config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(config_key.as_str())
        .to_string();
    Some(SessionModelRuntime {
        config_key: Some(config_key),
        display_name: Some(display_name),
        max_context: model_config.max_context.filter(|value| *value > 0),
    })
}

async fn resolve_cached_session_model_runtime(
    config: &crate::config::Config,
    cache: &mut HashMap<String, Option<crate::storage::UserAgentRecord>>,
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    agent_id: Option<&str>,
) -> Result<Option<SessionModelRuntime>, Response> {
    let agent_key = agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("__default__")
        .to_string();
    if !cache.contains_key(&agent_key) {
        let record = fetch_agent_record(state, user, agent_id, true).await?;
        cache.insert(agent_key.clone(), record);
    }
    Ok(resolve_session_model_runtime(
        config,
        cache.get(&agent_key).and_then(|record| record.as_ref()),
    ))
}

fn resolve_pagination(query: &SessionListQuery) -> (i64, i64) {
    if let (Some(page), Some(size)) = (query.page, query.page_size) {
        let safe_page = page.max(1);
        let safe_size = size.clamp(1, 200);
        return ((safe_page - 1) * safe_size, safe_size);
    }
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    (offset, limit)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_session_running_state, build_projected_queue_assistant_message,
        build_projected_queue_user_message, has_active_queue_task, history_page_cursor_after_merge,
        is_session_stream_active_or_queued, merge_visible_transcript_page,
        normalize_history_before_id, project_queued_session_messages,
        raw_history_page_from_loaded_history, RunningTurnHint,
    };
    use crate::storage::{AgentTaskRecord, SqliteStorage, StorageBackend};
    use crate::user_store::UserStore;
    use serde_json::json;
    use serde_json::Value;
    use std::sync::Arc;

    fn build_user_store() -> UserStore {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_chat_queue_active_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        UserStore::new(storage)
    }

    #[test]
    fn normalize_history_before_id_ignores_invalid_cursor_values() {
        assert_eq!(normalize_history_before_id(Some("42")), Some(42));
        assert_eq!(normalize_history_before_id(Some(" 42 ")), Some(42));
        assert_eq!(normalize_history_before_id(Some("NaN")), None);
        assert_eq!(normalize_history_before_id(Some("0")), None);
        assert_eq!(normalize_history_before_id(Some("-1")), None);
        assert_eq!(normalize_history_before_id(None), None);
    }

    #[test]
    fn visible_transcript_page_merge_backfills_without_dropping_recent_window() {
        let mut transcript = vec![
            json!({"role": "user", "content": "newer-user", "history_id": 40}),
            json!({"role": "assistant", "content": "newer-assistant", "history_id": 41}),
        ];
        let trimmed = merge_visible_transcript_page(
            &mut transcript,
            vec![
                json!({"role": "user", "content": "older-user", "history_id": 20}),
                json!({"role": "assistant", "content": "older-assistant", "history_id": 21}),
            ],
            3,
        );

        assert!(trimmed);
        let contents = transcript
            .iter()
            .map(|item| item["content"].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            vec!["older-assistant", "newer-user", "newer-assistant"]
        );
    }

    #[test]
    fn raw_history_page_uses_oldest_raw_id_as_cursor_after_sentinel_trim() {
        let page = raw_history_page_from_loaded_history(
            vec![
                json!({"role": "system", "content": "hidden", "_history_id": 7}),
                json!({"role": "tool", "content": "hidden", "_history_id": 8}),
                json!({"role": "assistant", "content": "visible", "_history_id": 9}),
            ],
            2,
        );

        assert!(page.has_more);
        assert_eq!(page.before_id, Some(8));
        assert_eq!(page.history.len(), 2);
    }

    #[test]
    fn history_cursor_advances_past_scanned_raw_rows_when_visible_window_not_trimmed() {
        let transcript = vec![json!({"role": "assistant", "content": "visible", "history_id": 9})];

        assert_eq!(
            history_page_cursor_after_merge(false, &transcript, Some(7)),
            Some(7)
        );
        assert_eq!(
            history_page_cursor_after_merge(true, &transcript, Some(7)),
            Some(9)
        );
    }

    #[test]
    fn queue_task_counts_as_active_stream_state() {
        let store = build_user_store();
        store
            .insert_agent_task(&AgentTaskRecord {
                task_id: "task_1".to_string(),
                thread_id: "thread_sess_active".to_string(),
                user_id: "user_a".to_string(),
                agent_id: "agent_a".to_string(),
                session_id: "sess_active".to_string(),
                status: "pending".to_string(),
                request_payload: json!({}),
                request_id: None,
                retry_count: 0,
                retry_at: 1.0,
                created_at: 1.0,
                updated_at: 1.0,
                started_at: None,
                finished_at: None,
                last_error: None,
            })
            .expect("insert agent task");

        assert!(has_active_queue_task(&store, "sess_active"));
        assert!(is_session_stream_active_or_queued(
            &store,
            None,
            "sess_active"
        ));
    }

    #[test]
    fn finished_queue_task_does_not_keep_stream_active() {
        let store = build_user_store();
        store
            .insert_agent_task(&AgentTaskRecord {
                task_id: "task_2".to_string(),
                thread_id: "thread_sess_done".to_string(),
                user_id: "user_b".to_string(),
                agent_id: "agent_b".to_string(),
                session_id: "sess_done".to_string(),
                status: "success".to_string(),
                request_payload: json!({}),
                request_id: None,
                retry_count: 0,
                retry_at: 1.0,
                created_at: 1.0,
                updated_at: 1.0,
                started_at: Some(1.0),
                finished_at: Some(2.0),
                last_error: None,
            })
            .expect("insert completed agent task");

        assert!(!has_active_queue_task(&store, "sess_done"));
        assert!(!is_session_stream_active_or_queued(
            &store,
            None,
            "sess_done"
        ));
    }

    fn build_queue_task(
        task_id: &str,
        session_id: &str,
        status: &str,
        question: &str,
        created_at: f64,
    ) -> AgentTaskRecord {
        AgentTaskRecord {
            task_id: task_id.to_string(),
            thread_id: format!("thread_{session_id}"),
            user_id: "user_queue".to_string(),
            agent_id: "agent_queue".to_string(),
            session_id: session_id.to_string(),
            status: status.to_string(),
            request_payload: json!({
                "question": question,
                "queue_ahead": 2,
                "queue_total": 3,
                "attachments": [
                    {
                        "name": "note.txt",
                        "content": "hello"
                    }
                ]
            }),
            request_id: None,
            retry_count: 0,
            retry_at: created_at,
            created_at,
            updated_at: created_at,
            started_at: None,
            finished_at: None,
            last_error: None,
        }
    }

    #[test]
    fn pure_queue_projection_appends_pending_turn_without_touching_previous_assistant() {
        let task = build_queue_task(
            "task_proj_1",
            "sess_proj_1",
            "pending",
            "new queued turn",
            3.0,
        );
        let mut messages = vec![
            json!({
                "role": "user",
                "content": "old question",
                "created_at": "2026-03-26T10:00:00+08:00"
            }),
            json!({
                "role": "assistant",
                "content": "old answer",
                "created_at": "2026-03-26T10:00:01+08:00"
            }),
        ];

        project_queued_session_messages(&mut messages, &[task], true);
        apply_session_running_state(&mut messages, true, RunningTurnHint::default());

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_ne!(messages[1]["stream_incomplete"], json!(true));
        assert_eq!(messages[2]["role"], json!("user"));
        assert_eq!(messages[2]["content"], json!("new queued turn"));
        assert_eq!(
            messages[2]["attachments"],
            json!([
                {
                    "name": "note.txt",
                    "content": "hello"
                }
            ])
        );
        assert_eq!(messages[3]["role"], json!("assistant"));
        assert_eq!(messages[3]["stream_incomplete"], json!(true));
    }

    #[test]
    fn pure_queue_projection_reuses_trailing_user_message() {
        let task = build_queue_task(
            "task_proj_2",
            "sess_proj_2",
            "retry",
            "already persisted",
            5.0,
        );
        let mut messages = vec![json!({
            "role": "user",
            "content": "already persisted",
            "created_at": "2026-03-26T10:00:05+08:00",
            "attachments": [
                {
                    "name": "note.txt",
                    "content": "hello"
                }
            ]
        })];

        project_queued_session_messages(&mut messages, &[task], true);
        apply_session_running_state(&mut messages, true, RunningTurnHint::default());

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], json!("user"));
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_eq!(messages[1]["stream_incomplete"], json!(true));
    }

    #[test]
    fn running_state_uses_monitor_user_round_hint() {
        let mut messages = vec![
            json!({
                "role": "user",
                "content": "first",
                "user_turn_id": "user-turn:sess_run:round:1",
                "user_turn_index": 1,
            }),
            json!({
                "role": "user",
                "content": "second",
                "user_turn_id": "user-turn:sess_run:round:2",
                "user_turn_index": 2,
            }),
        ];

        apply_session_running_state(
            &mut messages,
            true,
            RunningTurnHint {
                user_round: Some(1),
                model_round: Some(2),
            },
        );

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_eq!(
            messages[1]["user_turn_id"],
            json!("user-turn:sess_run:round:1")
        );
        assert_eq!(
            messages[1]["model_turn_id"],
            json!("model-turn:sess_run:user:1:model:2")
        );
        assert_eq!(messages[2]["content"], json!("second"));
    }

    #[test]
    fn running_state_inserts_placeholder_after_hinted_user_round() {
        let mut messages = vec![
            json!({
                "role": "user",
                "content": "first",
                "user_turn_id": "user-turn:sess_run:round:1",
                "user_turn_index": 1,
            }),
            json!({
                "role": "user",
                "content": "second",
                "user_turn_id": "user-turn:sess_run:round:2",
                "user_turn_index": 2,
            }),
        ];

        apply_session_running_state(
            &mut messages,
            true,
            RunningTurnHint {
                user_round: Some(1),
                model_round: Some(1),
            },
        );

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["content"], json!("first"));
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_eq!(
            messages[1]["user_turn_id"],
            json!("user-turn:sess_run:round:1")
        );
        assert_eq!(messages[2]["content"], json!("second"));
    }

    #[test]
    fn projected_queue_user_message_keeps_attachment_payload() {
        let task = build_queue_task(
            "task_proj_3",
            "sess_proj_3",
            "pending",
            "queued with file",
            7.0,
        );
        let message = build_projected_queue_user_message(&task).expect("projected user message");

        assert_eq!(message["role"], json!("user"));
        assert_eq!(message["content"], json!("queued with file"));
        assert_eq!(
            message["attachments"],
            json!([
                {
                    "name": "note.txt",
                    "content": "hello"
                }
            ])
        );
    }

    #[test]
    fn projected_queue_assistant_message_contains_queue_workflow() {
        let task = build_queue_task("task_proj_4", "sess_proj_4", "pending", "queued now", 9.0);
        let message = build_projected_queue_assistant_message(&task);
        let events = message
            .get("workflow_events")
            .and_then(Value::as_array)
            .expect("workflow events");

        assert_eq!(message["role"], json!("assistant"));
        assert_eq!(message["stream_incomplete"], json!(true));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], json!("queue_enter"));
        assert_eq!(events[0]["data"]["queue_id"], json!("task_proj_4"));
        assert_eq!(events[0]["data"]["queue_ahead"], json!(2));
        assert_eq!(events[0]["data"]["queue_total"], json!(3));
    }
}
