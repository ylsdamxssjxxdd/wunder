use super::session_run_stream;
use super::swarm_realtime::reconcile_swarm_task_from_session_run;
use super::{
    append_child_announce, build_effective_tool_names, collect_user_allowed_tools,
    finalize_tool_names, insert_run_metadata_field, load_agent_record, normalize_optional_string,
    now_ts, resolve_child_session_tool_names, should_skip_announce, truncate_text, AnnounceConfig,
    ChildSessionToolMode, SessionCleanup, ToolContext,
};
use crate::config::Config;
use crate::core::blocking;
use crate::core::long_task;
use crate::history::HistoryManager;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::truncate_tool_result_text;
use crate::schemas::WunderRequest;
use crate::services::subagents;
use crate::storage::{
    AgentThreadRecord, ChatSessionRecord, SessionRunRecord, StorageBackend, UserAgentRecord,
};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::sleep;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_SESSION_TITLE: &str = "新会话";
const SUBAGENT_MESSAGE_PREVIEW_MAX_CHARS: usize = 240;
const MAX_SUBAGENT_SESSION_DEPTH: usize = 32;
const SESSION_RUN_BLOCKING_EXEC_TIMEOUT_S: u64 = 24 * 60 * 60;

#[derive(Debug)]
pub(crate) struct SessionRunOutcome {
    pub(crate) status: String,
    pub(crate) answer: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) elapsed_s: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionRunMeta {
    pub(crate) dispatch_id: Option<String>,
    pub(crate) run_kind: Option<String>,
    pub(crate) requested_by: Option<String>,
    pub(crate) team_task_id: Option<String>,
    pub(crate) metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedChildSession {
    pub(crate) child_session_id: String,
    pub(crate) child_agent_id: Option<String>,
    pub(crate) model_name: Option<String>,
    pub(crate) request: WunderRequest,
    pub(crate) announce: AnnounceConfig,
    pub(crate) run_metadata: Value,
}

fn child_session_depth(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
) -> i64 {
    let cleaned_user = user_id.trim();
    let cleaned_parent = parent_session_id.trim();
    if cleaned_user.is_empty() || cleaned_parent.is_empty() {
        return 1;
    }
    let mut depth = 0_i64;
    let mut cursor = Some(cleaned_parent.to_string());
    let mut seen = HashSet::new();
    while let Some(session_id) = cursor {
        if !seen.insert(session_id.clone()) || depth >= MAX_SUBAGENT_SESSION_DEPTH as i64 {
            break;
        }
        let Some(session) = storage
            .get_chat_session(cleaned_user, &session_id)
            .ok()
            .flatten()
        else {
            break;
        };
        let Some(next_parent) = session
            .parent_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            break;
        };
        depth += 1;
        cursor = Some(next_parent.to_string());
    }
    depth + 1
}

fn subagent_control_scope(tool_names: &[String]) -> &'static str {
    if tool_names
        .iter()
        .any(|name| super::resolve_tool_name(name) == "子智能体控制")
    {
        "children"
    } else {
        "none"
    }
}

fn subagent_role_for_scope(control_scope: &str) -> &'static str {
    if control_scope == "children" {
        "orchestrator"
    } else {
        "worker"
    }
}

fn build_prepared_child_run_metadata(
    context: &ToolContext<'_>,
    parent_session_id: &str,
    parent_tool_names: &[String],
    parent_turn_ref: Option<&str>,
) -> Value {
    let depth = child_session_depth(context.storage.as_ref(), context.user_id, parent_session_id);
    let control_scope = subagent_control_scope(parent_tool_names);
    json!({
        "controller_session_id": parent_session_id.trim(),
        "parent_turn_ref": parent_turn_ref.map(str::to_string),
        "parent_user_round": context.user_round,
        "parent_model_round": context.model_round,
        "depth": depth,
        "role": subagent_role_for_scope(control_scope),
        "control_scope": control_scope,
        "auto_wake": false,
        "emit_parent_events": true,
    })
}

