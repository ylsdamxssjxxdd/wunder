use super::{
    build_model_tool_success, build_model_tool_success_with_hint, compact_swarm_run_result_preview,
    current_agent_id, dedupe_non_empty_strings, is_swarm_run_failed, is_swarm_run_terminal,
    normalize_swarm_poll_interval, now_ts, ToolContext,
};
use crate::i18n;
use crate::services::swarm::beeroom::claim_mother_agent as claim_swarm_mother_agent;
use crate::storage::{TeamRunRecord, TeamTaskRecord};
use anyhow::Result;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct SwarmRunSnapshot {
    status: String,
    terminal: bool,
    failed: bool,
    payload: Value,
}

pub(crate) fn claim_swarm_mother_for_context(
    context: &ToolContext<'_>,
    user_id: &str,
    hive_id: &str,
) -> Result<Option<String>> {
    let Some(agent_id) = current_agent_id(context) else {
        return Ok(None);
    };
    let mother_agent_id =
        claim_swarm_mother_agent(context.storage.as_ref(), user_id, hive_id, &agent_id)?;
    Ok(Some(mother_agent_id))
}

pub(crate) fn create_swarm_team_run_record(
    context: &ToolContext<'_>,
    user_id: &str,
    hive_id: &str,
    mother_agent_id: Option<String>,
    team_run_id_override: Option<&str>,
    strategy: &str,
    task_total: usize,
) -> TeamRunRecord {
    let now = now_ts();
    let team_run_id = team_run_id_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("team_{}", Uuid::new_v4().simple()));
    TeamRunRecord {
        team_run_id,
        user_id: user_id.to_string(),
        hive_id: hive_id.to_string(),
        parent_session_id: context.session_id.to_string(),
        parent_agent_id: current_agent_id(context),
        mother_agent_id,
        strategy: strategy.to_string(),
        status: "queued".to_string(),
        task_total: task_total as i64,
        task_success: 0,
        task_failed: 0,
        context_tokens_total: 0,
        context_tokens_peak: 0,
        model_round_total: 0,
        started_time: Some(now),
        finished_time: None,
        elapsed_s: None,
        summary: None,
        error: None,
        updated_time: now,
    }
}

pub(crate) fn create_swarm_team_task_record(
    run: &TeamRunRecord,
    agent_id: &str,
    target_session_id: Option<String>,
    spawned_session_id: Option<String>,
    priority: i64,
) -> TeamTaskRecord {
    let now = now_ts();
    TeamTaskRecord {
        task_id: format!("task_{}", Uuid::new_v4().simple()),
        team_run_id: run.team_run_id.clone(),
        user_id: run.user_id.clone(),
        hive_id: run.hive_id.clone(),
        agent_id: agent_id.to_string(),
        target_session_id,
        spawned_session_id,
        session_run_id: None,
        status: "queued".to_string(),
        retry_count: 0,
        priority,
        started_time: None,
        finished_time: None,
        elapsed_s: None,
        result_summary: None,
        error: None,
        updated_time: now,
    }
}

