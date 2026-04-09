use crate::state::AppState;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct ResetWorkStateSession {
    pub agent_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetWorkStateSummary {
    pub cancelled_sessions: usize,
    pub cancelled_tasks: usize,
    pub cancelled_team_runs: usize,
    pub cleared_workspaces: usize,
    pub removed_workspace_entries: u64,
    pub fresh_main_sessions: Vec<ResetWorkStateSession>,
}

pub async fn reset_user_work_state(
    state: &Arc<AppState>,
    user_id: &str,
    reason: &str,
) -> Result<ResetWorkStateSummary> {
    let cleaned_user_id = user_id.trim();
    if cleaned_user_id.is_empty() {
        return Err(anyhow!("user_id is required"));
    }

    let agents = state.user_store.list_user_agents(cleaned_user_id)?;

    let mut session_ids = HashSet::new();
    for session in state.monitor.list_sessions(true) {
        let session_user_id = session
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_user_id != cleaned_user_id {
            continue;
        }
        let session_id = session
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }
    for session in state.monitor.load_records_by_user(
        cleaned_user_id,
        Some(&[
            crate::monitor::MonitorState::STATUS_WAITING,
            crate::monitor::MonitorState::STATUS_RUNNING,
            crate::monitor::MonitorState::STATUS_CANCELLING,
        ]),
        None,
        4096,
    ) {
        let session_id = session
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }
    for lock in state
        .user_store
        .list_session_locks_by_user(cleaned_user_id)?
    {
        let session_id = lock.session_id.trim();
        if !session_id.is_empty() {
            session_ids.insert(session_id.to_string());
        }
    }

    let cancelled_sessions = session_ids
        .iter()
        .filter(|session_id| state.monitor.cancel(session_id))
        .count();

    let (all_sessions, _) =
        state
            .user_store
            .list_chat_sessions(cleaned_user_id, None, None, 0, 4096)?;
    let mut cancelled_tasks = 0usize;
    let mut cancelled_task_ids = HashSet::new();
    for session in &all_sessions {
        let thread_id = format!("thread_{}", session.session_id);
        let tasks = state
            .kernel
            .thread_runtime
            .list_thread_tasks(&thread_id, None, 256)
            .await?;
        for task in tasks {
            if !is_resettable_agent_task_status(&task.status) {
                continue;
            }
            if !cancelled_task_ids.insert(task.task_id.clone()) {
                continue;
            }
            state.kernel.thread_runtime.cancel_task(&task.task_id)?;
            cancelled_tasks = cancelled_tasks.saturating_add(1);
        }
    }

    let (team_runs, _) = state
        .user_store
        .list_team_runs(cleaned_user_id, None, None, 0, 4096)?;
    let mut cancelled_team_runs = 0usize;
    for run in team_runs {
        if is_terminal_team_run_status(&run.status) {
            continue;
        }
        state.kernel.mission_runtime.cancel(&run.team_run_id).await;
        cancelled_team_runs = cancelled_team_runs.saturating_add(1);
    }

    let mut workspace_scopes = HashSet::new();
    workspace_scopes.insert(state.workspace.scoped_user_id_by_container(
        cleaned_user_id,
        state.user_store.default_sandbox_container_id(),
    ));
    for variant in state
        .workspace
        .scoped_user_id_variants(cleaned_user_id, None)
    {
        workspace_scopes.insert(variant);
    }
    for agent in &agents {
        workspace_scopes.insert(
            state
                .workspace
                .scoped_user_id_by_container(cleaned_user_id, agent.sandbox_container_id),
        );
        for variant in state
            .workspace
            .scoped_user_id_variants(cleaned_user_id, Some(&agent.agent_id))
        {
            workspace_scopes.insert(variant);
        }
    }

    let mut removed_workspace_entries = 0u64;
    for scope in &workspace_scopes {
        removed_workspace_entries = removed_workspace_entries
            .saturating_add(state.workspace.clear_work_state_contents(scope)?);
    }

    let mut fresh_main_sessions = Vec::with_capacity(agents.len().saturating_add(1));
    let default_session_id = state
        .kernel
        .thread_runtime
        .create_fresh_main_session_id(cleaned_user_id, "", reason)
        .await?;
    fresh_main_sessions.push(ResetWorkStateSession {
        agent_id: String::new(),
        session_id: default_session_id,
    });
    for agent in &agents {
        let session_id = state
            .kernel
            .thread_runtime
            .create_fresh_main_session_id(cleaned_user_id, &agent.agent_id, reason)
            .await?;
        fresh_main_sessions.push(ResetWorkStateSession {
            agent_id: agent.agent_id.clone(),
            session_id,
        });
    }

    state.kernel.thread_runtime.wake().await;
    state.kernel.mission_runtime.wake().await;

    Ok(ResetWorkStateSummary {
        cancelled_sessions,
        cancelled_tasks,
        cancelled_team_runs,
        cleared_workspaces: workspace_scopes.len(),
        removed_workspace_entries,
        fresh_main_sessions,
    })
}

fn is_resettable_agent_task_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pending" | "running" | "retry"
    )
}

fn is_terminal_team_run_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "success" | "failed" | "timeout" | "cancelled"
    )
}
