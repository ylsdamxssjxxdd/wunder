use crate::api::user_context::resolve_user;
use crate::auth as guard_auth;
use crate::i18n;
use crate::schemas::{StreamEvent, TokenUsage, WunderRequest};
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::services::default_agent_protocol::DEFAULT_AGENT_ID_ALIAS;
use crate::services::external::provision_external_launch_session;
use crate::services::stream_events::StreamEventService;
use crate::services::user_agent_presets::ensure_user_preset_agents;
use crate::state::AppState;
use crate::storage::{
    SessionRunRecord, UpdateAgentTaskStatusParams, UserAccountRecord, UserAgentRecord,
    MAX_SANDBOX_CONTAINER_ID,
};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names, is_agent_allowed};
use crate::user_store::{build_default_agent_record_from_storage, UserStore};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;
use tracing::warn;
use uuid::Uuid;
use walkdir::WalkDir;

const EXTERNAL_WORKFLOW_CONTAINER_ID: i32 = MAX_SANDBOX_CONTAINER_ID;
const RUN_KIND: &str = "external_workflow";
const REQUESTED_BY: &str = "external_workflow_api";
const DEFAULT_TIMEOUT_S: f64 = 6000.0;
const MAX_TIMEOUT_S: f64 = 6000.0;
const MAX_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
const DEFAULT_EVENT_LIMIT: i64 = 200;
const MAX_EVENT_LIMIT: i64 = 1000;
const ACTIVE_RUN_META_PREFIX: &str = "external_workflow_active:";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/external/workflows:stream",
            post(create_workflow_stream).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/external/workflows",
            post(create_workflow).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route("/wunder/external/workflows/{run_id}", get(get_workflow))
        .route(
            "/wunder/external/workflows/{run_id}/events",
            get(list_workflow_events),
        )
        .route(
            "/wunder/external/workflows/{run_id}/cancel",
            post(cancel_workflow),
        )
        .route(
            "/wunder/external/workflows/{run_id}/files/{file_id}",
            get(download_workflow_file),
        )
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalWorkflowRequest {
    #[serde(default, alias = "userId", alias = "user_id")]
    user_id: Option<String>,
    #[serde(default, alias = "userName", alias = "user_name", alias = "username")]
    user_name: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, alias = "agentName", alias = "agent_name")]
    agent_name: Option<String>,
    #[serde(default, alias = "content")]
    message: Option<String>,
    #[serde(
        default,
        alias = "workspaceContainerId",
        alias = "workspace_container_id",
        alias = "containerId",
        alias = "container_id"
    )]
    workspace_container_id: Option<i32>,
    #[serde(default, alias = "clearWorkspace", alias = "clear_workspace")]
    clear_workspace: Option<bool>,
    #[serde(default, alias = "timeoutS", alias = "timeout_s")]
    timeout_s: Option<f64>,
    #[serde(default, alias = "clientRunId", alias = "client_run_id")]
    client_run_id: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WorkflowEventsQuery {
    #[serde(default, alias = "afterEventId", alias = "after_event_id")]
    after_event_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Clone)]
struct PendingUpload {
    temp_path: PathBuf,
    filename: String,
}

#[derive(Debug)]
struct ParsedWorkflowMultipart {
    request: ExternalWorkflowRequest,
    files: Vec<PendingUpload>,
    temp_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowFileRef {
    file_id: String,
    path: String,
    name: String,
    size: u64,
    download_url: String,
}

struct PreparedWorkflow {
    run_id: String,
    session_id: String,
    user: UserAccountRecord,
    agent: UserAgentRecord,
    request: ExternalWorkflowRequest,
    workspace_id: String,
    input_files: Vec<String>,
    started_at: f64,
}

async fn create_workflow_stream(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, Response> {
    validate_content_length(&headers)?;
    let parsed = parse_multipart(multipart).await?;
    let prepared = prepare_workflow(state.clone(), &headers, parsed).await?;
    let stream = run_workflow_stream(state, prepared);
    let sse =
        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
    Ok(sse.into_response())
}

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, Response> {
    validate_content_length(&headers)?;
    let parsed = parse_multipart(multipart).await?;
    let prepared = prepare_workflow(state.clone(), &headers, parsed).await?;
    let payload = workflow_started_payload(&prepared, "running");
    tokio::spawn(async move {
        let mut stream = run_workflow_stream(state, prepared);
        while stream.next().await.is_some() {}
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "data": payload }))).into_response())
}

async fn get_workflow(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let record = load_authorized_run(&state, &headers, &run_id).await?;
    Ok(Json(json!({ "data": run_record_payload(&record) })))
}

async fn list_workflow_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<WorkflowEventsQuery>,
) -> Result<Json<Value>, Response> {
    let record = load_authorized_run(&state, &headers, &run_id).await?;
    let after_event_id = query.after_event_id.unwrap_or(0).max(0);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(1, MAX_EVENT_LIMIT);
    let service = StreamEventService::new(state.storage.clone());
    let rows = service
        .list_events(&record.session_id, after_event_id, limit)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let events = rows
        .into_iter()
        .map(|row| map_persisted_event(&record, row))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "run_id": record.run_id, "events": events } }),
    ))
}

