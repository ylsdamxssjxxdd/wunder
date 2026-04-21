use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::prompting::read_prompt_template;
use crate::services::orchestration_context::{
    build_branch_history_record_from_state, build_chat_session_with_title, build_closed_history_record,
    build_history_record_from_state,
    build_initial_round_state, build_orchestration_run_id, build_orchestration_thread_title,
    clear_hive_state, clear_history_record,
    clear_member_bindings, clear_orchestration_workspace_tree, clear_round_state, clear_session_context,
    collect_descendant_history_ids_after_round,
    copy_chat_history_until_round, copy_round_directory_tree, copy_round_situation_files,
    delete_round_directories_after, latest_formal_round_index, list_history_records, load_hive_state,
    load_history_record, load_round_state, normalize_orchestration_run_name,
    orchestration_agent_artifact_dir_name,
    persist_hive_state, persist_history_record, persist_member_binding, persist_round_state,
    persist_session_context, rebuild_branch_round_state, repair_active_orchestration_main_threads,
    repair_orchestration_session_main_thread, round_dir_name, round_id,
    OrchestrationHiveState,
    OrchestrationHistoryRecord, OrchestrationMemberBinding, OrchestrationRoundRecord,
    OrchestrationRoundState, OrchestrationSessionContext, OrchestrationSuppressedMessageRange,
    ORCHESTRATION_HISTORY_STATUS_ACTIVE, ORCHESTRATION_HISTORY_STATUS_CLOSED, ORCHESTRATION_MODE,
};
use crate::services::swarm::beeroom::{
    claim_mother_agent, collect_agent_activity, get_mother_agent_id, mother_meta_key,
    resolve_preferred_mother_agent_id, set_mother_agent, snapshot_team_run,
};
use crate::state::AppState;
use crate::storage::{normalize_hive_id, HiveRecord, UserAgentRecord, DEFAULT_HIVE_ID};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;

fn close_existing_history_record(
    storage: &dyn crate::storage::StorageBackend,
    user_id: &str,
    group_id: &str,
    active_state: &OrchestrationHiveState,
    latest_round_index: i64,
    updated_at: f64,
) -> anyhow::Result<OrchestrationHistoryRecord> {
    let existing = load_history_record(storage, user_id, group_id, &active_state.orchestration_id);
    let history = build_closed_history_record(
        active_state,
        existing.as_ref(),
        latest_round_index,
        updated_at,
    );
    persist_history_record(storage, user_id, &history)?;
    Ok(history)
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/beeroom/groups",
            get(list_beeroom_groups).post(create_beeroom_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}",
            get(get_beeroom_group)
                .put(update_beeroom_group)
                .delete(delete_beeroom_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/move_agents",
            axum::routing::post(move_agents_to_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/missions",
            get(list_beeroom_missions),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/missions/{mission_id}",
            get(get_beeroom_mission),
        )
        .route(
            "/wunder/beeroom/orchestration/prompts",
            get(get_orchestration_prompts),
        )
        .route(
            "/wunder/beeroom/orchestration/session-context",
            axum::routing::post(update_orchestration_session_context),
        )
        .route(
            "/wunder/beeroom/orchestration/state",
            get(get_orchestration_state),
        )
        .route(
            "/wunder/beeroom/orchestration/state/create",
            axum::routing::post(create_orchestration_state),
        )
        .route(
            "/wunder/beeroom/orchestration/state/exit",
            axum::routing::post(exit_orchestration_state),
        )
        .route(
            "/wunder/beeroom/orchestration/history",
            get(list_orchestration_history).delete(delete_orchestration_history),
        )
        .route(
            "/wunder/beeroom/orchestration/history/restore",
            axum::routing::post(restore_orchestration_history),
        )
        .route(
            "/wunder/beeroom/orchestration/history/branch",
            axum::routing::post(branch_orchestration_history),
        )
        .route(
            "/wunder/beeroom/orchestration/history/truncate",
            axum::routing::post(truncate_orchestration_history),
        )
        .route(
            "/wunder/beeroom/orchestration/rounds/reserve",
            axum::routing::post(reserve_orchestration_round),
        )
        .route(
            "/wunder/beeroom/orchestration/rounds/finalize",
            axum::routing::post(finalize_orchestration_round),
        )
        .route(
            "/wunder/beeroom/orchestration/rounds/cancel",
            axum::routing::post(cancel_orchestration_round),
        )
}

async fn update_orchestration_session_context(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<UpdateOrchestrationSessionContextRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let session_id = payload.session_id.trim();
    let run_id = payload.run_id.trim();
    if session_id.is_empty() || run_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "session_id and run_id are required".to_string(),
        ));
    }
    let session = state
        .user_store
        .get_chat_session(&user_id, session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let context = OrchestrationSessionContext {
        mode: ORCHESTRATION_MODE.to_string(),
        run_id: run_id.to_string(),
        group_id: payload
            .group_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        role: payload
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("mother")
            .to_string(),
        round_index: payload.round_index.unwrap_or(1).max(1),
        mother_agent_id: payload
            .mother_agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(session.agent_id.as_deref())
            .unwrap_or_default()
            .to_string(),
    };
    persist_session_context(state.storage.as_ref(), &user_id, session_id, &context)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let _ = repair_orchestration_session_main_thread(
        state.storage.as_ref(),
        &user_id,
        session_id,
        context.round_index,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "session_id": session_id,
            "mode": context.mode,
            "run_id": context.run_id,
            "group_id": context.group_id,
            "role": context.role,
            "round_index": context.round_index,
            "mother_agent_id": context.mother_agent_id,
        }
    })))
}

