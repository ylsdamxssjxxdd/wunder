use super::context::ToolContext;
use crate::services::swarm::events::{
    TEAM_ERROR, TEAM_FINISH, TEAM_START, TEAM_TASK_DISPATCH, TEAM_TASK_RESULT, TEAM_TASK_UPDATE,
};
use crate::storage::{TeamRunRecord, TeamTaskRecord};
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};

pub(crate) fn emit_swarm_run_started(context: &ToolContext<'_>, run: &TeamRunRecord) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_START,
        json!({
            "team_run_id": run.team_run_id,
            "hive_id": run.hive_id,
            "status": run.status,
            "task_total": run.task_total,
            "strategy": run.strategy,
            "updated_time": run.updated_time,
        }),
    );
}

pub(crate) fn emit_swarm_task_dispatched(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    task: &TeamTaskRecord,
) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_TASK_DISPATCH,
        json!({
            "team_run_id": task.team_run_id,
            "task_id": task.task_id,
            "hive_id": task.hive_id,
            "agent_id": task.agent_id,
            "status": task.status,
            "priority": task.priority,
            "target_session_id": task.target_session_id,
            "spawned_session_id": task.spawned_session_id,
            "updated_time": task.updated_time,
        }),
    );
}

pub(crate) fn emit_swarm_task_updated(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    task: &TeamTaskRecord,
) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_TASK_UPDATE,
        json!({
            "team_run_id": task.team_run_id,
            "task_id": task.task_id,
            "hive_id": task.hive_id,
            "agent_id": task.agent_id,
            "session_run_id": task.session_run_id,
            "status": task.status,
            "retry_count": task.retry_count,
            "started_time": task.started_time,
            "finished_time": task.finished_time,
            "elapsed_s": task.elapsed_s,
            "result_summary": task.result_summary,
            "error": task.error,
            "updated_time": task.updated_time,
        }),
    );

    if !is_terminal_task_status(&task.status) {
        return;
    }

    emit_swarm_team_event(
        context,
        run,
        TEAM_TASK_RESULT,
        json!({
            "team_run_id": task.team_run_id,
            "task_id": task.task_id,
            "hive_id": task.hive_id,
            "agent_id": task.agent_id,
            "session_run_id": task.session_run_id,
            "status": task.status,
            "retry_count": task.retry_count,
            "started_time": task.started_time,
            "finished_time": task.finished_time,
            "elapsed_s": task.elapsed_s,
            "result_summary": task.result_summary,
            "error": task.error,
            "updated_time": task.updated_time,
        }),
    );
}

pub(crate) fn sync_swarm_run_summary(
    context: &ToolContext<'_>,
    run: &mut TeamRunRecord,
    tasks: &[TeamTaskRecord],
) -> Result<(bool, bool)> {
    // Keep run-level counters/status aligned with task snapshots from agent_swarm paths.
    let mut success_total = 0i64;
    let mut failed_total = 0i64;
    let mut active_total = 0usize;
    let mut all_cancelled = !tasks.is_empty();
    let mut latest_updated = run.updated_time;
    let mut earliest_started = run.started_time;
    let mut latest_finished = run.finished_time;

    for task in tasks {
        let normalized = normalize_status(&task.status);
        if is_success_task_status(&normalized) {
            success_total += 1;
            all_cancelled = false;
        } else if is_failed_task_status(&normalized) {
            failed_total += 1;
            if normalized != "cancelled" {
                all_cancelled = false;
            }
        } else {
            active_total += 1;
            all_cancelled = false;
        }
        latest_updated = latest_updated.max(task.updated_time);
        if let Some(started) = task.started_time {
            earliest_started = Some(
                earliest_started
                    .map(|current| current.min(started))
                    .unwrap_or(started),
            );
        }
        if let Some(finished) = task.finished_time {
            latest_finished = Some(
                latest_finished
                    .map(|current| current.max(finished))
                    .unwrap_or(finished),
            );
        }
    }

    run.task_total = tasks.len() as i64;
    run.task_success = success_total;
    run.task_failed = failed_total;
    run.started_time = earliest_started;
    run.updated_time = latest_updated;

    let terminal = !tasks.is_empty() && active_total == 0;
    let failed = terminal && failed_total > 0;

    if !terminal {
        run.status = if tasks.is_empty() {
            "queued".to_string()
        } else {
            "running".to_string()
        };
        run.finished_time = None;
        run.elapsed_s = None;
        run.error = None;
    } else {
        let finished_at = latest_finished.unwrap_or_else(now_ts);
        run.finished_time = Some(finished_at);
        run.elapsed_s = run
            .started_time
            .map(|started| (finished_at - started).max(0.0));

        if failed {
            run.status = if all_cancelled {
                "cancelled".to_string()
            } else {
                "failed".to_string()
            };
            run.error = tasks
                .iter()
                .filter_map(|task| normalize_optional(task.error.as_deref()))
                .next()
                .or_else(|| all_cancelled.then_some("cancelled".to_string()));
        } else {
            run.status = "completed".to_string();
            run.error = None;
            if normalize_optional(run.summary.as_deref()).is_none() {
                run.summary = tasks
                    .iter()
                    .filter_map(|task| normalize_optional(task.result_summary.as_deref()))
                    .next();
            }
        }
    }

    context.storage.upsert_team_run(run)?;
    Ok((terminal, failed))
}