async fn cancel_workflow(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let mut record = load_authorized_run(&state, &headers, &run_id).await?;
    let cancel_requested = cancel_session_and_tasks(state.as_ref(), &record.user_id, &record);
    let now = now_ts();
    record.status = if is_terminal_status(&record.status) {
        record.status.clone()
    } else {
        "cancelling".to_string()
    };
    record.updated_time = now;
    state
        .user_store
        .upsert_session_run(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if record.status == "cancelling" {
        release_active_workflow(state.as_ref(), &record.user_id, &record.run_id);
    }
    Ok(Json(json!({
        "data": {
            "run_id": record.run_id,
            "session_id": record.session_id,
            "status": record.status,
            "cancel_requested": cancel_requested,
        }
    })))
}

async fn download_workflow_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((run_id, file_id)): AxumPath<(String, String)>,
) -> Result<Response, Response> {
    let record = load_authorized_run(&state, &headers, &run_id).await?;
    let files = files_from_record(&record);
    let Some(file_ref) = files.iter().find(|item| item.file_id == file_id) else {
        return Err(error_with_code(
            StatusCode::NOT_FOUND,
            "FILE_NOT_FOUND",
            "workflow file not found".to_string(),
        ));
    };
    let workspace_id = workflow_workspace_id(state.as_ref(), &record.user_id);
    let target = state
        .workspace
        .resolve_path(&workspace_id, &file_ref.path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() || !target.is_file() {
        return Err(error_with_code(
            StatusCode::NOT_FOUND,
            "FILE_NOT_FOUND",
            "workflow file not found".to_string(),
        ));
    }
    let file = fs::File::open(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(stream_response(
        ReaderStream::new(file),
        &file_ref.name,
        "application/octet-stream",
    ))
}

async fn parse_multipart(mut multipart: Multipart) -> Result<ParsedWorkflowMultipart, Response> {
    let mut request: Option<ExternalWorkflowRequest> = None;
    let mut files = Vec::new();
    let mut temp_dir = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "request" {
            let raw = field
                .text()
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let parsed = serde_json::from_str::<ExternalWorkflowRequest>(&raw).map_err(|err| {
                error_with_code(
                    StatusCode::BAD_REQUEST,
                    "INVALID_REQUEST",
                    format!("invalid request json: {err}"),
                )
            })?;
            request = Some(parsed);
            continue;
        }
        if name == "files" || name == "files[]" || field.file_name().is_some() {
            let filename = field.file_name().unwrap_or("upload").to_string();
            if temp_dir.is_none() {
                temp_dir = Some(
                    create_temp_upload_dir()
                        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?,
                );
            }
            let Some(dir) = temp_dir.as_ref() else {
                return Err(error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    i18n::t("error.internal"),
                ));
            };
            let temp_path = dir.join(format!("upload_{}", Uuid::new_v4().simple()));
            save_multipart_file(field, &temp_path).await?;
            files.push(PendingUpload {
                temp_path,
                filename,
            });
        }
    }

    let Some(request) = request else {
        cleanup_temp_uploads(&files, temp_dir.as_ref());
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "multipart field `request` is required".to_string(),
        ));
    };
    Ok(ParsedWorkflowMultipart {
        request,
        files,
        temp_dir,
    })
}

async fn prepare_workflow(
    state: Arc<AppState>,
    headers: &HeaderMap,
    parsed: ParsedWorkflowMultipart,
) -> Result<PreparedWorkflow, Response> {
    let ParsedWorkflowMultipart {
        request,
        files,
        temp_dir,
    } = parsed;
    let result = prepare_workflow_inner(state, headers, &request, &files).await;
    cleanup_temp_uploads(&files, temp_dir.as_ref());
    result
}

async fn prepare_workflow_inner(
    state: Arc<AppState>,
    headers: &HeaderMap,
    request: &ExternalWorkflowRequest,
    files: &[PendingUpload],
) -> Result<PreparedWorkflow, Response> {
    validate_workflow_request(request, files)?;
    let user = resolve_target_user(state.as_ref(), headers, request).await?;
    let agent = resolve_target_agent(state.as_ref(), &user, request).await?;
    // Always cancel any active external workflow for this user before starting a new one.
    // This simplifies the API: new requests automatically preempt old ones.
    cancel_active_external_workflow(state.as_ref(), &user.user_id)?;
    let _preempted = preempt_agent_current_work(state.as_ref(), &user.user_id, &agent.agent_id)?;

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let session_id = state
        .kernel
        .thread_runtime
        .create_fresh_main_session_id(&user.user_id, &agent.agent_id, "external_workflow")
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let workspace_id = workflow_workspace_id(state.as_ref(), &user.user_id);
    let removed = state
        .workspace
        .clear_container_workspace(&user.user_id, EXTERNAL_WORKFLOW_CONTAINER_ID)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let input_files = store_input_files(state.as_ref(), &workspace_id, files).await?;
    ensure_output_dir(state.as_ref(), &workspace_id).await?;
    let started_at = now_ts();
    let dispatch_id = request
        .client_run_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| run_id.clone());
    let record = SessionRunRecord {
        run_id: run_id.clone(),
        session_id: session_id.clone(),
        parent_session_id: None,
        user_id: user.user_id.clone(),
        dispatch_id: Some(dispatch_id),
        run_kind: Some(RUN_KIND.to_string()),
        requested_by: Some(REQUESTED_BY.to_string()),
        agent_id: Some(agent.agent_id.clone()),
        model_name: agent.model_name.clone(),
        status: "running".to_string(),
        queued_time: started_at,
        started_time: started_at,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: started_at,
        metadata: Some(json!({
            "workspace_container_id": EXTERNAL_WORKFLOW_CONTAINER_ID,
            "workspace_id": workspace_id,
            "input_files": input_files,
            "workspace_removed_entries": removed,
            "request": {
                "user_name": request.user_name,
                "agent_name": request.agent_name,
                "client_run_id": request.client_run_id,
                "timeout_s": timeout_s(request),
                "metadata": request.metadata,
            }
        })),
    };
    state
        .user_store
        .upsert_session_run(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    set_active_workflow(state.as_ref(), &user.user_id, &run_id)?;
    Ok(PreparedWorkflow {
        run_id,
        session_id,
        user,
        agent,
        request: request.clone(),
        workspace_id,
        input_files,
        started_at,
    })
}