async fn get_orchestration_state(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<OrchestrationStateQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(query.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let state_value = load_hive_state(state.storage.as_ref(), &user_id, &group_id);
    if let Some(active_state) = state_value.as_ref() {
        let round_index = latest_formal_round_index(
            load_or_migrate_round_state(state.as_ref(), &user_id, active_state).as_ref(),
        );
        let _ = repair_active_orchestration_main_threads(
            state.storage.as_ref(),
            &user_id,
            active_state,
            round_index,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let bindings = state_value
        .as_ref()
        .map(|item| {
            crate::services::orchestration_context::list_member_bindings(
                state.storage.as_ref(),
                &item.orchestration_id,
            )
            .unwrap_or_default()
        })
        .unwrap_or_default();
    let round_state = state_value
        .as_ref()
        .and_then(|item| load_or_migrate_round_state(state.as_ref(), &user_id, item));
    Ok(Json(json!({
        "data": {
            "active": state_value.is_some(),
            "state": state_value
                .as_ref()
                .map(|item| orchestration_state_payload(item, round_state.as_ref())),
            "member_threads": bindings.iter().map(orchestration_member_binding_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn create_orchestration_state(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateOrchestrationStateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if agents.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group has no agents".to_string(),
        ));
    }
    let mother_agent_id = payload
        .mother_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            get_mother_agent_id(state.storage.as_ref(), &user_id, &group.hive_id)
                .ok()
                .flatten()
        })
        .or_else(|| {
            resolve_preferred_mother_agent_id(state.storage.as_ref(), &user_id, &group.hive_id, None)
                .ok()
                .flatten()
        })
        .unwrap_or_default();
    if mother_agent_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "mother agent is required".to_string(),
        ));
    }
    let mother_agent = agents
        .iter()
        .find(|item| item.agent_id.trim() == mother_agent_id)
        .cloned()
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "mother agent not found".to_string()))?;
    let now = now_ts();
    let orchestration_id = format!("orch_state_{}", Uuid::new_v4().simple());
    let run_id = payload
        .run_name
        .as_deref()
        .or(payload.name.as_deref())
        .or(payload.run_id.as_deref())
        .map(normalize_orchestration_run_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| build_orchestration_run_id(None));

    if let Some(previous) = load_hive_state(state.storage.as_ref(), &user_id, &group.hive_id) {
        let previous_bindings = crate::services::orchestration_context::list_member_bindings(
            state.storage.as_ref(),
            &previous.orchestration_id,
        )
        .unwrap_or_default();
        let latest_round_index = latest_formal_round_index(
            load_or_migrate_round_state(state.as_ref(), &user_id, &previous).as_ref(),
        );
        let mut history = load_history_record(
            state.storage.as_ref(),
            &user_id,
            &group.hive_id,
            &previous.orchestration_id,
        )
        .unwrap_or_else(|| {
            build_history_record_from_state(
                &previous,
                ORCHESTRATION_HISTORY_STATUS_CLOSED,
                latest_round_index,
            )
        });
        history.status = ORCHESTRATION_HISTORY_STATUS_CLOSED.to_string();
        history.latest_round_index = latest_round_index;
        history.updated_at = now;
        history.exited_at = now;
        persist_history_record(state.storage.as_ref(), &user_id, &history)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        for binding in &previous_bindings {
            clear_session_context(state.storage.as_ref(), &user_id, &binding.session_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        clear_hive_state(state.storage.as_ref(), &user_id, &group.hive_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }

    let mut member_bindings = Vec::new();
    let mut mother_session_id = String::new();
    for agent in &agents {
        let title = build_orchestration_thread_title(&agent.name);
        let session = build_chat_session_with_title(&user_id, agent, &title);
        state
            .user_store
            .upsert_chat_session(&session)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let role = if agent.agent_id.trim() == mother_agent_id {
            "mother"
        } else {
            "worker"
        };
        state
            .kernel
            .thread_runtime
            .set_main_session(&user_id, &agent.agent_id, &session.session_id, "orchestration_create")
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let binding = OrchestrationMemberBinding {
            orchestration_id: orchestration_id.clone(),
            run_id: run_id.clone(),
            group_id: group.hive_id.clone(),
            agent_id: agent.agent_id.clone(),
            agent_name: agent.name.clone(),
            role: role.to_string(),
            session_id: session.session_id.clone(),
            title,
            created_at: session.created_at,
        };
        persist_member_binding(state.storage.as_ref(), &binding)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        persist_session_context(
            state.storage.as_ref(),
            &user_id,
            &session.session_id,
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: run_id.clone(),
                group_id: group.hive_id.clone(),
                role: role.to_string(),
                round_index: 1,
                mother_agent_id: mother_agent_id.clone(),
            },
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if role == "mother" {
            mother_session_id = session.session_id.clone();
        }
        member_bindings.push(binding);
    }

    let hive_state = OrchestrationHiveState {
        orchestration_id: orchestration_id.clone(),
        run_id: run_id.clone(),
        group_id: group.hive_id.clone(),
        mother_agent_id: mother_agent_id.clone(),
        mother_agent_name: mother_agent.name.clone(),
        mother_session_id: mother_session_id.clone(),
        active: true,
        entered_at: now,
        updated_at: now,
    };
    persist_hive_state(state.storage.as_ref(), &user_id, &hive_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let round_state = build_initial_round_state(&hive_state);
    persist_round_state(state.storage.as_ref(), &user_id, &round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let history = build_history_record_from_state(&hive_state, ORCHESTRATION_HISTORY_STATUS_ACTIVE, 1);
    persist_history_record(state.storage.as_ref(), &user_id, &history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
            "member_threads": member_bindings.iter().map(orchestration_member_binding_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn exit_orchestration_state(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ExitOrchestrationStateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let active_state = load_hive_state(state.storage.as_ref(), &user_id, &group.hive_id);
    let active_bindings = active_state
        .as_ref()
        .map(|item| {
            crate::services::orchestration_context::list_member_bindings(
                state.storage.as_ref(),
                &item.orchestration_id,
            )
            .unwrap_or_default()
        })
        .unwrap_or_default();
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut fresh_threads = Vec::new();
    for agent in &agents {
        let fresh_session_id = state
            .kernel
            .thread_runtime
            .create_fresh_main_session_id(&user_id, &agent.agent_id, "orchestration_exit")
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        clear_session_context(state.storage.as_ref(), &user_id, &fresh_session_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        fresh_threads.push(json!({
            "agent_id": agent.agent_id,
            "agent_name": agent.name,
            "session_id": fresh_session_id,
        }));
    }
    for binding in &active_bindings {
        clear_session_context(state.storage.as_ref(), &user_id, &binding.session_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    if let Some(active_state) = active_state {
        let latest_round_index = latest_formal_round_index(
            load_or_migrate_round_state(state.as_ref(), &user_id, &active_state).as_ref(),
        );
        let mut history = load_history_record(
            state.storage.as_ref(),
            &user_id,
            &group.hive_id,
            &active_state.orchestration_id,
        )
        .unwrap_or_else(|| {
            build_history_record_from_state(
                &active_state,
                ORCHESTRATION_HISTORY_STATUS_CLOSED,
                latest_round_index,
            )
        });
        history.status = ORCHESTRATION_HISTORY_STATUS_CLOSED.to_string();
        history.latest_round_index = latest_round_index;
        history.updated_at = now_ts();
        history.exited_at = history.updated_at;
        persist_history_record(state.storage.as_ref(), &user_id, &history)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    clear_hive_state(state.storage.as_ref(), &user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "group_id": group.hive_id,
            "active": false,
            "member_threads": fresh_threads,
        }
    })))
}

async fn list_orchestration_history(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<OrchestrationHistoryQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(query.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let items = list_history_records(state.storage.as_ref(), &user_id, &group_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items.iter().map(orchestration_history_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn delete_orchestration_history(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<DeleteOrchestrationHistoryRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    let orchestration_id = payload
        .orchestration_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if group_id.is_empty() || orchestration_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id and orchestration_id are required".to_string(),
        ));
    }
    load_history_record(state.storage.as_ref(), &user_id, &group_id, &orchestration_id)
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "orchestration history not found".to_string(),
            )
        })?;
    if let Some(active_state) = load_hive_state(state.storage.as_ref(), &user_id, &group_id) {
        if active_state.orchestration_id.trim() == orchestration_id {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "active orchestration history cannot be deleted".to_string(),
            ));
        }
    }
    clear_history_record(state.storage.as_ref(), &user_id, &group_id, &orchestration_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "ok": true,
            "orchestration_id": orchestration_id,
        }
    })))
}

