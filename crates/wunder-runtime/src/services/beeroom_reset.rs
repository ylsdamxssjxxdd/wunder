use crate::services::orchestration_context::{
    build_closed_history_record, clear_hive_state, clear_member_bindings, clear_session_context,
    latest_formal_round_index, list_member_bindings, load_history_record, load_hive_state,
    load_round_state, persist_history_record,
};
use crate::services::swarm::beeroom::{
    get_mother_agent_id, resolve_or_create_hive_mother_session, resolve_preferred_mother_agent_id,
};
use crate::state::AppState;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct ResetBeeroomMemberThread {
    pub agent_id: String,
    pub agent_name: String,
    pub role: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetBeeroomSummary {
    pub group_id: String,
    pub cancelled_sessions: usize,
    pub cancelled_tasks: usize,
    pub cancelled_team_runs: usize,
    pub removed_team_runs: i64,
    pub removed_chat_messages: i64,
    pub member_threads: Vec<ResetBeeroomMemberThread>,
}

pub async fn reset_beeroom_group(
    state: &Arc<AppState>,
    user_id: &str,
    group_id: &str,
) -> Result<ResetBeeroomSummary> {
    let mut agents = state
        .user_store
        .list_user_agents_by_hive_with_default(user_id, group_id)?;
    let mother_agent_id = match get_mother_agent_id(state.storage.as_ref(), user_id, group_id)? {
        Some(agent_id) => agent_id,
        None => resolve_preferred_mother_agent_id(
            state.storage.as_ref(),
            user_id,
            group_id,
            agents.first().map(|agent| agent.agent_id.as_str()),
        )?
        .unwrap_or_default(),
    };

    let mut session_ids = HashSet::new();
    for agent in &agents {
        let session_agent_id = if agent.agent_id.trim().eq_ignore_ascii_case("__default__") {
            ""
        } else {
            agent.agent_id.as_str()
        };
        let (sessions, _) =
            state
                .user_store
                .list_chat_sessions(user_id, Some(session_agent_id), None, 0, 4096)?;
        session_ids.extend(sessions.into_iter().map(|item| item.session_id));
    }

    let mut cancelled_sessions = 0usize;
    for session_id in &session_ids {
        let settlement = state
            .kernel
            .thread_runtime
            .cancel_session_activity(user_id, session_id, "beeroom_reset")
            .await?;
        if settlement.monitor_cancelled
            || settlement.queued_tasks_cancelled > 0
            || settlement.running_tasks_marked_cancelled > 0
        {
            cancelled_sessions += 1;
        }
    }
    let mut cancelled_tasks = 0usize;
    let mut cancelled_task_ids = HashSet::new();
    for session_id in &session_ids {
        let thread_id = format!("thread_{session_id}");
        for task in state
            .kernel
            .thread_runtime
            .list_thread_tasks(&thread_id, None, 256)
            .await?
        {
            if !matches!(task.status.trim(), "pending" | "running" | "retry")
                || !cancelled_task_ids.insert(task.task_id.clone())
            {
                continue;
            }
            state.kernel.thread_runtime.cancel_task(&task.task_id)?;
            cancelled_tasks += 1;
        }
    }

    let (team_runs, _) = state
        .user_store
        .list_team_runs(user_id, Some(group_id), None, 0, 4096)?;
    let mut cancelled_team_runs = 0usize;
    for run in &team_runs {
        if matches!(
            run.status.trim().to_ascii_lowercase().as_str(),
            "success" | "failed" | "timeout" | "cancelled"
        ) {
            continue;
        }
        state.kernel.mission_runtime.cancel(&run.team_run_id).await;
        cancelled_team_runs += 1;
    }

    close_active_orchestration(state, user_id, group_id)?;
    let removed_team_runs = state
        .user_store
        .delete_team_runs_by_hive(user_id, group_id)?;
    let removed_chat_messages = state
        .user_store
        .delete_beeroom_chat_messages(user_id, group_id)?;

    let mut member_threads = Vec::with_capacity(agents.len());
    for agent in &agents {
        let runtime_agent_id = if agent.agent_id.trim().eq_ignore_ascii_case("__default__") {
            ""
        } else {
            agent.agent_id.as_str()
        };
        let session_id = state
            .kernel
            .thread_runtime
            .create_fresh_main_session_id(user_id, runtime_agent_id, "beeroom_reset")
            .await?;
        clear_session_context(state.storage.as_ref(), user_id, &session_id)?;
        member_threads.push(ResetBeeroomMemberThread {
            agent_id: agent.agent_id.clone(),
            agent_name: agent.name.clone(),
            role: if agent.agent_id == mother_agent_id {
                "mother".to_string()
            } else {
                "worker".to_string()
            },
            session_id,
        });
    }
    if let Some(mother) = agents
        .iter_mut()
        .find(|agent| agent.agent_id == mother_agent_id)
    {
        let _ = resolve_or_create_hive_mother_session(
            state.storage.as_ref(),
            user_id,
            group_id,
            mother,
        )?;
    }

    state.kernel.thread_runtime.wake().await;
    state.kernel.mission_runtime.wake().await;
    Ok(ResetBeeroomSummary {
        group_id: group_id.to_string(),
        cancelled_sessions,
        cancelled_tasks,
        cancelled_team_runs,
        removed_team_runs,
        removed_chat_messages,
        member_threads,
    })
}

fn close_active_orchestration(state: &AppState, user_id: &str, group_id: &str) -> Result<()> {
    let Some(active) = load_hive_state(state.storage.as_ref(), user_id, group_id) else {
        return Ok(());
    };
    let bindings = list_member_bindings(state.storage.as_ref(), &active.orchestration_id)?;
    for binding in &bindings {
        clear_session_context(state.storage.as_ref(), user_id, &binding.session_id)?;
    }
    let round_state = load_round_state(state.storage.as_ref(), user_id, &active.orchestration_id);
    let latest_round = latest_formal_round_index(round_state.as_ref());
    let existing = load_history_record(
        state.storage.as_ref(),
        user_id,
        group_id,
        &active.orchestration_id,
    );
    let closed = build_closed_history_record(&active, existing.as_ref(), latest_round, now_ts());
    persist_history_record(state.storage.as_ref(), user_id, &closed)?;
    clear_member_bindings(state.storage.as_ref(), &active.orchestration_id)?;
    clear_hive_state(state.storage.as_ref(), user_id, group_id)?;
    Ok(())
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