pub(crate) fn emit_swarm_run_terminal(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    failed: bool,
) {
    if failed {
        emit_swarm_team_event(
            context,
            run,
            TEAM_ERROR,
            json!({
                "team_run_id": run.team_run_id,
                "hive_id": run.hive_id,
                "status": run.status,
                "task_total": run.task_total,
                "task_success": run.task_success,
                "task_failed": run.task_failed,
                "summary": run.summary,
                "error": run.error,
                "updated_time": run.updated_time,
            }),
        );
    }
    emit_swarm_team_event(
        context,
        run,
        TEAM_FINISH,
        json!({
            "team_run_id": run.team_run_id,
            "hive_id": run.hive_id,
            "status": run.status,
            "task_total": run.task_total,
            "task_success": run.task_success,
            "task_failed": run.task_failed,
            "started_time": run.started_time,
            "finished_time": run.finished_time,
            "elapsed_s": run.elapsed_s,
            "summary": run.summary,
            "error": run.error,
            "updated_time": run.updated_time,
        }),
    );
}

pub(crate) fn emit_swarm_team_event(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    event_type: &str,
    payload: Value,
) {
    let cleaned_event = event_type.trim();
    if cleaned_event.is_empty() {
        return;
    }

    let session_id = run.parent_session_id.trim();
    if !session_id.is_empty() {
        if let Some(monitor) = context.monitor.as_ref() {
            monitor.record_event(session_id, cleaned_event, &payload);
        }
    }

    let cleaned_user = run.user_id.trim();
    let cleaned_hive = run.hive_id.trim();
    if cleaned_user.is_empty() || cleaned_hive.is_empty() {
        return;
    }

    let mut realtime_payload = payload;
    if let Value::Object(ref mut map) = realtime_payload {
        map.entry("team_run_id".to_string())
            .or_insert_with(|| Value::String(run.team_run_id.clone()));
        map.entry("hive_id".to_string())
            .or_insert_with(|| Value::String(run.hive_id.clone()));
        map.entry("status".to_string())
            .or_insert_with(|| Value::String(run.status.clone()));
        map.entry("updated_time".to_string())
            .or_insert_with(|| json!(run.updated_time));
    }

    let Some(realtime) = context.beeroom_realtime.as_ref().cloned() else {
        return;
    };
    let user_id = cleaned_user.to_string();
    let hive_id = cleaned_hive.to_string();
    let event_name = cleaned_event.to_string();
    tokio::spawn(async move {
        realtime
            .publish_group_event(&user_id, &hive_id, &event_name, realtime_payload)
            .await;
    });
}

fn normalize_status(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn is_success_task_status(status: &str) -> bool {
    matches!(status, "success" | "completed")
}

fn is_failed_task_status(status: &str) -> bool {
    matches!(status, "failed" | "error" | "timeout" | "cancelled")
}

fn is_terminal_task_status(status: &str) -> bool {
    let normalized = normalize_status(status);
    is_success_task_status(&normalized) || is_failed_task_status(&normalized)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