pub(crate) fn prepare_swarm_child_session(
    context: &ToolContext<'_>,
    task: &str,
    label: Option<String>,
    agent_id: &str,
) -> Result<PreparedChildSession> {
    let parent_session_id = context.session_id.trim();
    if parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }
    let prepared = prepare_child_session(
        context,
        parent_session_id,
        task,
        label,
        Some(agent_id.to_string()),
        None,
        ChildSessionToolMode::UseTargetAgentDefaults,
    )?;
    let user_id = context.user_id.trim();
    if !user_id.is_empty() {
        if let Some(ref child_agent_id) = prepared.child_agent_id {
            bind_child_session_as_agent_main_thread(
                context.storage.as_ref(),
                user_id,
                child_agent_id,
                &prepared.child_session_id,
            )?;
        }
        if let Some(mut session) = context
            .storage
            .get_chat_session(user_id, &prepared.child_session_id)?
        {
            session.spawned_by = Some("agent_swarm".to_string());
            context.storage.upsert_chat_session(&session)?;
        }
    }
    Ok(prepared)
}

pub(crate) fn prepare_child_session(
    context: &ToolContext<'_>,
    parent_session_id: &str,
    task: &str,
    label: Option<String>,
    agent_id: Option<String>,
    model_name: Option<String>,
    tool_mode: ChildSessionToolMode,
) -> Result<PreparedChildSession> {
    let cleaned_task = task.trim();
    if cleaned_task.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let cleaned_parent_session_id = parent_session_id.trim();
    if cleaned_parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let label = normalize_optional_string(label);
    let agent_id = normalize_optional_string(agent_id);
    let model_name = normalize_optional_string(model_name);
    let parent_record = context
        .storage
        .get_chat_session(user_id, cleaned_parent_session_id)
        .unwrap_or(None);
    let parent_agent_id = parent_record
        .as_ref()
        .and_then(|record| record.agent_id.clone())
        .or_else(|| context.agent_id.map(|value| value.to_string()));
    let parent_agent_record = load_agent_record(
        context.storage.as_ref(),
        user_id,
        parent_agent_id.as_deref(),
        true,
    )?;
    let parent_tool_names = if let Some(record) = parent_record.as_ref() {
        build_effective_tool_names(context, user_id, record, parent_agent_record.as_ref())?
    } else {
        finalize_tool_names(collect_user_allowed_tools(context, user_id)?)
    };

    let (child_agent_id, child_agent_record) = resolve_child_agent(
        context.storage.as_ref(),
        user_id,
        agent_id.as_deref(),
        parent_agent_id.as_deref(),
    )?;
    let child_tool_names = resolve_child_session_tool_names(
        tool_mode,
        &parent_tool_names,
        child_agent_record.as_ref(),
    );
    // Keep the first spawned child turn aligned with the same effective model
    // resolution used by the normal chat entrypoint.
    let resolved_model_name = model_name.or_else(|| {
        resolve_effective_agent_model_name(context.config, child_agent_record.as_ref())
    });
    let agent_prompt = child_agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = child_agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);

    let now = now_ts();
    let child_session_id = format!("sess_{}", Uuid::new_v4().simple());
    let parent_turn_ref =
        subagents::encode_parent_turn_ref(context.user_round, context.model_round);
    let child_record = ChatSessionRecord {
        session_id: child_session_id.clone(),
        user_id: user_id.to_string(),
        title: label
            .clone()
            .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: child_agent_id.clone(),
        tool_overrides: child_tool_names.clone(),
        parent_session_id: Some(cleaned_parent_session_id.to_string()),
        parent_message_id: parent_turn_ref.clone(),
        spawn_label: label.clone(),
        spawned_by: Some("model".to_string()),
    };
    context.storage.upsert_chat_session(&child_record)?;
    let run_metadata = build_prepared_child_run_metadata(
        context,
        cleaned_parent_session_id,
        &parent_tool_names,
        parent_turn_ref.as_deref(),
    );

    Ok(PreparedChildSession {
        child_session_id: child_session_id.clone(),
        child_agent_id: child_agent_id.clone(),
        model_name: resolved_model_name.clone(),
        request: WunderRequest {
            user_id: user_id.to_string(),
            question: cleaned_task.to_string(),
            client_message_id: None,
            tool_names: child_tool_names,
            skip_tool_calls: false,
            stream: true,
            debug_payload: false,
            session_id: Some(child_session_id),
            agent_id: child_agent_id,
            workspace_container_id: None,
            model_name: resolved_model_name,
            language: Some(i18n::get_language()),
            config_overrides: context.request_config_overrides.cloned(),
            agent_prompt,
            preview_skill,
            attachments: None,
            allow_queue: true,
            is_admin: context.is_admin,
            enforce_runtime_queue: false,
            approval_tx: None,
        },
        announce: AnnounceConfig {
            parent_session_id: cleaned_parent_session_id.to_string(),
            label,
            dispatch_id: None,
            strategy: None,
            completion_mode: None,
            remaining_action: None,
            parent_turn_ref,
            parent_user_round: context.user_round,
            parent_model_round: context.model_round,
            emit_parent_events: true,
            // Background child runs wake the parent once they settle.
            // Waiting parent turns collect the result inline and disable auto_wake later.
            auto_wake: false,
            persist_history_message: false,
        },
        run_metadata,
    })
}