fn run_workflow_stream(
    state: Arc<AppState>,
    prepared: PreparedWorkflow,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    tokio::spawn(async move {
        let timeout = std::time::Duration::from_secs_f64(timeout_s(&prepared.request));
        let _ = send_workflow_event(
            &tx,
            "workflow.start",
            None,
            workflow_started_payload(&prepared, "running"),
        )
        .await;
        let result = tokio::time::timeout(
            timeout,
            run_workflow_to_completion(state.clone(), &prepared, tx.clone()),
        )
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!("external workflow stream failed: {err}");
                finish_workflow_with_error(
                    state.as_ref(),
                    &prepared,
                    &tx,
                    "error",
                    err.to_string(),
                )
                .await;
            }
            Err(_) => {
                state
                    .monitor
                    .cancel_with_source(&prepared.session_id, "external_workflow_timeout");
                finish_workflow_with_error(
                    state.as_ref(),
                    &prepared,
                    &tx,
                    "timeout",
                    "external workflow timeout".to_string(),
                )
                .await;
            }
        }
    });
    ReceiverStream::new(rx).map(Ok::<_, std::convert::Infallible>)
}

async fn run_workflow_to_completion(
    state: Arc<AppState>,
    prepared: &PreparedWorkflow,
    tx: tokio::sync::mpsc::Sender<Event>,
) -> anyhow::Result<()> {
    let mut final_answer: Option<String> = None;
    let mut final_usage: Option<TokenUsage> = None;
    let mut stop_reason: Option<String> = None;
    let mut error_text: Option<String> = None;
    let mut status = "completed".to_string();
    let request = build_wunder_request(state.as_ref(), prepared).await?;
    let stream = match state.kernel.orchestrator.stream(request).await {
        Ok(stream) => stream,
        Err(err) => {
            status = "error".to_string();
            error_text = Some(err.to_string());
            update_run_terminal_if_current(
                state.as_ref(),
                prepared,
                &status,
                None,
                error_text.as_deref(),
                None,
                None,
                None,
            )
            .await;
            release_active_workflow(state.as_ref(), &prepared.user.user_id, &prepared.run_id);
            let _ = send_workflow_event(
                &tx,
                "workflow.error",
                None,
                json!({
                    "run_id": prepared.run_id,
                    "session_id": prepared.session_id,
                    "status": status,
                    "error": error_text,
                }),
            )
            .await;
            return Ok(());
        }
    };

    tokio::pin!(stream);
    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(event) => event,
            Err(err) => match err {},
        };
        let payload = workflow_event_payload(&prepared.run_id, &prepared.session_id, &event);
        let _ = send_workflow_event(&tx, "workflow.event", event.id.as_deref(), payload).await;
        match event.event.as_str() {
            "final" => {
                let data = inner_event_data(&event);
                final_answer = data
                    .get("answer")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                final_usage = data
                    .get("usage")
                    .and_then(|value| serde_json::from_value(value.clone()).ok());
                stop_reason = data
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
            }
            "error" => {
                status = if is_cancelled_error(&event) {
                    "cancelled".to_string()
                } else {
                    "error".to_string()
                };
                error_text = Some(extract_error_text(&event));
            }
            _ => {}
        }
    }

    let current_run_status = state
        .user_store
        .get_session_run(&prepared.run_id)
        .ok()
        .flatten()
        .map(|record| record.status);
    let cancel_requested = current_run_status
        .as_deref()
        .map(|value| matches!(value, "cancelling" | "cancelled"))
        .unwrap_or(false);
    let cancelled = state
        .monitor
        .get_record(&prepared.session_id)
        .and_then(|record| {
            record
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|value| matches!(value.as_str(), "cancelled" | "cancelling"))
        .unwrap_or(false);
    if (cancel_requested || cancelled) && status == "completed" {
        status = "cancelled".to_string();
        error_text = Some("cancelled".to_string());
        if cancel_requested {
            final_answer = None;
        }
    }
    let files = collect_workflow_files(state.as_ref(), prepared, final_answer.as_deref()).await;
    update_run_terminal_if_current(
        state.as_ref(),
        prepared,
        &status,
        final_answer.as_deref(),
        error_text.as_deref(),
        Some(files.clone()),
        stop_reason.as_deref(),
        final_usage.as_ref(),
    )
    .await;
    release_active_workflow(state.as_ref(), &prepared.user.user_id, &prepared.run_id);
    let event_name = if status == "completed" {
        "workflow.final"
    } else {
        "workflow.error"
    };
    let _ = send_workflow_event(
        &tx,
        event_name,
        None,
        json!({
            "run_id": prepared.run_id,
            "session_id": prepared.session_id,
            "status": status,
            "answer": final_answer,
            "usage": final_usage,
            "stop_reason": stop_reason,
            "files": files,
            "error": error_text,
        }),
    )
    .await;
    Ok(())
}

async fn build_wunder_request(
    state: &AppState,
    prepared: &PreparedWorkflow,
) -> anyhow::Result<WunderRequest> {
    let user_context = build_user_tool_context(state, &prepared.user.user_id).await;
    let allowed = compute_allowed_tool_names(&prepared.user, &user_context);
    let agent_defaults = resolve_agent_runtime_tool_names(
        &prepared.agent.tool_names,
        &prepared.agent.declared_tool_names,
        &prepared.agent.declared_skill_names,
    );
    let tool_names = resolve_allowed_agent_tools(allowed, &agent_defaults);
    let message = build_workflow_message(
        prepared.request.message.as_deref().unwrap_or_default(),
        &prepared.input_files,
    );
    let agent_prompt = prepared.agent.system_prompt.trim().to_string();
    let agent_prompt = if agent_prompt.is_empty() {
        None
    } else {
        Some(agent_prompt)
    };
    Ok(WunderRequest {
        user_id: prepared.user.user_id.clone(),
        question: message,
        tool_names,
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(prepared.session_id.clone()),
        agent_id: Some(prepared.agent.agent_id.clone()),
        workspace_container_id: Some(EXTERNAL_WORKFLOW_CONTAINER_ID),
        model_name: prepared.agent.model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: Some(json!({
            "security": {
                "approval_mode": normalize_approval_mode(&prepared.agent.approval_mode)
            }
        })),
        agent_prompt,
        preview_skill: prepared.agent.preview_skill,
        attachments: None,
        allow_queue: false,
        is_admin: UserStore::is_admin(&prepared.user),
        approval_tx: None,
    })
}

