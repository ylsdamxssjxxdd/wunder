use crate::services::orchestration_context::{
    load_hive_state, load_round_state, load_session_context, OrchestrationSessionContext,
};
use crate::services::stream_events::StreamEventService;
use crate::services::swarm::events::{TEAM_FINISH, TEAM_TASK_UPDATE};
use crate::state::AppState;
use crate::storage::{StorageBackend, TeamRunRecord, TeamTaskRecord};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use tracing::warn;

fn parse_chat_timestamp(value: &Value) -> f64 {
    if let Some(numeric) = value.as_f64() {
        return numeric.max(0.0);
    }
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        if let Ok(numeric) = text.parse::<f64>() {
            return numeric.max(0.0);
        }
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
            return (parsed.timestamp_millis() as f64 / 1000.0).max(0.0);
        }
    }
    0.0
}

fn chat_message_timestamp(record: &Value) -> f64 {
    record
        .get("timestamp")
        .map(parse_chat_timestamp)
        .or_else(|| record.get("created_at").map(parse_chat_timestamp))
        .unwrap_or(0.0)
}

fn chat_message_role(record: &Value) -> Option<&str> {
    record.get("role").and_then(Value::as_str).map(str::trim)
}

pub fn orchestration_dispatch_guard_context(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
    worker_session_id: &str,
) -> Option<(OrchestrationSessionContext, f64)> {
    let parent_context = load_session_context(storage, user_id, parent_session_id)?;
    let worker_context = load_session_context(storage, user_id, worker_session_id)?;
    if parent_context.mode != worker_context.mode
        || parent_context.mode.trim() != crate::services::orchestration_context::ORCHESTRATION_MODE
        || parent_context.run_id.trim() != worker_context.run_id.trim()
        || parent_context.group_id.trim() != worker_context.group_id.trim()
        || parent_context.round_index <= 0
        || worker_context.round_index != parent_context.round_index
    {
        return None;
    }
    let hive_state = load_hive_state(storage, user_id, &parent_context.group_id)?;
    let round_state = load_round_state(storage, user_id, &hive_state.orchestration_id)?;
    let round_record = round_state
        .rounds
        .iter()
        .find(|round| round.index == parent_context.round_index)?;
    Some((parent_context, round_record.created_at))
}

pub fn worker_already_dispatched_in_round(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
    worker_session_id: &str,
) -> Result<bool> {
    let Some((_, round_created_at)) = orchestration_dispatch_guard_context(
        storage,
        user_id,
        parent_session_id,
        worker_session_id,
    ) else {
        return Ok(false);
    };
    let history = storage.load_chat_history(user_id.trim(), worker_session_id.trim(), None)?;
    Ok(history.into_iter().any(|item| {
        matches!(chat_message_role(&item), Some("user"))
            && chat_message_timestamp(&item) >= round_created_at
    }))
}

fn is_active_team_run_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "queued" | "running" | "merging"
    )
}

fn is_terminal_team_task_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "success" | "failed" | "error" | "timeout" | "cancelled"
    )
}

pub async fn cancel_team_run_record(
    state: &AppState,
    user_id: &str,
    run: &mut TeamRunRecord,
    tasks: Vec<TeamTaskRecord>,
) -> Result<bool> {
    if run.user_id.trim() != user_id.trim() || !is_active_team_run_status(&run.status) {
        return Ok(false);
    }
    let now = now_ts();
    run.status = "cancelled".to_string();
    run.finished_time = Some(now);
    run.elapsed_s = run.started_time.map(|start| (now - start).max(0.0));
    run.updated_time = now;
    state.user_store.upsert_team_run(run)?;

    state.kernel.mission_runtime.cancel(&run.team_run_id).await;

    for task in &tasks {
        if let Some(session_id) = task
            .target_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let _ = state.monitor.cancel(session_id);
            let thread_id = format!("thread_{session_id}");
            let agent_tasks = state
                .kernel
                .thread_runtime
                .list_thread_tasks(&thread_id, None, 256)
                .await?;
            for agent_task in agent_tasks {
                let status = agent_task.status.trim().to_ascii_lowercase();
                if !matches!(status.as_str(), "pending" | "running" | "retry") {
                    continue;
                }
                state
                    .kernel
                    .thread_runtime
                    .cancel_task(&agent_task.task_id)?;
            }
        }
    }

    for mut task in tasks {
        if is_terminal_team_task_status(&task.status) {
            continue;
        }
        task.status = "cancelled".to_string();
        task.updated_time = now;
        task.finished_time = Some(now);
        task.elapsed_s = task.started_time.map(|start| (now - start).max(0.0));
        state.user_store.upsert_team_task(&task)?;
        emit_team_event(
            state,
            user_id,
            &run.parent_session_id,
            &task.hive_id,
            TEAM_TASK_UPDATE,
            serde_json::json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": task.hive_id,
                "agent_id": task.agent_id,
                "status": task.status,
            }),
        );
    }

    emit_team_event(
        state,
        user_id,
        &run.parent_session_id,
        &run.hive_id,
        TEAM_FINISH,
        serde_json::json!({
            "team_run_id": run.team_run_id,
            "hive_id": run.hive_id,
            "status": run.status,
            "updated_time": run.updated_time,
        }),
    );

    Ok(true)
}

