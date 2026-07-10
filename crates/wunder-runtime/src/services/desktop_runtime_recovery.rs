#![allow(dead_code)]

use crate::monitor::MonitorState;
use crate::services::runtime::thread::ThreadRuntime;
use crate::state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct DesktopRuntimeRecoverySummary {
    pub cancelled_monitor_sessions: usize,
    pub cancelled_session_locks: usize,
    pub cancelled_agent_tasks: usize,
    pub reset_agent_threads: usize,
}

pub async fn recover_desktop_runtime_state(
    state: &AppState,
    user_id: &str,
) -> Result<DesktopRuntimeRecoverySummary> {
    let cleaned_user_id = user_id.trim();
    if cleaned_user_id.is_empty() {
        return Ok(DesktopRuntimeRecoverySummary::default());
    }

    let mut session_ids = HashSet::new();
    for lock in state
        .user_store
        .list_session_locks_by_user(cleaned_user_id)?
    {
        let session_id = lock.session_id.trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }
    for record in state.monitor.load_records_by_user(
        cleaned_user_id,
        Some(&[
            MonitorState::STATUS_QUEUED,
            MonitorState::STATUS_WAITING,
            MonitorState::STATUS_RUNNING,
            MonitorState::STATUS_CANCELLING,
        ]),
        None,
        4096,
    ) {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }
    for session in state
        .user_store
        .list_chat_sessions(cleaned_user_id, None, None, 0, 4096)?
        .0
    {
        let session_id = session.session_id.trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }

    let cancelled_monitor_sessions = cancel_desktop_monitor_sessions(state, &session_ids)?;

    let cancelled_session_locks = state
        .storage
        .delete_session_locks_by_user(cleaned_user_id)
        .unwrap_or(0)
        .max(0) as usize;

    let thread_runtime = state.kernel.thread_runtime.clone();
    let (cancelled_agent_tasks, reset_agent_threads) =
        cancel_desktop_agent_tasks(state, cleaned_user_id, &thread_runtime).await?;

    thread_runtime.wake().await;
    state.kernel.mission_runtime.wake().await;

    Ok(DesktopRuntimeRecoverySummary {
        cancelled_monitor_sessions,
        cancelled_session_locks,
        cancelled_agent_tasks,
        reset_agent_threads,
    })
}

async fn cancel_desktop_agent_tasks(
    state: &AppState,
    user_id: &str,
    thread_runtime: &Arc<ThreadRuntime>,
) -> Result<(usize, usize)> {
    let (sessions, _) = state
        .user_store
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut task_ids = HashSet::new();
    let mut reset_thread_ids = HashSet::new();
    for session in sessions {
        let thread_id = format!("thread_{}", session.session_id.trim());
        if thread_id.trim().is_empty() {
            continue;
        }
        let tasks = thread_runtime
            .list_thread_tasks(&thread_id, None, 256)
            .await?;
        for task in tasks {
            if !is_resettable_agent_task_status(&task.status) {
                continue;
            }
            if !task_ids.insert(task.task_id.clone()) {
                continue;
            }
            thread_runtime.cancel_task(&task.task_id)?;
            reset_thread_ids.insert(thread_id.clone());
        }
    }
    Ok((task_ids.len(), reset_thread_ids.len()))
}

fn cancel_desktop_monitor_sessions(
    state: &AppState,
    session_ids: &HashSet<String>,
) -> Result<usize> {
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let mut cancelled_count = 0usize;
    for session_id in session_ids {
        let Some(mut record) = state.storage.get_monitor_record(session_id)? else {
            continue;
        };
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !matches!(status.as_str(), "running" | "waiting" | "cancelling") {
            continue;
        }
        if let Value::Object(ref mut map) = record {
            map.insert(
                "status".to_string(),
                Value::String(MonitorState::STATUS_CANCELLED.to_string()),
            );
            map.insert("stage".to_string(), Value::String("cancelled".to_string()));
            map.insert(
                "summary".to_string(),
                Value::String(crate::i18n::t("monitor.summary.cancelled")),
            );
            map.insert("updated_time".to_string(), json!(now));
            map.insert("ended_time".to_string(), json!(now));
            map.insert("cancel_requested".to_string(), Value::Bool(true));
            map.insert(
                "cancel_source".to_string(),
                Value::String("desktop_restart_recovery".to_string()),
            );
            state.storage.upsert_monitor_record(&record)?;
            cancelled_count = cancelled_count.saturating_add(1);
        }
    }
    Ok(cancelled_count)
}

fn is_resettable_agent_task_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pending" | "running" | "retry"
    )
}
