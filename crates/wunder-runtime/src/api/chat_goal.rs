use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::goal::{
    self, GoalCommand, GoalContinuationPlan, GoalStatus, GoalUpsertPayload, SOURCE_API,
};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
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

async fn get_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    let goal = goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "data": {
            "goal": goal.as_ref().map(goal::goal_payload)
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
    let session = ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    let approval_mode = payload.approval_mode.clone();
    let command = goal_command_from_payload(payload)?;
    let (goal, continuation, command_user_round) = apply_goal_command(
        &state,
        &resolved.user.user_id,
        &session_id,
        command,
        session.agent_id.as_deref(),
        crate::user_store::UserStore::is_admin(&resolved.user),
        approval_mode.as_deref(),
    )
    .await?;
    Ok(Json(json!({
        "data": {
            "goal": goal.as_ref().map(goal::goal_payload),
            "continuation": continuation,
            "user_round": command_user_round
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
    let deleted = goal::clear_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(
        json!({ "data": { "deleted": deleted, "goal": null } }),
    ))
}

pub(crate) async fn apply_goal_command(
    state: &Arc<AppState>,
    user_id: &str,
    session_id: &str,
    command: GoalCommand,
    agent_id: Option<&str>,
    is_admin: bool,
    approval_mode: Option<&str>,
) -> Result<
    (
        Option<crate::storage::SessionGoalRecord>,
        GoalContinuationPlan,
        Option<i64>,
    ),
    Response,
> {
    let session = goal::ensure_session(state.storage.clone(), user_id, session_id, agent_id)
        .await
        .map_err(bad_request)?;
    let mut should_schedule = false;
    let command_echo = match &command {
        GoalCommand::Set { objective, .. } => Some(format!("/goal {objective}")),
        GoalCommand::Resume => Some("/goal resume".to_string()),
        _ => None,
    };
    let mut record = match command {
        GoalCommand::Show => goal::get_goal(state.storage.clone(), user_id, session_id)
            .await
            .map_err(bad_request)?,
        GoalCommand::Set {
            objective,
            token_budget,
        } => {
            let record = goal::set_goal(
                state.storage.clone(),
                user_id,
                session_id,
                &objective,
                token_budget,
                SOURCE_API,
            )
            .await
            .map_err(bad_request)?;
            should_schedule = true;
            Some(record)
        }
        GoalCommand::Pause => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "use the chat stop button to exit goal mode".to_string(),
            ));
        }
        GoalCommand::Resume => {
            let record = goal::set_goal_status(
                state.storage.clone(),
                user_id,
                session_id,
                GoalStatus::Active,
                SOURCE_API,
            )
            .await
            .map_err(bad_request)?;
            should_schedule = true;
            Some(record)
        }
        GoalCommand::Clear => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "use the chat stop button to exit goal mode".to_string(),
            ));
        }
    };
    // Persist the /goal command as a durable user round in the transcript so
    // the chat bubble survives reloads, mirroring the /compact command flow.
    let command_user_round = if let Some(echo) = command_echo.as_deref() {
        let user_round = state.monitor.register(
            &session.session_id,
            user_id,
            session.agent_id.as_deref().unwrap_or(""),
            echo,
            is_admin,
        );
        state.kernel.orchestrator.append_goal_command_message(
            user_id,
            &session.session_id,
            echo,
            user_round,
        );
        Some(user_round)
    } else {
        None
    };
    if let Some(goal_record) = record.take() {
        record = Some(if command_user_round.is_some() {
            goal::bind_goal_execution_context(
                state.storage.clone(),
                goal_record,
                command_user_round,
                approval_mode,
            )
            .await
            .map_err(bad_request)?
        } else {
            goal_record
        });
    }
    let continuation = if should_schedule {
        schedule_goal_continuation(state, user_id, session_id).await
    } else {
        GoalContinuationPlan {
            should_start: false,
            reason: "not_requested".to_string(),
            session_id: session_id.to_string(),
            goal_id: record.as_ref().map(|item| item.goal_id.clone()),
        }
    };
    Ok((record, continuation, command_user_round))
}

pub(crate) async fn schedule_goal_continuation(
    state: &Arc<AppState>,
    user_id: &str,
    session_id: &str,
) -> GoalContinuationPlan {
    let session_id = session_id.trim().to_string();
    match state
        .kernel
        .thread_runtime
        .submit_goal_continuation(user_id, &session_id)
        .await
    {
        Ok(crate::services::runtime::thread::GoalContinuationSubmission::Started {
            goal_id,
            ..
        }) => GoalContinuationPlan {
            should_start: true,
            reason: "started".to_string(),
            session_id,
            goal_id: Some(goal_id),
        },
        Ok(crate::services::runtime::thread::GoalContinuationSubmission::Queued(info)) => {
            GoalContinuationPlan {
                should_start: true,
                reason: "queued".to_string(),
                session_id: info.session_id,
                goal_id: None,
            }
        }
        Ok(crate::services::runtime::thread::GoalContinuationSubmission::Skipped) => {
            GoalContinuationPlan {
                should_start: false,
                reason: "not_ready".to_string(),
                session_id,
                goal_id: None,
            }
        }
        Err(err) => GoalContinuationPlan {
            should_start: false,
            reason: err.to_string(),
            session_id,
            goal_id: None,
        },
    }
}

fn goal_command_from_payload(payload: GoalUpsertPayload) -> Result<GoalCommand, Response> {
    if let Some(status) = payload
        .status
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return match goal::normalize_status(status).map_err(bad_request)? {
            GoalStatus::Active => Ok(GoalCommand::Resume),
            GoalStatus::Paused => Err(error_response(
                StatusCode::BAD_REQUEST,
                "use the chat stop button to exit goal mode".to_string(),
            )),
            GoalStatus::Complete => Err(error_response(
                StatusCode::BAD_REQUEST,
                "model tool must mark goals complete".to_string(),
            )),
            GoalStatus::BudgetLimited => Err(error_response(
                StatusCode::BAD_REQUEST,
                "budget_limited is system controlled".to_string(),
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
    Ok(GoalCommand::Set {
        objective,
        token_budget: payload.token_budget.filter(|value| *value > 0),
    })
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