async fn restore_orchestration_history(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<RestoreOrchestrationHistoryRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    let activate = payload.activate.unwrap_or(true);
    let orchestration_id = payload
        .orchestration_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if group_id.is_empty() || orchestration_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id and orchestration_id are required".to_string(),
        ));
    }
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let history = load_history_record(
        state.storage.as_ref(),
        &user_id,
        &group.hive_id,
        &orchestration_id,
    )
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration history not found".to_string()))?;
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if agents.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group has no agents".to_string(),
        ));
    }
    let mother_agent = agents
        .iter()
        .find(|item| item.agent_id.trim() == history.mother_agent_id.trim())
        .cloned()
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "mother agent not found".to_string()))?;
    if activate {
        if let Some(previous) = load_hive_state(state.storage.as_ref(), &user_id, &group.hive_id) {
            let previous_bindings = crate::services::orchestration_context::list_member_bindings(
                state.storage.as_ref(),
                &previous.orchestration_id,
            )
            .unwrap_or_default();
            let latest_round_index = latest_formal_round_index(
                load_or_migrate_round_state(state.as_ref(), &user_id, &previous).as_ref(),
            );
            let mut previous_history = load_history_record(
                state.storage.as_ref(),
                &user_id,
                &group.hive_id,
                &previous.orchestration_id,
            )
            .unwrap_or_else(|| {
                build_history_record_from_state(
                    &previous,
                    ORCHESTRATION_HISTORY_STATUS_CLOSED,
                    latest_round_index,
                )
            });
            previous_history.status = ORCHESTRATION_HISTORY_STATUS_CLOSED.to_string();
            previous_history.latest_round_index = latest_round_index;
            previous_history.updated_at = now_ts();
            previous_history.exited_at = previous_history.updated_at;
            persist_history_record(state.storage.as_ref(), &user_id, &previous_history)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            for binding in &previous_bindings {
                clear_session_context(state.storage.as_ref(), &user_id, &binding.session_id)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            clear_hive_state(state.storage.as_ref(), &user_id, &group.hive_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }

    let history_bindings =
        crate::services::orchestration_context::list_member_bindings(state.storage.as_ref(), &orchestration_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let round_state = load_or_migrate_round_state_by_orchestration_id(
        state.as_ref(),
        &user_id,
        &group.hive_id,
        &history.orchestration_id,
        &history.run_id,
        &history.mother_session_id,
        history.latest_round_index,
    )
    .unwrap_or_else(|| OrchestrationRoundState {
        orchestration_id: history.orchestration_id.clone(),
        run_id: history.run_id.clone(),
        group_id: group.hive_id.clone(),
        rounds: vec![OrchestrationRoundRecord {
            id: round_id(1),
            index: 1,
            situation: String::new(),
            user_message: String::new(),
            created_at: now_ts(),
            finalized_at: 0.0,
        }],
        suppressed_message_ranges: Vec::new(),
        updated_at: now_ts(),
    });
    let latest_round_index = latest_formal_round_index(Some(&round_state));
    let now = now_ts();
    let hive_state = OrchestrationHiveState {
        orchestration_id: history.orchestration_id.clone(),
        run_id: history.run_id.clone(),
        group_id: group.hive_id.clone(),
        mother_agent_id: history.mother_agent_id.clone(),
        mother_agent_name: mother_agent.name.clone(),
        mother_session_id: history.mother_session_id.clone(),
        active: activate,
        entered_at: history.entered_at.max(now),
        updated_at: now,
    };

    let mut member_bindings = Vec::new();
    for agent in &agents {
        let role = if agent.agent_id.trim() == hive_state.mother_agent_id.trim() {
            "mother"
        } else {
            "worker"
        };
        let existing = history_bindings
            .iter()
            .find(|item| item.agent_id.trim() == agent.agent_id.trim())
            .cloned();
        let reused_session_id = if role == "mother" {
            let fallback_mother_session = history.mother_session_id.trim().to_string();
            if !fallback_mother_session.is_empty() {
                fallback_mother_session
            } else {
                existing
                    .as_ref()
                    .map(|item| item.session_id.trim().to_string())
                    .unwrap_or_default()
            }
        } else {
            existing
                .as_ref()
                .map(|item| item.session_id.trim().to_string())
                .unwrap_or_default()
        };
        let session = if !reused_session_id.is_empty() {
            state
                .user_store
                .get_chat_session(&user_id, &reused_session_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        } else {
            None
        };
        let binding = if let Some(session) = session {
            let mut binding = existing.unwrap_or_default();
            binding.orchestration_id = hive_state.orchestration_id.clone();
            binding.run_id = hive_state.run_id.clone();
            binding.group_id = hive_state.group_id.clone();
            binding.agent_id = agent.agent_id.clone();
            binding.agent_name = agent.name.clone();
            binding.role = role.to_string();
            binding.session_id = session.session_id.clone();
            binding.title = if binding.title.trim().is_empty() {
                build_orchestration_thread_title(&agent.name)
            } else {
                binding.title
            };
            binding
        } else {
            let title = build_orchestration_thread_title(&agent.name);
            let session = build_chat_session_with_title(&user_id, agent, &title);
            state
                .user_store
                .upsert_chat_session(&session)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            OrchestrationMemberBinding {
                orchestration_id: hive_state.orchestration_id.clone(),
                run_id: hive_state.run_id.clone(),
                group_id: hive_state.group_id.clone(),
                agent_id: agent.agent_id.clone(),
                agent_name: agent.name.clone(),
                role: role.to_string(),
                session_id: session.session_id,
                title,
                created_at: session.created_at,
            }
        };
        persist_member_binding(state.storage.as_ref(), &binding)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if activate {
            persist_session_context(
                state.storage.as_ref(),
                &user_id,
                &binding.session_id,
                &OrchestrationSessionContext {
                    mode: ORCHESTRATION_MODE.to_string(),
                    run_id: hive_state.run_id.clone(),
                    group_id: hive_state.group_id.clone(),
                    role: role.to_string(),
                    round_index: latest_round_index,
                    mother_agent_id: hive_state.mother_agent_id.clone(),
                },
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            state
                .kernel
                .thread_runtime
                .set_main_session(&user_id, &agent.agent_id, &binding.session_id, "orchestration_restore")
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        member_bindings.push(binding);
    }
    if activate {
        persist_hive_state(state.storage.as_ref(), &user_id, &hive_state)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        persist_round_state(state.storage.as_ref(), &user_id, &round_state)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut next_history = history.clone();
    if activate {
        next_history.status = ORCHESTRATION_HISTORY_STATUS_ACTIVE.to_string();
        next_history.updated_at = now;
        next_history.restored_at = now;
    }
    next_history.latest_round_index = latest_round_index;
    next_history.mother_agent_name = mother_agent.name.clone();
    if activate {
        persist_history_record(state.storage.as_ref(), &user_id, &next_history)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(Json(json!({
        "data": {
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
            "history": orchestration_history_payload(&next_history),
            "member_threads": member_bindings.iter().map(orchestration_member_binding_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn branch_orchestration_history(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<BranchOrchestrationHistoryRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    let source_orchestration_id = payload
        .source_orchestration_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let requested_round_index = payload.round_index.unwrap_or(1).max(1);
    let activate = payload.activate.unwrap_or(true);
    if group_id.is_empty() || source_orchestration_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id and source_orchestration_id are required".to_string(),
        ));
    }
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let source_history = load_history_record(
        state.storage.as_ref(),
        &user_id,
        &group.hive_id,
        &source_orchestration_id,
    )
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration history not found".to_string()))?;
    let source_round_state = load_or_migrate_round_state_by_orchestration_id(
        state.as_ref(),
        &user_id,
        &group.hive_id,
        &source_history.orchestration_id,
        &source_history.run_id,
        &source_history.mother_session_id,
        source_history.latest_round_index,
    )
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration round state not found".to_string()))?;
    let branch_round_index = requested_round_index.min(latest_formal_round_index(Some(&source_round_state)));
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if agents.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group has no agents".to_string(),
        ));
    }
    let mother_agent = agents
        .iter()
        .find(|item| item.agent_id.trim() == source_history.mother_agent_id.trim())
        .cloned()
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "mother agent not found".to_string()))?;
    if activate {
        if let Some(previous) = load_hive_state(state.storage.as_ref(), &user_id, &group.hive_id) {
            let previous_bindings = crate::services::orchestration_context::list_member_bindings(
                state.storage.as_ref(),
                &previous.orchestration_id,
            )
            .unwrap_or_default();
            let latest_round_index = latest_formal_round_index(
                load_or_migrate_round_state(state.as_ref(), &user_id, &previous).as_ref(),
            );
            close_existing_history_record(
                state.storage.as_ref(),
                &user_id,
                &group.hive_id,
                &previous,
                latest_round_index,
                now_ts(),
            )
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            for binding in &previous_bindings {
                clear_session_context(state.storage.as_ref(), &user_id, &binding.session_id)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            clear_hive_state(state.storage.as_ref(), &user_id, &group.hive_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }

    let now = now_ts();
    let orchestration_id = format!("orch_state_{}", Uuid::new_v4().simple());
    let parent_run_id = normalize_orchestration_run_name(&source_history.run_id);
    let run_id = if parent_run_id.is_empty() {
        build_orchestration_run_id(None)
    } else {
        let branch_suffix = Uuid::new_v4().simple().to_string();
        let compact_parent: String = parent_run_id.chars().take(36).collect();
        format!("{}_b{}", compact_parent, &branch_suffix[..4])
    };
    let mut member_bindings = Vec::new();
    let mut mother_session_id = String::new();
    for agent in &agents {
        let title = build_orchestration_thread_title(&agent.name);
        let session = build_chat_session_with_title(&user_id, agent, &title);
        state
            .user_store
            .upsert_chat_session(&session)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let role = if agent.agent_id.trim() == source_history.mother_agent_id.trim() {
            "mother"
        } else {
            "worker"
        };
        if role == "mother" {
            copy_chat_history_until_round(
                state.storage.as_ref(),
                &user_id,
                &source_history.mother_session_id,
                &session.session_id,
                branch_round_index,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            mother_session_id = session.session_id.clone();
        }
        if activate {
            state
                .kernel
                .thread_runtime
                .set_main_session(&user_id, &agent.agent_id, &session.session_id, "orchestration_branch")
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        let binding = OrchestrationMemberBinding {
            orchestration_id: orchestration_id.clone(),
            run_id: run_id.clone(),
            group_id: group.hive_id.clone(),
            agent_id: agent.agent_id.clone(),
            agent_name: agent.name.clone(),
            role: role.to_string(),
            session_id: session.session_id.clone(),
            title,
            created_at: session.created_at,
        };
        persist_member_binding(state.storage.as_ref(), &binding)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        persist_session_context(
            state.storage.as_ref(),
            &user_id,
            &session.session_id,
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: run_id.clone(),
                group_id: group.hive_id.clone(),
                role: role.to_string(),
                round_index: branch_round_index,
                mother_agent_id: source_history.mother_agent_id.clone(),
            },
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        member_bindings.push(binding);
    }
    if mother_session_id.trim().is_empty() {
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "branch mother session create failed".to_string(),
        ));
    }
    let hive_state = OrchestrationHiveState {
        orchestration_id: orchestration_id.clone(),
        run_id: run_id.clone(),
        group_id: group.hive_id.clone(),
        mother_agent_id: source_history.mother_agent_id.clone(),
        mother_agent_name: mother_agent.name.clone(),
        mother_session_id: mother_session_id.clone(),
        active: activate,
        entered_at: now,
        updated_at: now,
    };
    let mut round_state = rebuild_branch_round_state(
        &source_round_state,
        &orchestration_id,
        &run_id,
        branch_round_index,
        now,
    );
    round_state.group_id = group.hive_id.clone();
    let mother_workspace_id = state
        .workspace
        .scoped_user_id_by_container(&user_id, mother_agent.sandbox_container_id);
    copy_round_directory_tree(
        state.workspace.as_ref(),
        &mother_workspace_id,
        &source_history.run_id,
        &run_id,
        branch_round_index,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    copy_round_situation_files(
        state.workspace.as_ref(),
        &mother_workspace_id,
        &source_history.run_id,
        &run_id,
        branch_round_index,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mother_reopen_dir = state
        .workspace
        .resolve_path(
            &mother_workspace_id,
            &[
                "orchestration",
                run_id.as_str(),
                &round_dir_name(branch_round_index.saturating_add(1).max(1)),
            ]
            .join("/"),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    fs::create_dir_all(&mother_reopen_dir)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for agent in &agents {
        if agent.agent_id.trim() == mother_agent.agent_id.trim() {
            continue;
        }
        let worker_workspace_id = state
            .workspace
            .scoped_user_id_by_container(&user_id, agent.sandbox_container_id);
        copy_round_directory_tree(
            state.workspace.as_ref(),
            &worker_workspace_id,
            &source_history.run_id,
            &run_id,
            branch_round_index,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let reopen_dir = state
            .workspace
            .resolve_path(
                &worker_workspace_id,
                &[
                    "orchestration".to_string(),
                    run_id.clone(),
                    round_dir_name(branch_round_index.saturating_add(1).max(1)),
                    orchestration_agent_artifact_dir_name(&agent.name, &agent.agent_id),
                ]
                .join("/"),
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        fs::create_dir_all(&reopen_dir)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    if activate {
        persist_hive_state(state.storage.as_ref(), &user_id, &hive_state)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    persist_round_state(state.storage.as_ref(), &user_id, &round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let latest_round_index = latest_formal_round_index(Some(&round_state));
    let history = build_branch_history_record_from_state(
        &hive_state,
        if activate {
            ORCHESTRATION_HISTORY_STATUS_ACTIVE
        } else {
            ORCHESTRATION_HISTORY_STATUS_CLOSED
        },
        latest_round_index,
        &source_history,
        branch_round_index,
    );
    persist_history_record(state.storage.as_ref(), &user_id, &history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
            "history": orchestration_history_payload(&history),
            "member_threads": member_bindings.iter().map(orchestration_member_binding_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn truncate_orchestration_history(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<TruncateOrchestrationHistoryRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    let orchestration_id = payload
        .orchestration_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let retained_round_index = payload.round_index.unwrap_or(1).max(1);
    if group_id.is_empty() || orchestration_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id and orchestration_id are required".to_string(),
        ));
    }
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let root_history = load_history_record(state.storage.as_ref(), &user_id, &group.hive_id, &orchestration_id)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration history not found".to_string()))?;
    let all_histories = list_history_records(state.storage.as_ref(), &user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let descendants_to_remove =
        collect_descendant_history_ids_after_round(&root_history, &all_histories, retained_round_index);
    let active_hive_state = load_hive_state(state.storage.as_ref(), &user_id, &group.hive_id);
    let is_active_root = active_hive_state
        .as_ref()
        .is_some_and(|active| active.orchestration_id.trim() == root_history.orchestration_id.trim());
    if let Some(active_state) = active_hive_state.as_ref() {
        if descendants_to_remove
            .iter()
            .any(|item| item.trim() == active_state.orchestration_id.trim())
        {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "active orchestration descendant cannot be removed".to_string(),
            ));
        }
    }

    for descendant_id in &descendants_to_remove {
        if let Some(history) = load_history_record(state.storage.as_ref(), &user_id, &group.hive_id, descendant_id) {
            let bindings =
                crate::services::orchestration_context::list_member_bindings(state.storage.as_ref(), descendant_id)
                    .unwrap_or_default();
            for binding in &bindings {
                clear_session_context(state.storage.as_ref(), &user_id, &binding.session_id)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            clear_member_bindings(state.storage.as_ref(), descendant_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            clear_round_state(state.storage.as_ref(), &user_id, descendant_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            clear_history_record(state.storage.as_ref(), &user_id, &group.hive_id, descendant_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let mut workspace_container_ids = std::collections::BTreeSet::new();
            for agent in state
                .user_store
                .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            {
                workspace_container_ids.insert(agent.sandbox_container_id);
            }
            if let Some(agent) = state
                .user_store
                .get_user_agent(&user_id, &history.mother_agent_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            {
                workspace_container_ids.insert(agent.sandbox_container_id);
            }
            for container_id in workspace_container_ids {
                let workspace_id = state
                    .workspace
                    .scoped_user_id_by_container(&user_id, container_id);
                clear_orchestration_workspace_tree(state.workspace.as_ref(), &workspace_id, &history.run_id)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
        }
    }

    let mut root_round_state = load_or_migrate_round_state_by_orchestration_id(
        state.as_ref(),
        &user_id,
        &group.hive_id,
        &root_history.orchestration_id,
        &root_history.run_id,
        &root_history.mother_session_id,
        root_history.latest_round_index,
    )
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration round state not found".to_string()))?;
    root_round_state.rounds.retain(|round| round.index <= retained_round_index);
    if root_round_state.rounds.is_empty() {
        root_round_state.rounds.push(OrchestrationRoundRecord {
            id: round_id(1),
            index: 1,
            situation: String::new(),
            user_message: String::new(),
            created_at: now_ts(),
            finalized_at: 0.0,
        });
    }
    root_round_state.updated_at = now_ts();
    persist_round_state(state.storage.as_ref(), &user_id, &root_round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for agent in &agents {
        let workspace_id = state
            .workspace
            .scoped_user_id_by_container(&user_id, agent.sandbox_container_id);
        delete_round_directories_after(
            state.workspace.as_ref(),
            &workspace_id,
            &root_history.run_id,
            retained_round_index,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }

    let latest_round_index = latest_formal_round_index(Some(&root_round_state));
    let mut next_history = root_history.clone();
    next_history.latest_round_index = latest_round_index;
    next_history.updated_at = root_round_state.updated_at;
    persist_history_record(state.storage.as_ref(), &user_id, &next_history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let mut next_state_payload = None;
    if is_active_root {
        if let Some(active_state) = active_hive_state {
            let bindings = crate::services::orchestration_context::list_member_bindings(
                state.storage.as_ref(),
                &active_state.orchestration_id,
            )
            .unwrap_or_default();
            for binding in &bindings {
                persist_session_context(
                    state.storage.as_ref(),
                    &user_id,
                    &binding.session_id,
                    &OrchestrationSessionContext {
                        mode: ORCHESTRATION_MODE.to_string(),
                        run_id: active_state.run_id.clone(),
                        group_id: active_state.group_id.clone(),
                        role: binding.role.clone(),
                        round_index: latest_round_index,
                        mother_agent_id: active_state.mother_agent_id.clone(),
                    },
                )
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            next_state_payload = Some(orchestration_state_payload(&active_state, Some(&root_round_state)));
        }
    }

    Ok(Json(json!({
        "data": {
            "history": orchestration_history_payload(&next_history),
            "state": next_state_payload,
            "round_state": orchestration_round_state_payload(&root_round_state),
            "removed_orchestration_ids": descendants_to_remove,
            "retained_round_index": latest_round_index,
        }
    })))
}

async fn reserve_orchestration_round(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ReserveOrchestrationRoundRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let hive_state = load_hive_state(state.storage.as_ref(), &user_id, &group_id)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration state not found".to_string()))?;
    let mut round_state = load_or_migrate_round_state(state.as_ref(), &user_id, &hive_state)
        .unwrap_or_else(|| build_initial_round_state(&hive_state));
    let current_round_index = latest_formal_round_index(Some(&round_state));
    let _ = repair_active_orchestration_main_threads(
        state.storage.as_ref(),
        &user_id,
        &hive_state,
        current_round_index,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let requested_round_id = payload
        .round_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let requested_index = payload.round_index.unwrap_or(0).max(0);
    let situation = payload
        .situation
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let user_message = payload
        .user_message
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let now = now_ts();
    let requested_target_index = if requested_index > 0 {
        requested_index
    } else if !requested_round_id.is_empty() {
        round_state
            .rounds
            .iter()
            .find(|round| round.id.trim() == requested_round_id)
            .map(|round| round.index)
            .unwrap_or_else(|| latest_formal_round_index(Some(&round_state)).saturating_add(1))
    } else {
        latest_formal_round_index(Some(&round_state)).saturating_add(1)
    };
    let mut normalized_target_index = requested_target_index.max(1);
    let latest_formal_index = latest_formal_round_index_or_zero(&round_state);
    let history_user_round_index =
        count_mother_user_rounds(state.storage.as_ref(), &user_id, &hive_state.mother_session_id, &round_state);
    let authoritative_next_index = latest_formal_index
        .max(history_user_round_index)
        .saturating_add(1)
        .max(1);
    if normalized_target_index < authoritative_next_index {
        normalized_target_index = authoritative_next_index;
    }
    if let Some(existing) = round_state
        .rounds
        .iter()
        .find(|round| round.index == normalized_target_index)
    {
        let requested_existing_round =
            !requested_round_id.is_empty() && existing.id.trim() == requested_round_id;
        if !requested_existing_round && !existing.user_message.trim().is_empty() {
            normalized_target_index = authoritative_next_index;
        }
    }
    debug!(
        target: "wunder::orchestration",
        event = "reserve_round_decision",
        user_id = %user_id,
        group_id = %group_id,
        orchestration_id = %hive_state.orchestration_id,
        run_id = %hive_state.run_id,
        requested_round_id = %requested_round_id,
        requested_index = requested_index,
        requested_target_index = requested_target_index,
        normalized_target_index = normalized_target_index,
        latest_formal_index = latest_formal_index,
        history_user_round_index = history_user_round_index,
        authoritative_next_index = authoritative_next_index,
        rounds = ?round_state.rounds.iter().map(|round| {
            json!({
                "id": round.id,
                "index": round.index,
                "user_message": !round.user_message.trim().is_empty(),
                "created_at": round.created_at,
                "finalized_at": round.finalized_at
            })
        }).collect::<Vec<_>>(),
        "orchestration reserve round decision"
    );
    if let Some(existing) = round_state
        .rounds
        .iter_mut()
        .find(|round| round.index == normalized_target_index)
    {
        if !situation.is_empty() {
            existing.situation = situation;
        }
        if !user_message.is_empty() {
            existing.user_message = user_message;
        }
        if existing.created_at <= 0.0 {
            existing.created_at = now;
        }
    } else {
        round_state.rounds.push(OrchestrationRoundRecord {
            id: round_id(normalized_target_index),
            index: normalized_target_index,
            situation,
            user_message,
            created_at: now,
            finalized_at: 0.0,
        });
    }
    round_state.updated_at = now;
    persist_round_state(state.storage.as_ref(), &user_id, &round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let latest_round_index = latest_formal_round_index(Some(&round_state));
    let mut history = load_history_record(
        state.storage.as_ref(),
        &user_id,
        &group_id,
        &hive_state.orchestration_id,
    )
    .unwrap_or_else(|| {
        build_history_record_from_state(
            &hive_state,
            ORCHESTRATION_HISTORY_STATUS_ACTIVE,
            latest_round_index,
        )
    });
    history.latest_round_index = latest_round_index;
    history.updated_at = now;
    persist_history_record(state.storage.as_ref(), &user_id, &history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let round = round_state
        .rounds
        .iter()
        .find(|round| round.index == normalized_target_index)
        .cloned()
        .ok_or_else(|| error_response(StatusCode::INTERNAL_SERVER_ERROR, "round reserve failed".to_string()))?;
    debug!(
        target: "wunder::orchestration",
        event = "reserve_round_result",
        user_id = %user_id,
        group_id = %group_id,
        orchestration_id = %hive_state.orchestration_id,
        run_id = %hive_state.run_id,
        reserved_round_id = %round.id,
        reserved_round_index = round.index,
        latest_round_index = latest_round_index,
        "orchestration reserve round result"
    );
    Ok(Json(json!({
        "data": {
            "round": orchestration_round_payload(&round),
            "round_state": orchestration_round_state_payload(&round_state),
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
        }
    })))
}

async fn finalize_orchestration_round(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<FinalizeOrchestrationRoundRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let hive_state = load_hive_state(state.storage.as_ref(), &user_id, &group_id)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration state not found".to_string()))?;
    let mut round_state = load_or_migrate_round_state(state.as_ref(), &user_id, &hive_state)
        .unwrap_or_else(|| build_initial_round_state(&hive_state));
    let target_round_id = payload
        .round_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let target_round_index = payload.round_index.unwrap_or(0).max(0);
    let target_position = round_state
        .rounds
        .iter()
        .position(|round| {
            (!target_round_id.is_empty() && round.id.trim() == target_round_id)
                || (target_round_index > 0 && round.index == target_round_index)
        })
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "round not found".to_string()))?;
    if let Some(situation) = payload.situation.as_deref().map(str::trim) {
        if !situation.is_empty() {
            round_state.rounds[target_position].situation = situation.to_string();
        }
    }
    if let Some(user_message) = payload.user_message.as_deref().map(str::trim) {
        if !user_message.is_empty() {
            round_state.rounds[target_position].user_message = user_message.to_string();
        }
    }
    round_state.rounds[target_position].finalized_at = now_ts();
    round_state.updated_at = round_state.rounds[target_position].finalized_at;
    persist_round_state(state.storage.as_ref(), &user_id, &round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let latest_round_index = latest_formal_round_index(Some(&round_state));
    let mut history = load_history_record(
        state.storage.as_ref(),
        &user_id,
        &group_id,
        &hive_state.orchestration_id,
    )
    .unwrap_or_else(|| {
        build_history_record_from_state(
            &hive_state,
            ORCHESTRATION_HISTORY_STATUS_ACTIVE,
            latest_round_index,
        )
    });
    history.latest_round_index = latest_round_index;
    history.updated_at = round_state.updated_at;
    persist_history_record(state.storage.as_ref(), &user_id, &history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let response_round = round_state.rounds[target_position].clone();
    debug!(
        target: "wunder::orchestration",
        event = "finalize_round_result",
        user_id = %user_id,
        group_id = %group_id,
        orchestration_id = %hive_state.orchestration_id,
        run_id = %hive_state.run_id,
        round_id = %response_round.id,
        round_index = response_round.index,
        latest_round_index = latest_round_index,
        finalized_at = response_round.finalized_at,
        "orchestration finalize round result"
    );
    Ok(Json(json!({
        "data": {
            "round": orchestration_round_payload(&response_round),
            "round_state": orchestration_round_state_payload(&round_state),
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
        }
    })))
}

async fn cancel_orchestration_round(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CancelOrchestrationRoundRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group_id = normalize_hive_id(payload.group_id.as_deref().unwrap_or_default());
    if group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let hive_state = load_hive_state(state.storage.as_ref(), &user_id, &group_id)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "orchestration state not found".to_string()))?;
    let mut round_state = load_or_migrate_round_state(state.as_ref(), &user_id, &hive_state)
        .unwrap_or_else(|| build_initial_round_state(&hive_state));
    let target_round_id = payload
        .round_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let target_round_index = payload.round_index.unwrap_or(0).max(0);
    let start_at = payload.message_started_at.unwrap_or(0.0).max(0.0);
    let end_at = payload.message_ended_at.unwrap_or_else(now_ts).max(start_at);
    let remove_round = payload.remove_round.unwrap_or(false);
    let mut cancelled_round: Option<OrchestrationRoundRecord> = None;
    if remove_round {
        let mut retained = Vec::with_capacity(round_state.rounds.len());
        for round in round_state.rounds.drain(..) {
            let matches = (!target_round_id.is_empty() && round.id.trim() == target_round_id)
                || (target_round_index > 0 && round.index == target_round_index);
            if matches && cancelled_round.is_none() {
                cancelled_round = Some(round);
                continue;
            }
            retained.push(round);
        }
        if retained.is_empty() {
            retained.push(OrchestrationRoundRecord {
                id: round_id(1),
                index: 1,
                situation: String::new(),
                user_message: String::new(),
                created_at: now_ts(),
                finalized_at: 0.0,
            });
        }
        round_state.rounds = retained;
    } else if let Some(round) = round_state.rounds.iter_mut().find(|round| {
        (!target_round_id.is_empty() && round.id.trim() == target_round_id)
            || (target_round_index > 0 && round.index == target_round_index)
    }) {
        round.user_message.clear();
        round.finalized_at = 0.0;
        cancelled_round = Some(round.clone());
    }
    if cancelled_round.is_none() {
        return Err(error_response(StatusCode::NOT_FOUND, "round not found".to_string()));
    }
    if start_at > 0.0 {
        round_state
            .suppressed_message_ranges
            .push(OrchestrationSuppressedMessageRange { start_at, end_at });
        if round_state.suppressed_message_ranges.len() > 24 {
            let trim_from = round_state.suppressed_message_ranges.len() - 24;
            round_state.suppressed_message_ranges.drain(0..trim_from);
        }
    }
    round_state.updated_at = now_ts();
    persist_round_state(state.storage.as_ref(), &user_id, &round_state)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let latest_round_index = latest_formal_round_index(Some(&round_state));
    let mut history = load_history_record(
        state.storage.as_ref(),
        &user_id,
        &group_id,
        &hive_state.orchestration_id,
    )
    .unwrap_or_else(|| {
        build_history_record_from_state(
            &hive_state,
            ORCHESTRATION_HISTORY_STATUS_ACTIVE,
            latest_round_index,
        )
    });
    history.latest_round_index = latest_round_index;
    history.updated_at = round_state.updated_at;
    persist_history_record(state.storage.as_ref(), &user_id, &history)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(round) = cancelled_round.as_ref() {
        debug!(
            target: "wunder::orchestration",
            event = "cancel_round_result",
            user_id = %user_id,
            group_id = %group_id,
            orchestration_id = %hive_state.orchestration_id,
            run_id = %hive_state.run_id,
            round_id = %round.id,
            round_index = round.index,
            remove_round = remove_round,
            latest_round_index = latest_round_index,
            suppressed_ranges = round_state.suppressed_message_ranges.len(),
            "orchestration cancel round result"
        );
    }
    Ok(Json(json!({
        "data": {
            "round": cancelled_round.as_ref().map(orchestration_round_payload),
            "round_state": orchestration_round_state_payload(&round_state),
            "state": orchestration_state_payload(&hive_state, Some(&round_state)),
        }
    })))
}

async fn get_orchestration_prompts(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let _resolved = resolve_user(&state, &headers, None).await?;
    let locale = if i18n::get_language().to_ascii_lowercase().starts_with("en") {
        "en"
    } else {
        "zh"
    };
    let templates = [
        ("mother_runtime", "prompts/orchestration/mother_runtime.txt"),
        ("round_artifacts", "prompts/orchestration/round_artifacts.txt"),
        (
            "worker_first_dispatch",
            "prompts/orchestration/worker_first_dispatch.txt",
        ),
        (
            "worker_round_artifacts",
            "prompts/orchestration/worker_round_artifacts.txt",
        ),
        ("worker_guide", "prompts/orchestration/worker_guide.txt"),
        (
            "situation_context",
            "prompts/orchestration/situation_context.txt",
        ),
        ("user_message", "prompts/orchestration/user_message.txt"),
    ];
    let prompts = templates
        .into_iter()
        .map(|(key, path)| {
            let localized_path = Path::new("prompts").join(locale).join(
                Path::new(path)
                    .strip_prefix("prompts")
                    .unwrap_or_else(|_| Path::new(path)),
            );
            let localized_template = read_prompt_template(localized_path.as_path());
            let template = if localized_template.trim().is_empty() {
                read_prompt_template(Path::new(path))
            } else {
                localized_template
            };
            (
                key.to_string(),
                json!(template),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(Json(json!({
        "data": {
            "prompts": prompts,
        }
    })))
}

async fn list_beeroom_groups(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListBeeroomGroupsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let include_archived = query.include_archived.unwrap_or(false);
    let groups = state
        .user_store
        .list_hives(&user_id, include_archived)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let limit = query.mission_limit.unwrap_or(10).clamp(1, 50);
    let mut items = Vec::with_capacity(groups.len());
    for group in groups {
        items.push(group_payload(state.as_ref(), &group, limit)?);
    }
    Ok(Json(
        json!({ "data": { "items": items, "total": items.len() } }),
    ))
}

async fn create_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateBeeroomGroupRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "beeroom name is required".to_string(),
        ));
    }
    let mut hive_id = payload
        .group_id
        .as_deref()
        .map(normalize_hive_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| normalize_hive_id(name));
    if hive_id == DEFAULT_HIVE_ID {
        hive_id = format!("beeroom-{}", Uuid::new_v4().simple());
    }
    if state
        .user_store
        .get_hive(&user_id, &hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some()
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("beeroom group {hive_id} already exists"),
        ));
    }

    let now = now_ts();
    let record = HiveRecord {
        hive_id,
        user_id: user_id.clone(),
        name: name.to_string(),
        description: payload.description.unwrap_or_default(),
        is_default: false,
        status: "active".to_string(),
        created_time: now,
        updated_time: now,
    };
    state
        .user_store
        .upsert_hive(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    if let Some(mother_agent_id) = payload
        .mother_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let agent = state
            .user_store
            .get_user_agent(&user_id, mother_agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if agent.is_some() {
            state
                .user_store
                .move_agents_to_hive(&user_id, &record.hive_id, &[mother_agent_id.to_string()])
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            claim_mother_agent(
                state.storage.as_ref(),
                &user_id,
                &record.hive_id,
                mother_agent_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }

    Ok(Json(
        json!({ "data": group_payload(state.as_ref(), &record, 10)? }),
    ))
}

async fn get_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Query(query): Query<ListBeeroomGroupsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let mission_limit = query.mission_limit.unwrap_or(20).clamp(1, 100);
    let missions = load_group_missions(state.as_ref(), &user_id, &group.hive_id, mission_limit)?;
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let activity = collect_agent_activity(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &user_id,
        &group.hive_id,
        &agents,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "group": group_payload(state.as_ref(), &group, mission_limit)?,
            "agents": agents
                .iter()
                .map(|agent| agent_payload(agent, activity.get(&agent.agent_id)))
                .collect::<Vec<_>>(),
            "missions": missions,
        }
    })))
}

async fn update_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<UpdateBeeroomGroupRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let mut group = load_group(state.as_ref(), &user_id, &group_id)?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "beeroom name is required".to_string(),
        ));
    }

    group.name = name.to_string();
    group.description = payload.description.unwrap_or_default();
    group.updated_time = now_ts();
    state
        .user_store
        .upsert_hive(&group)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    match payload.mother_agent_id.as_deref().map(str::trim) {
        Some("") => {
            state
                .storage
                .set_meta(&mother_meta_key(&user_id, &group.hive_id), "")
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        Some(mother_agent_id) => {
            state
                .user_store
                .get_user_agent(&user_id, mother_agent_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
                .ok_or_else(|| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        "mother agent not found".to_string(),
                    )
                })?;
            state
                .user_store
                .move_agents_to_hive(&user_id, &group.hive_id, &[mother_agent_id.to_string()])
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            set_mother_agent(
                state.storage.as_ref(),
                &user_id,
                &group.hive_id,
                mother_agent_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        None => {}
    }

    Ok(Json(
        json!({ "data": group_payload(state.as_ref(), &group, 10)? }),
    ))
}

async fn delete_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    if group.is_default || normalize_hive_id(&group.hive_id) == DEFAULT_HIVE_ID {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "default beeroom group cannot be deleted".to_string(),
        ));
    }

    let member_ids = state
        .user_store
        .list_user_agents_by_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .map(|agent| agent.agent_id)
        .collect::<Vec<_>>();
    let reset_agent_total = if member_ids.is_empty() {
        0
    } else {
        state
            .user_store
            .move_agents_to_hive(&user_id, DEFAULT_HIVE_ID, &member_ids)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    };
    let deleted_mission_total = state
        .user_store
        .delete_team_runs_by_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted_chat_message_total = state
        .user_store
        .delete_beeroom_chat_messages(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted = state
        .user_store
        .delete_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if deleted <= 0 {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "beeroom group not found".to_string(),
        ));
    }
    if deleted_chat_message_total > 0 {
        state
            .projection
            .beeroom
            .publish_chat_cleared(
                &user_id,
                &group.hive_id,
                deleted_chat_message_total,
                now_ts(),
            )
            .await;
    }
    Ok(Json(json!({
        "data": {
            "deleted": deleted,
            "group_id": group.hive_id,
            "reset_agent_total": reset_agent_total,
            "deleted_mission_total": deleted_mission_total,
            "deleted_chat_message_total": deleted_chat_message_total,
            "fallback_group_id": DEFAULT_HIVE_ID,
        }
    })))
}

async fn move_agents_to_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<MoveAgentsRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let agent_ids = payload
        .agent_ids
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if agent_ids.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "agent_ids is empty".to_string(),
        ));
    }
    let moved = state
        .user_store
        .move_agents_to_hive(&user_id, &group.hive_id, &agent_ids)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "data": { "moved": moved, "group_id": group.hive_id } }),
    ))
}

async fn list_beeroom_missions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Query(query): Query<ListMissionsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let (runs, total) = state
        .user_store
        .list_team_runs(&user_id, Some(&group.hive_id), None, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        let snapshot =
            snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        items.push(mission_payload(&snapshot));
    }
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn get_beeroom_mission(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((group_id, mission_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let run = state
        .user_store
        .get_team_run(mission_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "mission not found".to_string()))?;
    if run.user_id != user_id
        || normalize_hive_id(&run.hive_id) != normalize_hive_id(&group.hive_id)
    {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "mission not found".to_string(),
        ));
    }
    let snapshot = snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": mission_payload(&snapshot) })))
}

pub(crate) fn load_group(
    state: &AppState,
    user_id: &str,
    group_id: &str,
) -> Result<HiveRecord, Response> {
    let normalized = normalize_hive_id(group_id);
    if normalized == DEFAULT_HIVE_ID {
        return state
            .user_store
            .ensure_default_hive(user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()));
    }
    state
        .user_store
        .get_hive(user_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "beeroom group not found".to_string()))
}

fn load_group_missions(
    state: &AppState,
    user_id: &str,
    hive_id: &str,
    limit: i64,
) -> Result<Vec<Value>, Response> {
    let (runs, _) = state
        .user_store
        .list_team_runs(user_id, Some(hive_id), None, 0, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        let snapshot =
            snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        items.push(mission_payload(&snapshot));
    }
    Ok(items)
}

fn group_payload(
    state: &AppState,
    group: &HiveRecord,
    mission_limit: i64,
) -> Result<Value, Response> {
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&group.user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let activity = collect_agent_activity(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &group.user_id,
        &group.hive_id,
        &agents,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let missions = load_group_missions(state, &group.user_id, &group.hive_id, mission_limit)?;
    let mother_agent_id =
        get_mother_agent_id(state.storage.as_ref(), &group.user_id, &group.hive_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .or_else(|| {
                resolve_preferred_mother_agent_id(
                    state.storage.as_ref(),
                    &group.user_id,
                    &group.hive_id,
                    None,
                )
                .ok()
                .flatten()
            });
    let mother_agent = mother_agent_id
        .as_deref()
        .and_then(|agent_id| agents.iter().find(|agent| agent.agent_id == agent_id));
    let active_agent_total = agents
        .iter()
        .filter(|agent| {
            activity
                .get(&agent.agent_id)
                .is_some_and(|item| !item.is_idle())
        })
        .count();
    let running_mission_total = missions
        .iter()
        .filter(|item| {
            !matches!(
                item.get("completion_status")
                    .and_then(Value::as_str)
                    .unwrap_or("running"),
                "completed" | "failed" | "cancelled"
            )
        })
        .count();
    let orchestration_state = load_hive_state(state.storage.as_ref(), &group.user_id, &group.hive_id);

    Ok(json!({
        "group_id": group.hive_id,
        "hive_id": group.hive_id,
        "name": group.name,
        "description": group.description,
        "status": group.status,
        "is_default": group.is_default,
        "created_time": group.created_time,
        "updated_time": group.updated_time,
        "agent_total": agents.len(),
        "active_agent_total": active_agent_total,
        "idle_agent_total": agents.len().saturating_sub(active_agent_total),
        "running_mission_total": running_mission_total,
        "mission_total": missions.len(),
        "mother_agent_id": mother_agent_id,
        "mother_agent_name": mother_agent.map(|agent| agent.name.clone()),
        "members": agents
            .iter()
            .take(6)
            .map(|agent| agent_payload(agent, activity.get(&agent.agent_id)))
            .collect::<Vec<_>>(),
        "latest_mission": missions.first().cloned(),
        "active_orchestration": orchestration_state
            .as_ref()
            .map(|item| orchestration_state_payload(item, None)),
    }))
}

fn mission_payload(snapshot: &crate::services::swarm::beeroom::TeamRunSnapshot) -> Value {
    json!({
        "team_run_id": snapshot.run.team_run_id,
        "mission_id": snapshot.run.team_run_id,
        "hive_id": snapshot.run.hive_id,
        "parent_session_id": snapshot.run.parent_session_id,
        "entry_agent_id": snapshot.run.parent_agent_id,
        "mother_agent_id": snapshot.run.mother_agent_id,
        "strategy": snapshot.run.strategy,
        "status": snapshot.run.status,
        "completion_status": snapshot.completion_status,
        "task_total": snapshot.run.task_total,
        "task_success": snapshot.run.task_success,
        "task_failed": snapshot.run.task_failed,
        "context_tokens_total": snapshot.run.context_tokens_total,
        "context_tokens_peak": snapshot.run.context_tokens_peak,
        "model_round_total": snapshot.run.model_round_total,
        "started_time": snapshot.run.started_time,
        "finished_time": snapshot.run.finished_time,
        "elapsed_s": snapshot.run.elapsed_s,
        "summary": snapshot.run.summary,
        "error": snapshot.run.error,
        "updated_time": snapshot.run.updated_time,
        "all_tasks_terminal": snapshot.all_tasks_terminal,
        "all_agents_idle": snapshot.all_agents_idle,
        "active_agent_ids": snapshot.active_agent_ids,
        "idle_agent_ids": snapshot.idle_agent_ids,
        "tasks": snapshot
            .tasks
            .iter()
            .map(|task| {
                json!({
                    "task_id": task.task_id,
                    "agent_id": task.agent_id,
                    "target_session_id": task.target_session_id,
                    "spawned_session_id": task.spawned_session_id,
                    "session_run_id": task.session_run_id,
                    "status": task.status,
                    "priority": task.priority,
                    "started_time": task.started_time,
                    "finished_time": task.finished_time,
                    "elapsed_s": task.elapsed_s,
                    "result_summary": task.result_summary,
                    "error": task.error,
                    "updated_time": task.updated_time,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn agent_payload(
    agent: &UserAgentRecord,
    activity: Option<&crate::services::swarm::beeroom::AgentActivitySnapshot>,
) -> Value {
    let active_session_ids = activity
        .map(crate::services::swarm::beeroom::AgentActivitySnapshot::active_session_ids)
        .unwrap_or_default();
    json!({
        "agent_id": agent.agent_id,
        "name": agent.name,
        "description": agent.description,
        "status": agent.status,
        "hive_id": agent.hive_id,
        "icon": agent.icon,
        "is_shared": agent.is_shared,
        "approval_mode": agent.approval_mode,
        "tool_names": agent.tool_names,
        "sandbox_container_id": agent.sandbox_container_id,
        "silent": agent.silent,
        "prefer_mother": agent.prefer_mother,
        "active_session_total": active_session_ids.len(),
        "active_session_ids": active_session_ids,
        "idle": activity.is_none_or(|item| item.is_idle()),
    })
}

fn orchestration_round_payload(round: &OrchestrationRoundRecord) -> Value {
    json!({
        "id": round.id,
        "index": round.index,
        "situation": round.situation,
        "user_message": round.user_message,
        "created_at": round.created_at,
        "finalized_at": round.finalized_at,
    })
}

fn orchestration_round_state_payload(round_state: &OrchestrationRoundState) -> Value {
    json!({
        "orchestration_id": round_state.orchestration_id,
        "run_id": round_state.run_id,
        "group_id": round_state.group_id,
        "rounds": round_state.rounds.iter().map(orchestration_round_payload).collect::<Vec<_>>(),
        "suppressed_message_ranges": round_state.suppressed_message_ranges.iter().map(|range| {
            json!({
                "start_at": range.start_at,
                "end_at": range.end_at,
            })
        }).collect::<Vec<_>>(),
        "updated_at": round_state.updated_at,
    })
}

fn orchestration_state_payload(
    state: &OrchestrationHiveState,
    round_state: Option<&OrchestrationRoundState>,
) -> Value {
    json!({
        "orchestration_id": state.orchestration_id,
        "run_id": state.run_id,
        "group_id": state.group_id,
        "mother_agent_id": state.mother_agent_id,
        "mother_agent_name": state.mother_agent_name,
        "mother_session_id": state.mother_session_id,
        "active": state.active,
        "entered_at": state.entered_at,
        "updated_at": state.updated_at,
        "round_state": round_state.map(orchestration_round_state_payload),
    })
}

fn orchestration_member_binding_payload(binding: &OrchestrationMemberBinding) -> Value {
    json!({
        "orchestration_id": binding.orchestration_id,
        "run_id": binding.run_id,
        "group_id": binding.group_id,
        "agent_id": binding.agent_id,
        "agent_name": binding.agent_name,
        "role": binding.role,
        "session_id": binding.session_id,
        "title": binding.title,
        "created_at": binding.created_at,
    })
}

fn orchestration_history_payload(record: &OrchestrationHistoryRecord) -> Value {
    json!({
        "orchestration_id": record.orchestration_id,
        "run_id": record.run_id,
        "group_id": record.group_id,
        "mother_agent_id": record.mother_agent_id,
        "mother_agent_name": record.mother_agent_name,
        "mother_session_id": record.mother_session_id,
        "status": record.status,
        "latest_round_index": record.latest_round_index,
        "entered_at": record.entered_at,
        "updated_at": record.updated_at,
        "exited_at": record.exited_at,
        "restored_at": record.restored_at,
        "parent_orchestration_id": record.parent_orchestration_id,
        "branch_root_orchestration_id": record.branch_root_orchestration_id,
        "branch_from_round_index": record.branch_from_round_index,
        "branch_depth": record.branch_depth,
    })
}

fn latest_formal_round_index_or_zero(round_state: &OrchestrationRoundState) -> i64 {
    round_state
        .rounds
        .iter()
        .filter(|round| !round.user_message.trim().is_empty())
        .map(|round| round.index)
        .max()
        .unwrap_or(0)
        .max(0)
}

fn count_mother_user_rounds(
    storage: &dyn crate::storage::StorageBackend,
    user_id: &str,
    mother_session_id: &str,
    round_state: &OrchestrationRoundState,
) -> i64 {
    if mother_session_id.trim().is_empty() {
        return 0;
    }
    let earliest_round_created_at = round_state
        .rounds
        .iter()
        .map(|round| round.created_at)
        .filter(|value| *value > 0.0)
        .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(0.0);
    storage
        .load_chat_history(user_id.trim(), mother_session_id.trim(), None)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .filter(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if role != "user" {
                return false;
            }
            let created_at = message
                .get("created_at")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            earliest_round_created_at <= 0.0 || created_at <= 0.0 || created_at >= earliest_round_created_at
        })
        .count() as i64
}

fn load_or_migrate_round_state(
    state: &AppState,
    user_id: &str,
    hive_state: &OrchestrationHiveState,
) -> Option<OrchestrationRoundState> {
    load_or_migrate_round_state_by_orchestration_id(
        state,
        user_id,
        &hive_state.group_id,
        &hive_state.orchestration_id,
        &hive_state.run_id,
        &hive_state.mother_session_id,
        1,
    )
}

fn load_or_migrate_round_state_by_orchestration_id(
    state: &AppState,
    user_id: &str,
    group_id: &str,
    orchestration_id: &str,
    run_id: &str,
    mother_session_id: &str,
    latest_round_index_hint: i64,
) -> Option<OrchestrationRoundState> {
    if let Some(existing) = load_round_state(state.storage.as_ref(), user_id, orchestration_id) {
        return Some(existing);
    }
    let messages = state
        .storage
        .load_chat_history(user_id.trim(), mother_session_id.trim(), Some(400))
        .ok()
        .unwrap_or_default();
    let mut rounds = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if role != "user" {
            continue;
        }
        let index = rounds.len() as i64 + 1;
        rounds.push(OrchestrationRoundRecord {
            id: round_id(index),
            index,
            situation: String::new(),
            user_message: message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string(),
            created_at: message
                .get("created_at")
                .and_then(Value::as_f64)
                .unwrap_or_else(now_ts),
            finalized_at: message
                .get("created_at")
                .and_then(Value::as_f64)
                .unwrap_or_else(now_ts),
        });
    }
    let fallback_count = latest_round_index_hint.max(1).max(rounds.len() as i64);
    if rounds.is_empty() {
        rounds.push(OrchestrationRoundRecord {
            id: round_id(1),
            index: 1,
            situation: String::new(),
            user_message: String::new(),
            created_at: now_ts(),
            finalized_at: 0.0,
        });
    } else if rounds.len() < fallback_count as usize {
        for index in (rounds.len() as i64 + 1)..=fallback_count {
            rounds.push(OrchestrationRoundRecord {
                id: round_id(index),
                index,
                situation: String::new(),
                user_message: String::new(),
                created_at: now_ts(),
                finalized_at: 0.0,
            });
        }
    }
    let migrated = OrchestrationRoundState {
        orchestration_id: orchestration_id.trim().to_string(),
        run_id: run_id.trim().to_string(),
        group_id: group_id.trim().to_string(),
        rounds,
        suppressed_message_ranges: Vec::new(),
        updated_at: now_ts(),
    };
    let _ = persist_round_state(state.storage.as_ref(), user_id, &migrated);
    Some(migrated)
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Debug, Deserialize)]
struct ListBeeroomGroupsQuery {
    #[serde(default)]
    include_archived: Option<bool>,
    #[serde(default)]
    mission_limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateBeeroomGroupRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "groupId", alias = "hive_id", alias = "hiveId")]
    group_id: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    mother_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateBeeroomGroupRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    mother_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoveAgentsRequest {
    #[serde(default, alias = "agentIds", alias = "agent_ids")]
    agent_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateOrchestrationSessionContextRequest {
    #[serde(alias = "sessionId")]
    session_id: String,
    #[serde(alias = "runId")]
    run_id: String,
    #[serde(default, alias = "groupId", alias = "hive_id", alias = "hiveId")]
    group_id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default, alias = "roundIndex")]
    round_index: Option<i64>,
    #[serde(default, alias = "motherAgentId")]
    mother_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrchestrationStateQuery {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateOrchestrationStateRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    mother_agent_id: Option<String>,
    #[serde(default, alias = "runId", alias = "run_id")]
    run_id: Option<String>,
    #[serde(default, alias = "runName", alias = "run_name")]
    run_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExitOrchestrationStateRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrchestrationHistoryQuery {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RestoreOrchestrationHistoryRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "orchestrationId", alias = "orchestration_id")]
    orchestration_id: Option<String>,
    #[serde(default)]
    activate: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DeleteOrchestrationHistoryRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "orchestrationId", alias = "orchestration_id")]
    orchestration_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BranchOrchestrationHistoryRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "sourceOrchestrationId", alias = "source_orchestration_id")]
    source_orchestration_id: Option<String>,
    #[serde(default, alias = "roundIndex", alias = "round_index")]
    round_index: Option<i64>,
    #[serde(default)]
    activate: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TruncateOrchestrationHistoryRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "orchestrationId", alias = "orchestration_id")]
    orchestration_id: Option<String>,
    #[serde(default, alias = "roundIndex", alias = "round_index")]
    round_index: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReserveOrchestrationRoundRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "roundId", alias = "round_id")]
    round_id: Option<String>,
    #[serde(default, alias = "roundIndex", alias = "round_index")]
    round_index: Option<i64>,
    #[serde(default)]
    situation: Option<String>,
    #[serde(default, alias = "userMessage", alias = "user_message")]
    user_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FinalizeOrchestrationRoundRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "roundId", alias = "round_id")]
    round_id: Option<String>,
    #[serde(default, alias = "roundIndex", alias = "round_index")]
    round_index: Option<i64>,
    #[serde(default)]
    situation: Option<String>,
    #[serde(default, alias = "userMessage", alias = "user_message")]
    user_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CancelOrchestrationRoundRequest {
    #[serde(default, alias = "groupId", alias = "hiveId", alias = "hive_id")]
    group_id: Option<String>,
    #[serde(default, alias = "roundId", alias = "round_id")]
    round_id: Option<String>,
    #[serde(default, alias = "roundIndex", alias = "round_index")]
    round_index: Option<i64>,
    #[serde(default, alias = "messageStartedAt", alias = "message_started_at")]
    message_started_at: Option<f64>,
    #[serde(default, alias = "messageEndedAt", alias = "message_ended_at")]
    message_ended_at: Option<f64>,
    #[serde(default, alias = "removeRound", alias = "remove_round")]
    remove_round: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListMissionsQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