fn validate_workflow_request(
    request: &ExternalWorkflowRequest,
    files: &[PendingUpload],
) -> Result<(), Response> {
    let container_id = request
        .workspace_container_id
        .unwrap_or(EXTERNAL_WORKFLOW_CONTAINER_ID);
    if container_id != EXTERNAL_WORKFLOW_CONTAINER_ID {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "external workflow only supports workspace_container_id=10".to_string(),
        ));
    }
    if request.clear_workspace == Some(false) {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "external workflow requires clear_workspace=true".to_string(),
        ));
    }
    let has_message = request
        .message
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if !has_message && files.is_empty() {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "message or files are required".to_string(),
        ));
    }
    if request.agent_id.as_deref().unwrap_or("").trim().is_empty()
        && request
            .agent_name
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "agent_name or agent_id is required".to_string(),
        ));
    }
    Ok(())
}

async fn resolve_target_user(
    state: &AppState,
    headers: &HeaderMap,
    request: &ExternalWorkflowRequest,
) -> Result<UserAccountRecord, Response> {
    let user_key = request
        .user_id
        .as_deref()
        .or(request.user_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if is_external_workflow_key_or_admin(state, headers).await {
        if let Some(user_key) = user_key {
            let user_store = state.user_store.clone();
            let lookup = user_key.to_string();
            let user = tokio::task::spawn_blocking(move || {
                resolve_or_provision_external_workflow_user(user_store.as_ref(), &lookup)
            })
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .map_err(|err| {
                error_with_code(
                    StatusCode::BAD_REQUEST,
                    "USER_PROVISION_FAILED",
                    err.to_string(),
                )
            })?;
            if let Err(err) = ensure_user_preset_agents(state, &user).await {
                warn!(
                    "failed to sync preset agents for external workflow user {}: {err}",
                    user.user_id
                );
            }
            return Ok(user);
        }
    }
    let resolved = resolve_user(state, headers, user_key).await?;
    Ok(resolved.user)
}

async fn resolve_target_agent(
    state: &AppState,
    user: &UserAccountRecord,
    request: &ExternalWorkflowRequest,
) -> Result<UserAgentRecord, Response> {
    if let Some(agent_id) = request
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if is_default_agent_alias(agent_id) {
            return build_default_agent_record_from_storage(state.storage.as_ref(), &user.user_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()));
        }
        let Some(agent) = state
            .user_store
            .get_user_agent_by_id(agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        else {
            return Err(error_with_code(
                StatusCode::NOT_FOUND,
                "AGENT_NOT_FOUND",
                "agent not found".to_string(),
            ));
        };
        ensure_agent_allowed(state, user, agent)
    } else if let Some(agent_name) = request
        .agent_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        resolve_agent_by_name(state, user, agent_name)
    } else {
        Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "agent_name or agent_id is required".to_string(),
        ))
    }
}

fn resolve_or_provision_external_workflow_user(
    user_store: &UserStore,
    user_key: &str,
) -> anyhow::Result<UserAccountRecord> {
    let lookup = user_key.trim();
    if lookup.is_empty() {
        return Err(anyhow::anyhow!("user is required"));
    }
    if let Some(user) = user_store.get_user_by_id(lookup)? {
        ensure_external_workflow_user_active(&user)?;
        return Ok(user);
    }
    if let Some(user) = user_store.get_user_by_username(lookup)? {
        ensure_external_workflow_user_active(&user)?;
        return Ok(user);
    }
    let (session, _, _) = provision_external_launch_session(
        user_store,
        lookup,
        None,
        None,
        false,
        UserStore::default_session_scope(),
    )?;
    Ok(session.user)
}

fn ensure_external_workflow_user_active(user: &UserAccountRecord) -> anyhow::Result<()> {
    if UserStore::is_admin(user) {
        return Err(anyhow::anyhow!("admin account is protected"));
    }
    if user.status.trim().to_lowercase() != "active" {
        return Err(anyhow::anyhow!("user disabled"));
    }
    Ok(())
}