pub(crate) async fn wait_for_swarm_runs(
    context: &ToolContext<'_>,
    run_ids: &[String],
    wait_seconds: Option<f64>,
    poll_interval_seconds: f64,
    emit_progress: bool,
) -> Result<Value> {
    let run_ids = dedupe_non_empty_strings(run_ids.to_vec());
    if run_ids.is_empty() {
        return Ok(build_model_tool_success(
            "wait",
            "error",
            "No worker runs were provided.",
            json!({
                "run_ids": [],
                "counts": {
                    "total": 0,
                    "done": 0,
                    "success": 0,
                    "failed": 0,
                    "queued": 0,
                    "running": 0,
                },
                "items": [],
            }),
        ));
    }

    let poll_interval = normalize_swarm_poll_interval(poll_interval_seconds);
    let started_at = Instant::now();

    loop {
        let snapshots = collect_swarm_run_snapshots(context, &run_ids)?;
        let total = snapshots.len();
        let done_total = snapshots.iter().filter(|item| item.terminal).count();
        let success_total = snapshots
            .iter()
            .filter(|item| item.status == "success")
            .count();
        let failed_total = snapshots.iter().filter(|item| item.failed).count();
        let queued_total = snapshots
            .iter()
            .filter(|item| item.status == "queued")
            .count();
        let running_total = snapshots
            .iter()
            .filter(|item| item.status == "running")
            .count();
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let all_finished = done_total >= total;
        let timed_out = wait_seconds
            .filter(|value| *value > 0.0)
            .is_some_and(|value| elapsed_s >= value && !all_finished);
        let immediate_snapshot = wait_seconds.is_some_and(|value| value <= 0.0);

        if all_finished || timed_out || immediate_snapshot {
            let state = if all_finished {
                if failed_total == 0 {
                    "completed"
                } else {
                    "partial"
                }
            } else if timed_out {
                "timeout"
            } else {
                "running"
            };
            let items = snapshots
                .into_iter()
                .map(|item| item.payload)
                .collect::<Vec<_>>();
            let response = build_model_tool_success_with_hint(
                "wait",
                state,
                if state == "completed" {
                    "All worker runs finished.".to_string()
                } else if state == "partial" {
                    "Worker runs finished with partial success.".to_string()
                } else if state == "timeout" {
                    "Waiting for worker runs timed out.".to_string()
                } else {
                    "Worker runs are still executing.".to_string()
                },
                json!({
                    "run_ids": run_ids.clone(),
                    "wait_seconds": wait_seconds,
                    "wait_forever": wait_seconds.is_none(),
                    "elapsed_s": elapsed_s,
                    "all_finished": all_finished,
                    "counts": {
                        "total": total,
                        "done": done_total,
                        "success": success_total,
                        "failed": failed_total,
                        "queued": queued_total,
                        "running": running_total,
                    },
                    "items": items,
                }),
                if timed_out || !all_finished {
                    Some(
                        "Use agent_swarm.wait again or inspect status/history before treating unfinished worker runs as complete."
                            .to_string(),
                    )
                } else {
                    None
                },
            );
            crate::services::subagents::suppress_auto_wake_from_wait_result(&response);
            return Ok(response);
        }

        if emit_progress {
            if let Some(emitter) = context.event_emitter.as_ref() {
                emitter.emit(
                    "progress",
                    json!({
                        "stage": "swarm_wait",
                        "summary": i18n::t("monitor.summary.swarm_wait"),
                        "total": total,
                        "done_total": done_total,
                        "success_total": success_total,
                        "failed_total": failed_total,
                        "elapsed_s": elapsed_s,
                    }),
                );
            }
        }

        sleep(Duration::from_secs_f64(poll_interval)).await;
    }
}

fn collect_swarm_run_snapshots(
    context: &ToolContext<'_>,
    run_ids: &[String],
) -> Result<Vec<SwarmRunSnapshot>> {
    let mut output = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        let record = context.storage.get_session_run(run_id)?;
        if let Some(record) = record {
            let status = record.status.trim().to_ascii_lowercase();
            let terminal = is_swarm_run_terminal(&status);
            let failed = is_swarm_run_failed(&status);
            output.push(SwarmRunSnapshot {
                status,
                terminal,
                failed,
                payload: json!({
                    "run_id": record.run_id,
                    "status": record.status,
                    "terminal": terminal,
                    "failed": failed,
                    "session_id": record.session_id,
                    "agent_id": record.agent_id,
                    "started_time": record.started_time,
                    "finished_time": record.finished_time,
                    "elapsed_s": record.elapsed_s,
                    "result_preview": compact_swarm_run_result_preview(record.result.as_deref()),
                    "error": record.error,
                    "updated_time": record.updated_time,
                }),
            });
        } else {
            output.push(SwarmRunSnapshot {
                status: "not_found".to_string(),
                terminal: true,
                failed: true,
                payload: json!({
                    "run_id": run_id,
                    "status": "not_found",
                    "terminal": true,
                    "failed": true,
                    "error": "run not found",
                }),
            });
        }
    }
    Ok(output)
}