pub(crate) fn bind_child_session_as_agent_main_thread(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
    child_session_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_agent_id = agent_id.trim();
    let cleaned_child_session_id = child_session_id.trim();
    if cleaned_user_id.is_empty()
        || cleaned_agent_id.is_empty()
        || cleaned_child_session_id.is_empty()
    {
        return Ok(());
    }
    let now = now_ts();
    let existing_thread = storage.get_agent_thread(cleaned_user_id, cleaned_agent_id)?;
    let thread_record = AgentThreadRecord {
        thread_id: format!("thread_{cleaned_child_session_id}"),
        user_id: cleaned_user_id.to_string(),
        agent_id: cleaned_agent_id.to_string(),
        session_id: cleaned_child_session_id.to_string(),
        status: existing_thread
            .as_ref()
            .map(|record| record.status.trim().to_string())
            .filter(|status| !status.is_empty())
            .unwrap_or_else(|| "idle".to_string()),
        created_at: existing_thread
            .as_ref()
            .map(|record| record.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    storage.upsert_agent_thread(&thread_record)?;
    Ok(())
}

pub(crate) fn resolve_effective_agent_model_name(
    config: &Config,
    agent_record: Option<&UserAgentRecord>,
) -> Option<String> {
    if let Some(model_name) = agent_record
        .and_then(|record| normalize_optional_string(record.model_name.clone()))
        .filter(|model_name| {
            config
                .llm
                .models
                .get(model_name)
                .is_some_and(crate::services::llm::is_llm_model)
        })
    {
        return Some(model_name);
    }

    let default_model_name = config.llm.default.trim();
    if config
        .llm
        .models
        .get(default_model_name)
        .is_some_and(crate::services::llm::is_llm_model)
    {
        return Some(default_model_name.to_string());
    }

    config.llm.models.iter().find_map(|(name, model)| {
        if !crate::services::llm::is_llm_model(model) {
            return None;
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_child_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    requested_agent_id: Option<&str>,
    parent_agent_id: Option<&str>,
) -> Result<(Option<String>, Option<UserAgentRecord>)> {
    let requested = requested_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(requested) = requested {
        if let Some(record) = load_agent_record(storage, user_id, Some(requested), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    let parent = parent_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(parent) = parent {
        if let Some(record) = load_agent_record(storage, user_id, Some(parent), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    Ok((None, None))
}

fn session_run_runtime() -> &'static tokio::runtime::Runtime {
    static SESSION_RUN_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    SESSION_RUN_RUNTIME.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().clamp(8, 128))
            .unwrap_or(16);
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .max_blocking_threads(1024)
            .thread_name("wunder-session-run")
            .enable_all()
            .build()
            .expect("build session run runtime")
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn spawn_session_run(
    context: &ToolContext<'_>,
    request: WunderRequest,
    run_id: String,
    parent_session_id: Option<String>,
    agent_id: Option<String>,
    model_name: Option<String>,
    run_meta: SessionRunMeta,
    announce: Option<AnnounceConfig>,
    cleanup: SessionCleanup,
    run_timeout_s: Option<f64>,
) -> Result<oneshot::Receiver<SessionRunOutcome>> {
    let orchestrator = context
        .orchestrator
        .clone()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let session_id = request
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let user_id = request.user_id.clone();
    let request_config_overrides = request.config_overrides.clone();
    let mut run_metadata = match run_meta.metadata.clone() {
        Some(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    };
    let user_message_preview =
        truncate_text(request.question.trim(), SUBAGENT_MESSAGE_PREVIEW_MAX_CHARS);
    if !user_message_preview.is_empty() {
        insert_run_metadata_field(
            &mut run_metadata,
            "user_message_preview",
            json!(user_message_preview),
        );
    }
    if let Some(team_task_id) = run_meta.team_task_id.as_deref() {
        insert_run_metadata_field(&mut run_metadata, "team_task_id", json!(team_task_id));
    }
    let now = now_ts();
    let record = SessionRunRecord {
        run_id: run_id.clone(),
        session_id: session_id.clone(),
        parent_session_id: parent_session_id.clone(),
        user_id: user_id.clone(),
        dispatch_id: run_meta.dispatch_id.clone(),
        run_kind: run_meta.run_kind.clone(),
        requested_by: run_meta.requested_by.clone(),
        agent_id: agent_id.clone(),
        model_name: model_name.clone(),
        status: "queued".to_string(),
        queued_time: now,
        started_time: 0.0,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: now,
        metadata: Some(run_metadata),
    };
    {
        let queued_storage = context.storage.clone();
        let queued_record = record.clone();
        blocking::run_db("tools.session_run.queued", move || {
            queued_storage.upsert_session_run(&queued_record)
        })
        .await?;
    }

    let storage = context.storage.clone();
    let workspace = context.workspace.clone();
    let monitor = context.monitor.clone();
    let beeroom_realtime = context.beeroom_realtime.clone();
    let swarm_team_task_id = run_meta.team_task_id.clone();
    let (tx, rx) = oneshot::channel::<SessionRunOutcome>();
    let announce_for_start = announce.clone();
    long_task::spawn("tools.session_run.lifecycle", async move {
        let started = now_ts();
        let running = SessionRunRecord {
            status: "running".to_string(),
            started_time: started,
            updated_time: started,
            ..record.clone()
        };
        {
            let storage_for_start = storage.clone();
            let user_for_start = user_id.clone();
            let session_for_start = session_id.clone();
            let running_for_start = running.clone();
            let _ = blocking::run_db("tools.session_run.started", move || {
                let _ = storage_for_start.touch_chat_session(
                    &user_for_start,
                    &session_for_start,
                    started,
                    started,
                );
                let _ = storage_for_start.upsert_session_run(&running_for_start);
                Ok(())
            })
            .await;
        }
        if let Some(config) = announce_for_start
            .as_ref()
            .filter(|entry| entry.emit_parent_events)
        {
            let _ = subagents::emit_child_runtime_update(
                storage.clone(),
                monitor.as_ref().map(Arc::clone),
                &user_id,
                &config.parent_session_id,
                &session_id,
            )
            .await;
        }

        // Use a dedicated runtime so high fan-out runs do not contend with the main runtime worker pool.
        let mut run_request = request;
        run_request.stream = true;
        let child_orchestrator = orchestrator.clone();
        let blocking_exec_timeout = run_timeout_s
            .filter(|value| *value > 0.0)
            .map(|value| Duration::from_secs_f64(value + 5.0))
            .unwrap_or_else(|| Duration::from_secs(SESSION_RUN_BLOCKING_EXEC_TIMEOUT_S));
        let mut run_handle = long_task::spawn(
            "tools.session_run.execute",
            blocking::run_external_with_timeout(
                "tools.session_run.execute",
                blocking_exec_timeout,
                move || {
                    session_run_runtime().block_on(session_run_stream::run_request(
                        child_orchestrator,
                        run_request,
                    ))
                },
            ),
        );
        let mut timeout_triggered = false;
        let run_result = if let Some(timeout_s) = run_timeout_s.filter(|value| *value > 0.0) {
            let timeout_duration = Duration::from_secs_f64(timeout_s);
            tokio::select! {
                res = &mut run_handle => match res {
                    Ok(value) => value,
                    Err(err) => Err(anyhow!(err.to_string())),
                },
                _ = sleep(timeout_duration) => {
                    timeout_triggered = true;
                    run_handle.abort();
                    if let Some(monitor) = monitor.as_ref() {
                        let _ = monitor.cancel(&session_id);
                    }
                    Err(anyhow!("timeout"))
                }
            }
        } else {
            match run_handle.await {
                Ok(value) => value,
                Err(err) => Err(anyhow!(err.to_string())),
            }
        };
        let finished = now_ts();
        let elapsed = (finished - started).max(0.0);
        let (status, answer, error) = match run_result {
            Ok(response) => {
                let answer =
                    truncate_tool_result_text(response.answer.as_deref().unwrap_or_default());
                ("success".to_string(), Some(answer), None)
            }
            Err(err) => {
                if timeout_triggered {
                    ("timeout".to_string(), None, Some("timeout".to_string()))
                } else {
                    (
                        "error".to_string(),
                        None,
                        Some(truncate_tool_result_text(&err.to_string())),
                    )
                }
            }
        };
        let finished_record = SessionRunRecord {
            status: status.clone(),
            finished_time: finished,
            elapsed_s: elapsed,
            result: answer.clone(),
            error: error.clone(),
            updated_time: finished,
            ..running
        };
        {
            let storage_for_finish = storage.clone();
            let finished_for_write = finished_record.clone();
            let _ = blocking::run_db("tools.session_run.finished", move || {
                let _ = storage_for_finish.upsert_session_run(&finished_for_write);
                Ok(())
            })
            .await;
        }
        if let Some(team_task_id) = swarm_team_task_id.as_deref() {
            if let Err(err) = reconcile_swarm_task_from_session_run(
                storage.as_ref(),
                monitor.as_ref().map(Arc::clone),
                beeroom_realtime.as_ref().map(Arc::clone),
                team_task_id,
                &finished_record,
            ) {
                warn!(
                    task_id = team_task_id,
                    run_id = %finished_record.run_id,
                    "failed to reconcile swarm task from session run: {err}"
                );
            }
        }

        if let Some(announce) = announce {
            if announce.persist_history_message && !should_skip_announce(answer.as_deref()) {
                append_child_announce(
                    &workspace,
                    &storage,
                    &user_id,
                    &announce.parent_session_id,
                    &session_id,
                    &run_id,
                    &status,
                    answer.as_deref(),
                    error.as_deref(),
                    elapsed,
                    model_name.as_deref(),
                    announce.label.as_deref(),
                    announce.parent_user_round,
                    announce.parent_model_round,
                );
            }
            if announce.emit_parent_events || announce.auto_wake {
                let parent_dispatch = subagents::ParentDispatchConfig {
                    parent_session_id: announce.parent_session_id.clone(),
                    dispatch_id: announce.dispatch_id.clone(),
                    strategy: announce.strategy.clone(),
                    completion_mode: announce.completion_mode.clone(),
                    remaining_action: announce.remaining_action.clone(),
                    label: announce.label.clone(),
                    parent_turn_ref: announce.parent_turn_ref.clone(),
                    parent_user_round: announce.parent_user_round,
                    parent_model_round: announce.parent_model_round,
                    emit_parent_events: announce.emit_parent_events,
                    auto_wake: announce.auto_wake,
                };
                subagents::handle_child_completion(
                    storage.clone(),
                    monitor.as_ref().map(Arc::clone),
                    orchestrator.clone(),
                    user_id.clone(),
                    session_id.clone(),
                    run_id.clone(),
                    answer.clone(),
                    error.clone(),
                    request_config_overrides.clone(),
                    parent_dispatch,
                )
                .await;
            }
        }
        if matches!(cleanup, SessionCleanup::Delete) {
            cleanup_session(
                &storage,
                &workspace,
                monitor.as_ref(),
                &user_id,
                &session_id,
            );
        }

        let _ = tx.send(SessionRunOutcome {
            status,
            answer,
            error,
            elapsed_s: elapsed,
        });
    });
    Ok(rx)
}

pub(crate) fn cleanup_session(
    storage: &Arc<dyn StorageBackend>,
    workspace: &WorkspaceManager,
    monitor: Option<&Arc<MonitorState>>,
    user_id: &str,
    session_id: &str,
) {
    workspace.purge_session_data(user_id, session_id);
    let _ = storage.delete_chat_session(user_id, session_id);
    if let Some(monitor) = monitor {
        let _ = monitor.purge_session(session_id);
    }
}

pub(crate) async fn load_session_messages(
    workspace: Arc<WorkspaceManager>,
    user_id: String,
    session_id: String,
    limit: i64,
    include_tools: bool,
) -> Vec<Value> {
    blocking::run_fs("tools.session_run.load_messages", move || {
        let messages = if include_tools {
            let history = workspace
                .load_history(&user_id, &session_id, limit)
                .unwrap_or_default();
            history
                .into_iter()
                .filter(|item| item.get("role").and_then(Value::as_str) != Some("system"))
                .collect()
        } else {
            let manager = HistoryManager;
            manager.load_history_messages(&workspace, &user_id, &session_id, limit)
        };
        Ok(messages)
    })
    .await
    .unwrap_or_default()
}
