use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::goal::{self, execute_goal_command, goal_payload, GoalCommand, GoalService};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/chat/sessions/{session_id}/goal",
        get(get_session_goal)
            .put(upsert_session_goal)
            .delete(delete_session_goal),
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoalUpsertPayload {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default, alias = "maxGoalRounds", alias = "max_goal_rounds")]
    pub max_goal_rounds: Option<i64>,
    /// Optional explicit transition: pause | resume. Without it an objective
    /// payload creates or edits the goal.
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GoalMutationOutcome {
    pub goal: Option<Value>,
}

async fn get_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    let view = goal_service(&state)
        .get_view(&state.storage, &resolved.user.user_id, &session_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "data": {
            "goal": view.as_ref().map(goal_payload)
        }
    })))
}

async fn upsert_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<GoalUpsertPayload>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    let command = goal_command_from_payload(payload)?;
    let outcome = native_session_goal_outcome(&state, &resolved.user, &session_id, command)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "data": {
            "goal": outcome.goal,
        }
    }))
    .into_response())
}

async fn delete_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    native_session_goal_outcome(&state, &resolved.user, &session_id, GoalCommand::Clear)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "data": { "deleted": true, "goal": null } })))
}

fn goal_service(state: &AppState) -> Arc<GoalService> {
    state.kernel.orchestrator.goal_handle()
}

/// In-process command entry used by the desktop façade and the CLI slash
/// surface. Persists the command echo as a durable user round and re-arms
/// the driver for create/resume.
pub async fn native_session_goal(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<Option<crate::storage::SessionGoalRecord>> {
    let outcome = native_session_goal_outcome(state, user, session_id, command).await?;
    match outcome.goal {
        Some(value) => {
            let goal_id = value
                .get("goal_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(goal_id) = goal_id {
                let storage = state.storage.clone();
                let user_id = user.user_id.clone();
                let session_id = session_id.trim().to_string();
                return crate::core::blocking::run_db("goal.load_record", move || {
                    Ok(storage
                        .get_session_goal(&user_id, &session_id)?
                        .filter(|record| record.goal_id == goal_id))
                })
                .await;
            }
            Ok(None)
        }
        None => Ok(None),
    }
}

/// WebSocket command face: execute one parsed goal command and return the
/// goal projection payload (snake_case) after the change.
pub async fn apply_goal_command(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<Option<Value>> {
    let outcome = native_session_goal_outcome(state, user, session_id, command).await?;
    Ok(outcome.goal)
}

async fn native_session_goal_outcome(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<GoalMutationOutcome> {
    let user_id = user.user_id.trim();
    let session_id = session_id.trim();
    let session = state
        .user_store
        .get_chat_session(user_id, session_id)
        .map_err(|error| anyhow::anyhow!("load session failed: {error}"))?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let echo = match &command {
        GoalCommand::Create { objective } => Some(format!("/goal {objective}")),
        GoalCommand::Edit { objective } => Some(format!("/goal edit {objective}")),
        GoalCommand::Pause => Some("/goal pause".to_string()),
        GoalCommand::Resume => Some("/goal resume".to_string()),
        GoalCommand::Clear => Some("/goal clear".to_string()),
        GoalCommand::Show | GoalCommand::InvalidEdit => None,
    };
    let (reply, goal_value) = execute_goal_command(
        goal_service(state).as_ref(),
        state.storage.clone(),
        user_id,
        session_id,
        command,
    )
    .await?;
    // Persist the /goal command as a durable completed user round so the
    // bubble survives reloads, mirroring the /compact command flow.
    if let Some(echo) = echo {
        let input = json!({
            "role": "user",
            "content": echo,
            "meta": {"type": "goal_command", "goal_command": true},
            "reply": reply,
        });
        let accepted = state
            .kernel
            .orchestrator
            .committer
            .accept_turn(user_id, session_id, &input)
            .await?;
        let user_round = accepted["user_turn_index"].as_i64().unwrap_or(1);
        state.monitor.register_continuation(
            session_id,
            user_id,
            session.agent_id.as_deref().unwrap_or(""),
            echo.as_str(),
            crate::user_store::UserStore::is_admin(user),
            user_round,
        );
        let storage = state.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        if let Some(turn) =
            crate::core::blocking::run_db("thread_log.goal_command.find_turn", move || {
                storage.find_thread_turn_id(&owner, &thread, user_round)
            })
            .await?
        {
            state
                .kernel
                .orchestrator
                .committer
                .update_turn(
                    user_id,
                    session_id,
                    &turn,
                    "completed",
                    "",
                    &json!({"stop_reason": "goal_command"}),
                )
                .await?;
        }
    }
    // Create/resume arm the goal: let the driver re-evaluate immediately.
    if goal_value.is_some() {
        goal_service(state)
            .driver()
            .notify_goal_changed(user_id, session_id);
    }
    Ok(GoalMutationOutcome { goal: goal_value })
}

fn goal_command_from_payload(payload: GoalUpsertPayload) -> Result<GoalCommand, Response> {
    if let Some(action) = payload
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match action {
            "pause" => Ok(GoalCommand::Pause),
            "resume" => Ok(GoalCommand::Resume),
            other => Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("unknown goal action: {other}"),
            )),
        };
    }
    let objective = payload
        .objective
        .as_deref()
        .map(goal::validate_objective)
        .transpose()
        .map_err(bad_request)?;
    let Some(objective) = objective else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    };
    Ok(GoalCommand::Edit { objective })
}

fn ensure_session_owner(
    state: &AppState,
    user_id: &str,
    session_id: &str,
) -> Result<crate::storage::ChatSessionRecord, Response> {
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .map_err(bad_request)?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))
}

fn normalize_session_id(session_id: String) -> Result<String, Response> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    Ok(session_id)
}

fn bad_request(err: impl ToString) -> Response {
    error_response(StatusCode::BAD_REQUEST, err.to_string())
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