pub async fn cancel_active_team_runs_for_parent_session(
    state: &AppState,
    user_id: &str,
    parent_session_id: &str,
) -> Result<usize> {
    let (runs, _) = state.user_store.list_team_runs(
        user_id.trim(),
        None,
        Some(parent_session_id.trim()),
        0,
        256,
    )?;
    let mut cancelled = 0usize;
    for mut run in runs {
        if !is_active_team_run_status(&run.status) {
            continue;
        }
        let tasks = state.user_store.list_team_tasks(&run.team_run_id)?;
        if cancel_team_run_record(state, user_id, &mut run, tasks).await? {
            cancelled += 1;
        }
    }
    Ok(cancelled)
}

fn emit_team_event(
    state: &AppState,
    user_id: &str,
    session_id: &str,
    hive_id: &str,
    event_type: &str,
    payload: Value,
) {
    let cleaned_session_id = session_id.trim();
    let cleaned_user = user_id.trim();
    let cleaned_event = event_type.trim();
    if cleaned_session_id.is_empty() || cleaned_event.is_empty() {
        return;
    }
    state
        .monitor
        .record_event(cleaned_session_id, cleaned_event, &payload);

    if !cleaned_user.is_empty() {
        let stream_events = StreamEventService::new(state.storage.clone());
        let session_id = cleaned_session_id.to_string();
        let user_id = cleaned_user.to_string();
        let event_name = cleaned_event.to_string();
        let stream_payload = serde_json::json!({
            "event": event_name.clone(),
            "data": payload.clone(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        tokio::spawn(async move {
            if let Err(err) = stream_events
                .append_event(&session_id, &user_id, stream_payload)
                .await
            {
                warn!(
                    "append team stream event failed: session_id={}, event_type={}, error={err}",
                    session_id, event_name
                );
            }
        });
    }

    let cleaned_hive = hive_id.trim();
    if cleaned_user.is_empty() || cleaned_hive.is_empty() || cleaned_event.is_empty() {
        return;
    }
    let realtime = state.projection.beeroom.clone();
    let user_id = cleaned_user.to_string();
    let hive_id = cleaned_hive.to_string();
    let event_name = cleaned_event.to_string();
    tokio::spawn(async move {
        realtime
            .publish_group_event(&user_id, &hive_id, &event_name, payload)
            .await;
    });
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::orchestration_context::{
        persist_hive_state, persist_round_state, persist_session_context, OrchestrationHiveState,
        OrchestrationRoundRecord, OrchestrationRoundState, ORCHESTRATION_MODE,
    };
    use crate::storage::{ChatSessionRecord, SqliteStorage, StorageBackend};
    use serde_json::json;
    use tempfile::tempdir;

    fn session(session_id: &str, agent_id: &str) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: session_id.to_string(),
            user_id: "alice".to_string(),
            title: session_id.to_string(),
            status: "active".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
            last_message_at: 1.0,
            agent_id: Some(agent_id.to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    #[test]
    fn worker_already_dispatched_in_round_only_counts_current_round_user_messages() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.sqlite3");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("init");
        storage
            .upsert_chat_session(&session("sess_mother", "agent_mother"))
            .expect("mother session");
        storage
            .upsert_chat_session(&session("sess_worker", "agent_worker"))
            .expect("worker session");
        persist_hive_state(
            &storage,
            "alice",
            &OrchestrationHiveState {
                orchestration_id: "orch_a".to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                mother_agent_id: "agent_mother".to_string(),
                mother_agent_name: "mother".to_string(),
                mother_session_id: "sess_mother".to_string(),
                active: true,
                entered_at: 1.0,
                updated_at: 1.0,
                mother_primer_injected: true,
            },
        )
        .expect("persist hive");
        persist_round_state(
            &storage,
            "alice",
            &OrchestrationRoundState {
                orchestration_id: "orch_a".to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                rounds: vec![OrchestrationRoundRecord {
                    id: "round_01".to_string(),
                    index: 1,
                    situation: String::new(),
                    user_message: "hello".to_string(),
                    created_at: 10.0,
                    finalized_at: 0.0,
                }],
                suppressed_message_ranges: Vec::new(),
                updated_at: 10.0,
            },
        )
        .expect("persist round state");
        persist_session_context(
            &storage,
            "alice",
            "sess_mother",
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                role: "mother".to_string(),
                round_index: 1,
                mother_agent_id: "agent_mother".to_string(),
            },
        )
        .expect("persist mother context");
        persist_session_context(
            &storage,
            "alice",
            "sess_worker",
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                role: "worker".to_string(),
                round_index: 1,
                mother_agent_id: "agent_mother".to_string(),
            },
        )
        .expect("persist worker context");

        storage
            .append_chat(
                "alice",
                &json!({
                    "session_id": "sess_worker",
                    "role": "user",
                    "content": "old round",
                    "timestamp": 9.0
                }),
            )
            .expect("append old user");
        assert!(!worker_already_dispatched_in_round(
            &storage,
            "alice",
            "sess_mother",
            "sess_worker"
        )
        .expect("guard result"));

        storage
            .append_chat(
                "alice",
                &json!({
                    "session_id": "sess_worker",
                    "role": "user",
                    "content": "current round",
                    "timestamp": 10.5
                }),
            )
            .expect("append current round user");
        assert!(worker_already_dispatched_in_round(
            &storage,
            "alice",
            "sess_mother",
            "sess_worker"
        )
        .expect("guard result"));
    }

    #[test]
    fn worker_already_dispatched_in_round_accepts_iso_created_at() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.sqlite3");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("init");
        storage
            .upsert_chat_session(&session("sess_mother", "agent_mother"))
            .expect("mother session");
        storage
            .upsert_chat_session(&session("sess_worker", "agent_worker"))
            .expect("worker session");
        persist_hive_state(
            &storage,
            "alice",
            &OrchestrationHiveState {
                orchestration_id: "orch_a".to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                mother_agent_id: "agent_mother".to_string(),
                mother_agent_name: "mother".to_string(),
                mother_session_id: "sess_mother".to_string(),
                active: true,
                entered_at: 1.0,
                updated_at: 1.0,
                mother_primer_injected: true,
            },
        )
        .expect("persist hive");
        persist_round_state(
            &storage,
            "alice",
            &OrchestrationRoundState {
                orchestration_id: "orch_a".to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                rounds: vec![OrchestrationRoundRecord {
                    id: "round_01".to_string(),
                    index: 1,
                    situation: String::new(),
                    user_message: "hello".to_string(),
                    created_at: 1_776_853_975.0,
                    finalized_at: 0.0,
                }],
                suppressed_message_ranges: Vec::new(),
                updated_at: 1_776_853_975.0,
            },
        )
        .expect("persist round state");
        persist_session_context(
            &storage,
            "alice",
            "sess_mother",
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                role: "mother".to_string(),
                round_index: 1,
                mother_agent_id: "agent_mother".to_string(),
            },
        )
        .expect("persist mother context");
        persist_session_context(
            &storage,
            "alice",
            "sess_worker",
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: "demo".to_string(),
                group_id: "hive_demo".to_string(),
                role: "worker".to_string(),
                round_index: 1,
                mother_agent_id: "agent_mother".to_string(),
            },
        )
        .expect("persist worker context");

        storage
            .append_chat(
                "alice",
                &json!({
                    "session_id": "sess_worker",
                    "role": "user",
                    "content": "current round iso",
                    "created_at": "2026-04-22T10:32:56Z"
                }),
            )
            .expect("append current round user");

        assert!(worker_already_dispatched_in_round(
            &storage,
            "alice",
            "sess_mother",
            "sess_worker"
        )
        .expect("guard result"));
    }
}