fn resolve_agent_by_name(
    state: &AppState,
    user: &UserAccountRecord,
    agent_name: &str,
) -> Result<UserAgentRecord, Response> {
    let access = state
        .user_store
        .get_user_agent_access(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut agents = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    agents.extend(
        state
            .user_store
            .list_shared_user_agents(&user.user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?,
    );
    agents.push(
        build_default_agent_record_from_storage(state.storage.as_ref(), &user.user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?,
    );
    let lookup = normalize_lookup_key(agent_name);
    let mut seen = HashSet::new();
    let mut matched = agents
        .into_iter()
        .filter(|agent| seen.insert(agent.agent_id.clone()))
        .filter(|agent| is_agent_allowed(user, access.as_ref(), agent))
        .filter(|agent| normalize_lookup_key(&agent.name) == lookup)
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    match matched.len() {
        0 => Err(error_with_code(
            StatusCode::NOT_FOUND,
            "AGENT_NOT_FOUND",
            "agent not found".to_string(),
        )),
        1 => Ok(matched.remove(0)),
        _ => Err(error_with_code(
            StatusCode::CONFLICT,
            "AGENT_NAME_AMBIGUOUS",
            "agent_name is ambiguous; use agent_id".to_string(),
        )),
    }
}

fn ensure_agent_allowed(
    state: &AppState,
    user: &UserAccountRecord,
    agent: UserAgentRecord,
) -> Result<UserAgentRecord, Response> {
    let access = state
        .user_store
        .get_user_agent_access(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if is_agent_allowed(user, access.as_ref(), &agent) {
        Ok(agent)
    } else {
        Err(error_with_code(
            StatusCode::NOT_FOUND,
            "AGENT_NOT_FOUND",
            "agent not found".to_string(),
        ))
    }
}

fn preempt_agent_current_work(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
) -> Result<bool, Response> {
    let Some(thread) = state
        .user_store
        .get_agent_thread(user_id, agent_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Ok(false);
    };
    let record = SessionRunRecord {
        run_id: String::new(),
        session_id: thread.session_id,
        parent_session_id: None,
        user_id: user_id.to_string(),
        dispatch_id: None,
        run_kind: None,
        requested_by: None,
        agent_id: Some(agent_id.to_string()),
        model_name: None,
        status: String::new(),
        queued_time: 0.0,
        started_time: 0.0,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: 0.0,
        metadata: None,
    };
    Ok(cancel_session_and_tasks(state, user_id, &record))
}

fn cancel_session_and_tasks(state: &AppState, user_id: &str, record: &SessionRunRecord) -> bool {
    let mut cancelled = state
        .monitor
        .cancel_with_source(&record.session_id, "external_workflow_cancel");
    let thread_id = format!("thread_{}", record.session_id.trim());
    let now = now_ts();
    if let Ok(tasks) = state
        .user_store
        .list_agent_tasks_by_thread(&thread_id, None, 128)
    {
        for task in tasks {
            if matches!(task.status.as_str(), "pending" | "running" | "retry") {
                if state
                    .user_store
                    .update_agent_task_status(UpdateAgentTaskStatusParams {
                        task_id: &task.task_id,
                        status: "cancelled",
                        retry_count: 0,
                        retry_at: now,
                        started_at: task.started_at,
                        finished_at: Some(now),
                        last_error: Some("cancelled"),
                        updated_at: now,
                    })
                    .is_ok()
                {
                    cancelled = true;
                }
            }
        }
    }
    let _ = state
        .storage
        .delete_session_goal(user_id, &record.session_id);
    cancelled
}

fn cancel_active_external_workflow(state: &AppState, user_id: &str) -> Result<(), Response> {
    if let Some(run_id) = load_active_workflow(state, user_id)? {
        if let Some(mut run) = state
            .user_store
            .get_session_run(&run_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .filter(|run| run.run_kind.as_deref() == Some(RUN_KIND))
        {
            let _ = cancel_session_and_tasks(state, user_id, &run);
            run.status = "cancelled".to_string();
            run.finished_time = now_ts();
            run.elapsed_s = (run.finished_time - run.started_time).max(0.0);
            run.error = Some("cancelled by a newer external workflow".to_string());
            run.updated_time = run.finished_time;
            let _ = state.user_store.upsert_session_run(&run);
        }
        release_active_workflow(state, user_id, &run_id);
    }
    Ok(())
}

async fn store_input_files(
    state: &AppState,
    workspace_id: &str,
    files: &[PendingUpload],
) -> Result<Vec<String>, Response> {
    let mut output = Vec::new();
    let mut used_names = HashSet::new();
    for file in files {
        let safe_name = unique_filename(&sanitize_filename(&file.filename), &mut used_names);
        let relative = format!("input/{safe_name}");
        let dest = state
            .workspace
            .resolve_path(workspace_id, &relative)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        if fs::rename(&file.temp_path, &dest).await.is_err() {
            fs::copy(&file.temp_path, &dest)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let _ = fs::remove_file(&file.temp_path).await;
        }
        output.push(relative);
    }
    state.workspace.refresh_workspace_tree(workspace_id);
    Ok(output)
}

async fn ensure_output_dir(state: &AppState, workspace_id: &str) -> Result<(), Response> {
    let output_dir = state
        .workspace
        .resolve_path(workspace_id, "output")
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    fs::create_dir_all(output_dir)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

async fn update_run_terminal_if_current(
    state: &AppState,
    prepared: &PreparedWorkflow,
    status: &str,
    answer: Option<&str>,
    error: Option<&str>,
    files: Option<Vec<WorkflowFileRef>>,
    stop_reason: Option<&str>,
    usage: Option<&TokenUsage>,
) {
    let Some(existing) = state
        .user_store
        .get_session_run(&prepared.run_id)
        .ok()
        .flatten()
    else {
        return;
    };
    if is_terminal_status(&existing.status) {
        return;
    }

    let finished = now_ts();
    let mut metadata = existing.metadata.unwrap_or_else(|| json!({}));
    if let Some(files) = files {
        insert_metadata_field(&mut metadata, "files", json!(files));
    }
    if let Some(stop_reason) = stop_reason {
        insert_metadata_field(&mut metadata, "stop_reason", json!(stop_reason));
    }
    if let Some(usage) = usage {
        insert_metadata_field(&mut metadata, "usage", json!(usage));
    }
    let record = SessionRunRecord {
        run_id: prepared.run_id.clone(),
        session_id: prepared.session_id.clone(),
        parent_session_id: None,
        user_id: prepared.user.user_id.clone(),
        dispatch_id: prepared
            .request
            .client_run_id
            .clone()
            .or_else(|| Some(prepared.run_id.clone())),
        run_kind: Some(RUN_KIND.to_string()),
        requested_by: Some(REQUESTED_BY.to_string()),
        agent_id: Some(prepared.agent.agent_id.clone()),
        model_name: prepared.agent.model_name.clone(),
        status: status.to_string(),
        queued_time: prepared.started_at,
        started_time: prepared.started_at,
        finished_time: if is_terminal_status(status) {
            finished
        } else {
            0.0
        },
        elapsed_s: if is_terminal_status(status) {
            (finished - prepared.started_at).max(0.0)
        } else {
            0.0
        },
        result: answer.map(ToString::to_string),
        error: error.map(ToString::to_string),
        updated_time: finished,
        metadata: Some(metadata),
    };
    let _ = state.user_store.upsert_session_run(&record);
}

async fn finish_workflow_with_error(
    state: &AppState,
    prepared: &PreparedWorkflow,
    tx: &tokio::sync::mpsc::Sender<Event>,
    status: &str,
    error_text: String,
) {
    update_run_terminal_if_current(
        state,
        prepared,
        status,
        None,
        Some(&error_text),
        None,
        None,
        None,
    )
    .await;
    release_active_workflow(state, &prepared.user.user_id, &prepared.run_id);
    let _ = send_workflow_event(
        tx,
        "workflow.error",
        None,
        json!({
            "run_id": prepared.run_id,
            "session_id": prepared.session_id,
            "status": status,
            "error": error_text,
        }),
    )
    .await;
}

async fn collect_workflow_files(
    state: &AppState,
    prepared: &PreparedWorkflow,
    final_answer: Option<&str>,
) -> Vec<WorkflowFileRef> {
    let root = match state.workspace.ensure_user_root(&prepared.workspace_id) {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
    let output_root = root.join("output");
    let mut files = Vec::new();
    if output_root.exists() && output_root.is_dir() {
        for entry in WalkDir::new(&output_root)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(&root) {
                let relative = relative.to_string_lossy().replace('\\', "/");
                if let Some(file_ref) = build_file_ref(&prepared.run_id, &relative, path) {
                    files.push(file_ref);
                }
            }
        }
    }
    for relative in
        referenced_workflow_paths(final_answer.unwrap_or_default(), &prepared.workspace_id)
    {
        if files.iter().any(|item| item.path == relative) {
            continue;
        }
        let target = match state
            .workspace
            .resolve_path(&prepared.workspace_id, &relative)
        {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !target.is_file() {
            continue;
        }
        if let Some(file_ref) = build_file_ref(&prepared.run_id, &relative, &target) {
            files.push(file_ref);
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn build_file_ref(run_id: &str, relative: &str, path: &Path) -> Option<WorkflowFileRef> {
    let metadata = path.metadata().ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let file_id = file_id_for_path(relative);
    Some(WorkflowFileRef {
        download_url: format!("/wunder/external/workflows/{run_id}/files/{file_id}"),
        file_id,
        path: relative.to_string(),
        name,
        size: metadata.len(),
    })
}

fn referenced_workflow_paths(text: &str, workspace_id: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for caps in workflow_path_regex().captures_iter(text) {
        let Some(raw) = caps.get(0).map(|matched| matched.as_str()) else {
            continue;
        };
        let cleaned = normalize_referenced_workflow_path_for_workspace(raw, workspace_id);
        if cleaned.is_empty() || seen.contains(&cleaned) {
            continue;
        }
        seen.insert(cleaned.clone());
        output.push(cleaned);
    }
    output
}

fn normalize_referenced_workflow_path(raw: &str) -> String {
    raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ' ' | '\t' | '\r' | '\n' | ')' | ']' | '}' | ',' | ';' | '。' | '，'
        )
    })
    .replace('\\', "/")
}

fn normalize_referenced_workflow_path_for_workspace(raw: &str, workspace_id: &str) -> String {
    let cleaned = normalize_referenced_workflow_path(raw)
        .trim_matches(|ch: char| matches!(ch, '.' | '。' | '，' | '；'))
        .to_string();
    if cleaned.starts_with("input/") || cleaned.starts_with("output/") {
        return cleaned;
    }
    let Some(public_relative) = cleaned
        .trim_start_matches('/')
        .strip_prefix("workspaces/")
        .map(|value| value.trim_start_matches('/'))
    else {
        return cleaned;
    };
    let scoped_prefix = workspace_id.trim_matches('/');
    if let Some(relative) = public_relative
        .strip_prefix(scoped_prefix)
        .map(|value| value.trim_start_matches('/'))
    {
        return relative.to_string();
    }
    String::new()
}

fn workflow_path_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?:/?workspaces/[^\s"'`<>()\[\]{}]+|(?:input|output)/[^\s"'`<>()\[\]{}]+|[A-Za-z0-9][A-Za-z0-9._-]*\.[A-Za-z0-9]{1,16})"#,
        )
            .expect("valid external workflow file path regex")
    })
}

fn active_workflow_meta_key(user_id: &str) -> String {
    format!("{ACTIVE_RUN_META_PREFIX}{user_id}:{EXTERNAL_WORKFLOW_CONTAINER_ID}")
}

fn load_active_workflow(state: &AppState, user_id: &str) -> Result<Option<String>, Response> {
    let key = active_workflow_meta_key(user_id);
    let Some(run_id) = state
        .user_store
        .get_meta(&key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(run) = state
        .user_store
        .get_session_run(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        release_active_workflow(state, user_id, &run_id);
        return Ok(None);
    };
    if is_terminal_status(&run.status) {
        release_active_workflow(state, user_id, &run_id);
        return Ok(None);
    }
    Ok(Some(run_id))
}

fn set_active_workflow(state: &AppState, user_id: &str, run_id: &str) -> Result<(), Response> {
    state
        .user_store
        .set_meta(&active_workflow_meta_key(user_id), run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

fn release_active_workflow(state: &AppState, user_id: &str, run_id: &str) {
    let key = active_workflow_meta_key(user_id);
    if state
        .user_store
        .get_meta(&key)
        .ok()
        .flatten()
        .map(|value| value.trim() == run_id)
        .unwrap_or(false)
    {
        let _ = state.storage.delete_meta_prefix(&key);
    }
}

fn files_from_record(record: &SessionRunRecord) -> Vec<WorkflowFileRef> {
    record
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("files"))
        .and_then(|value| serde_json::from_value::<Vec<WorkflowFileRef>>(value.clone()).ok())
        .unwrap_or_default()
}

async fn load_authorized_run(
    state: &AppState,
    headers: &HeaderMap,
    run_id: &str,
) -> Result<SessionRunRecord, Response> {
    let record = state
        .user_store
        .get_session_run(run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .filter(|record| record.run_kind.as_deref() == Some(RUN_KIND))
        .ok_or_else(|| {
            error_with_code(
                StatusCode::NOT_FOUND,
                "WORKFLOW_NOT_FOUND",
                "workflow run not found".to_string(),
            )
        })?;
    if is_external_workflow_key_or_admin(state, headers).await {
        return Ok(record);
    }

    let resolved = resolve_user(state, headers, Some(&record.user_id)).await?;
    if resolved.user.user_id != record.user_id && !UserStore::is_admin(&resolved.user) {
        return Err(error_with_code(
            StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            i18n::t("error.permission_denied"),
        ));
    }
    Ok(record)
}

fn run_record_payload(record: &SessionRunRecord) -> Value {
    let files = files_from_record(record);
    json!({
        "run_id": record.run_id,
        "session_id": record.session_id,
        "user_id": record.user_id,
        "agent_id": record.agent_id,
        "status": record.status,
        "answer": record.result,
        "stop_reason": record.metadata.as_ref().and_then(|meta| meta.get("stop_reason")).cloned().unwrap_or(Value::Null),
        "usage": record.metadata.as_ref().and_then(|meta| meta.get("usage")).cloned().unwrap_or(Value::Null),
        "files": files,
        "created_at": record.queued_time,
        "started_at": record.started_time,
        "finished_at": record.finished_time,
        "elapsed_s": record.elapsed_s,
        "error": record.error,
        "metadata": record.metadata,
    })
}

fn workflow_started_payload(prepared: &PreparedWorkflow, status: &str) -> Value {
    json!({
        "run_id": prepared.run_id,
        "session_id": prepared.session_id,
        "user_id": prepared.user.user_id,
        "agent_id": prepared.agent.agent_id,
        "status": status,
        "workspace_container_id": EXTERNAL_WORKFLOW_CONTAINER_ID,
        "events_url": format!("/wunder/external/workflows/{}/events", prepared.run_id),
        "cancel_url": format!("/wunder/external/workflows/{}/cancel", prepared.run_id),
    })
}

fn workflow_event_payload(run_id: &str, session_id: &str, event: &StreamEvent) -> Value {
    json!({
        "run_id": run_id,
        "session_id": session_id,
        "type": event.event,
        "event_id": event.id.as_deref().and_then(|value| value.parse::<i64>().ok()),
        "timestamp": event.timestamp,
        "data": inner_event_data(event),
    })
}

fn map_persisted_event(record: &SessionRunRecord, row: Value) -> Value {
    json!({
        "run_id": record.run_id,
        "session_id": record.session_id,
        "type": row.get("event").and_then(Value::as_str).unwrap_or(""),
        "event_id": row.get("event_id").and_then(Value::as_i64).unwrap_or(0),
        "timestamp": row.get("timestamp").cloned().unwrap_or(Value::Null),
        "data": row.get("data").cloned().unwrap_or(Value::Null),
    })
}

async fn send_workflow_event(
    tx: &tokio::sync::mpsc::Sender<Event>,
    event_name: &str,
    id: Option<&str>,
    payload: Value,
) -> Result<(), tokio::sync::mpsc::error::SendError<Event>> {
    let mut event = Event::default().event(event_name).data(payload.to_string());
    if let Some(id) = id {
        event = event.id(id);
    }
    tx.send(event).await
}

fn inner_event_data(event: &StreamEvent) -> Value {
    event
        .data
        .get("data")
        .cloned()
        .unwrap_or_else(|| event.data.clone())
}

fn extract_error_text(event: &StreamEvent) -> String {
    let data = inner_event_data(event);
    data.get("message")
        .or_else(|| data.get("error"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| data.to_string())
}

fn is_cancelled_error(event: &StreamEvent) -> bool {
    let data = inner_event_data(event);
    data.get("code")
        .and_then(Value::as_str)
        .map(|value| value == "CANCELLED")
        .unwrap_or(false)
}

fn resolve_allowed_agent_tools(allowed: HashSet<String>, agent_defaults: &[String]) -> Vec<String> {
    if allowed.is_empty() {
        return vec!["__no_tools__".to_string()];
    }
    let scoped: HashSet<String> = agent_defaults
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect();
    let mut list = if scoped.is_empty() { allowed } else { scoped }
        .into_iter()
        .collect::<Vec<_>>();
    list.sort();
    list
}

fn build_workflow_message(message: &str, input_files: &[String]) -> String {
    let mut output = message.trim().to_string();
    if !input_files.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str("Input files are available in workspace container 10:\n");
        for path in input_files {
            output.push_str("- ");
            output.push_str(path);
            output.push('\n');
        }
        output.push_str("\nWrite any downloadable deliverables under output/.");
    }
    output
}

fn normalize_approval_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "suggest" => "suggest".to_string(),
        "full_auto" | "full-auto" => "full_auto".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        _ => "full_auto".to_string(),
    }
}

fn validate_content_length(headers: &HeaderMap) -> Result<(), Response> {
    if let Some(length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > MAX_UPLOAD_BYTES as u64 {
            return Err(error_with_code(
                StatusCode::PAYLOAD_TOO_LARGE,
                "PAYLOAD_TOO_LARGE",
                "external workflow upload is too large".to_string(),
            ));
        }
    }
    Ok(())
}

async fn save_multipart_file(
    mut field: axum::extract::multipart::Field<'_>,
    target: &Path,
) -> Result<(), Response> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut file = fs::File::create(target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(())
}

fn create_temp_upload_dir() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_external_workflows");
    root.push(Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

fn cleanup_temp_uploads(files: &[PendingUpload], dir: Option<&PathBuf>) {
    for file in files {
        let _ = std::fs::remove_file(&file.temp_path);
    }
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn stream_response<S>(stream: S, filename: &str, content_type: &'static str) -> Response
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let disposition = build_content_disposition(filename);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn build_content_disposition(filename: &str) -> String {
    let ascii_name = sanitize_ascii_filename(filename);
    if ascii_name == filename {
        return format!("attachment; filename=\"{ascii_name}\"");
    }
    let encoded = percent_encode(filename);
    format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}")
}

fn sanitize_filename(value: &str) -> String {
    let fallback = "upload";
    let basename = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value);
    let mut output = String::new();
    for ch in basename.chars() {
        if ch == '/' || ch == '\\' || ch.is_control() {
            output.push('_');
        } else if matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    let trimmed = output.trim_matches('.');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn sanitize_ascii_filename(value: &str) -> String {
    let fallback = "download";
    let cleaned = sanitize_filename(value);
    let mut output = String::new();
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    let trimmed = output.trim_matches('.');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn unique_filename(name: &str, used: &mut HashSet<String>) -> String {
    let cleaned = sanitize_filename(name);
    if used.insert(cleaned.clone()) {
        return cleaned;
    }
    let path = Path::new(&cleaned);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("upload");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    for index in 2.. {
        let candidate = if ext.is_empty() {
            format!("{stem}_{index}")
        } else {
            format!("{stem}_{index}.{ext}")
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    cleaned
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn workflow_workspace_id(state: &AppState, user_id: &str) -> String {
    state
        .workspace
        .scoped_user_id_by_container(user_id, EXTERNAL_WORKFLOW_CONTAINER_ID)
}

fn is_default_agent_alias(value: &str) -> bool {
    value.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS) || value.eq_ignore_ascii_case("default")
}

fn normalize_lookup_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn timeout_s(request: &ExternalWorkflowRequest) -> f64 {
    request
        .timeout_s
        .unwrap_or(DEFAULT_TIMEOUT_S)
        .clamp(1.0, MAX_TIMEOUT_S)
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status.trim(),
        "completed" | "success" | "error" | "failed" | "timeout" | "cancelled"
    )
}

fn file_id_for_path(path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("file_{hash:016x}")
}

fn insert_metadata_field(metadata: &mut Value, key: &str, value: Value) {
    if let Value::Object(map) = metadata {
        map.insert(key.to_string(), value);
    } else {
        *metadata = json!({ key: value });
    }
}

async fn is_external_workflow_key_or_admin(state: &AppState, headers: &HeaderMap) -> bool {
    let config = state.config_store.get().await;
    if config.external_auth_key().as_ref().is_some_and(|expected| {
        guard_auth::extract_api_key(headers)
            .map(|provided| provided == *expected)
            .unwrap_or(false)
    }) {
        return true;
    }
    if let Some(token) = guard_auth::extract_bearer_token(headers) {
        let user_store = state.user_store.clone();
        if let Ok(Ok(Some(user))) =
            tokio::task::spawn_blocking(move || user_store.authenticate_token(&token)).await
        {
            return UserStore::is_admin(&user);
        }
    }
    false
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn error_with_code(status: StatusCode, code: &str, message: String) -> Response {
    crate::api::errors::error_response_with_detail(status, Some(code), message, None, None)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::external::DEFAULT_EXTERNAL_LAUNCH_PASSWORD;
    use crate::storage::{SqliteStorage, StorageBackend};

    #[test]
    fn sanitize_filename_removes_path_and_unsafe_chars() {
        assert_eq!(sanitize_filename("../reports/final?.txt"), "final_.txt");
        assert_eq!(sanitize_filename("需求.txt"), "需求.txt");
        assert_eq!(sanitize_filename(".."), "upload");
        assert_eq!(sanitize_filename("space name.csv"), "space name.csv");
    }

    #[test]
    fn sanitize_ascii_filename_keeps_header_fallback_ascii() {
        assert_eq!(sanitize_ascii_filename("需求.txt"), "__.txt");
    }

    #[test]
    fn unique_filename_appends_stable_suffixes() {
        let mut used = HashSet::new();

        assert_eq!(unique_filename("result.txt", &mut used), "result.txt");
        assert_eq!(unique_filename("result.txt", &mut used), "result_2.txt");
        assert_eq!(unique_filename("result.txt", &mut used), "result_3.txt");
        assert_eq!(unique_filename("需求.txt", &mut used), "需求.txt");
    }

    #[test]
    fn referenced_workflow_paths_extracts_input_and_output_files() {
        let paths = referenced_workflow_paths(
            "See output/final.txt, output/final.txt and `input/source.csv`.",
            "admin__c__10",
        );

        assert_eq!(
            paths,
            vec![
                "output/final.txt".to_string(),
                "input/source.csv".to_string()
            ]
        );
    }

    #[test]
    fn referenced_workflow_paths_extracts_public_workspace_links() {
        let paths = referenced_workflow_paths(
            "Image: ![heart](/workspaces/admin__c__10/heart.png) and code `heart.py`.",
            "admin__c__10",
        );

        assert_eq!(paths, vec!["heart.png".to_string(), "heart.py".to_string()]);
    }

    #[test]
    fn referenced_workflow_paths_extracts_plain_filenames() {
        let paths = referenced_workflow_paths("Files: heart.png and `heart.py`.", "admin__c__10");

        assert_eq!(paths, vec!["heart.png".to_string(), "heart.py".to_string()]);
    }

    #[test]
    fn external_workflow_timeout_defaults_to_long_running_budget() {
        let mut request = ExternalWorkflowRequest {
            user_id: None,
            user_name: None,
            agent_id: None,
            agent_name: None,
            message: None,
            workspace_container_id: None,
            clear_workspace: None,
            timeout_s: None,
            client_run_id: None,
            metadata: None,
        };

        assert_eq!(timeout_s(&request), 6000.0);

        request.timeout_s = Some(0.0);
        assert_eq!(timeout_s(&request), 1.0);

        request.timeout_s = Some(7200.0);
        assert_eq!(timeout_s(&request), MAX_TIMEOUT_S);
    }

    #[test]
    fn external_workflow_user_resolution_provisions_missing_user() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("external-workflow-user.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage as Arc<dyn StorageBackend>);

        let user = resolve_or_provision_external_workflow_user(&store, "external_api_user")
            .expect("provision user");

        assert_eq!(user.user_id, "external_api_user");
        let login = store
            .login("external_api_user", DEFAULT_EXTERNAL_LAUNCH_PASSWORD)
            .expect("login with default external password");
        assert_eq!(login.user.user_id, "external_api_user");
    }

    #[test]
    fn external_workflow_user_resolution_rejects_default_admin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("external-workflow-admin.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage as Arc<dyn StorageBackend>);

        let err = resolve_or_provision_external_workflow_user(&store, "admin")
            .expect_err("admin should not be provisioned");

        assert!(err.to_string().contains("admin account is protected"));
    }
}
