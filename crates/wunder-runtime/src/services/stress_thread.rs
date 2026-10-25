//! Stress-thread generation jobs.
//!
//! One explicitly admitted generation runs at a time. A bounded in-memory
//! registry exposes progress while the production ThreadStore writes rollout
//! records and projections. A job stays running until its worker returns.

use crate::storage::stress_model::{estimated_max_items, validate_spec, MAX_TOTAL_ITEMS};
pub use crate::storage::stress_model::{MAX_MODEL_ROUNDS, MAX_USER_ROUNDS};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

const MAX_TRACKED_JOBS: usize = 8;

pub struct StartStressJobRequest {
    /// Injected thread store selects the active runtime profile. Service
    /// generation uses PostgreSQL; embedded profiles retain local SQLite.
    pub thread_store: Arc<crate::storage::RuntimeThreadStore>,
    pub user_id: String,
    /// Service session catalog used by the normal chat/session APIs. The
    /// canonical writer owns the thread history, while this catalog entry
    /// makes a generated thread discoverable after a restart.
    pub user_store: Arc<crate::services::user_store::UserStore>,
    pub user_rounds: i64,
    pub model_rounds_per_turn: i64,
    pub title: Option<String>,
    pub seed: Option<u64>,
    pub scenario: crate::storage::StressThreadScenario,
    pub thread_updates: Option<Arc<crate::orchestrator::Orchestrator>>,
}

struct StressJob {
    user_id: String,
    session_id: String,
    total_rounds: i64,
    model_rounds_per_turn: i64,
    requested_seed: Option<u64>,
    scenario: crate::storage::StressThreadScenario,
    created_time: f64,
    state: StressJobState,
}

enum StressJobState {
    Running {
        done_rounds: i64,
        items_written: i64,
    },
    Completed {
        stats: Value,
    },
    Failed {
        error: String,
        done_rounds: i64,
    },
}

static JOBS: OnceLock<Mutex<HashMap<String, Arc<Mutex<StressJob>>>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, Arc<Mutex<StressJob>>>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_job(
    map: &mut HashMap<String, Arc<Mutex<StressJob>>>,
    job_id: String,
    job: Arc<Mutex<StressJob>>,
) -> Result<(), String> {
    // Admission and insertion share the registry lock. A second caller can
    // never overtake a worker that is still writing or waiting for DB capacity.
    if map
        .values()
        .any(|job| matches!(job.lock().state, StressJobState::Running { .. }))
    {
        return Err("已有压测生成任务正在运行，请等待完成后重试".to_string());
    }
    while map.len() >= MAX_TRACKED_JOBS {
        let oldest = map
            .iter()
            .filter_map(|(id, job)| {
                let job = job.lock();
                (!matches!(job.state, StressJobState::Running { .. }))
                    .then(|| (id.clone(), job.created_time))
            })
            .min_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| left.0.cmp(&right.0))
            })
            .map(|(id, _)| id)
            .ok_or_else(|| "压测任务数量已达上限，请稍后重试".to_string())?;
        map.remove(&oldest);
    }
    map.insert(job_id, job);
    Ok(())
}

pub fn validate_stress_params(user_rounds: i64, model_rounds_per_turn: i64) -> Result<(), String> {
    if !(1..=MAX_USER_ROUNDS).contains(&user_rounds) {
        return Err(format!("user_rounds 必须在 1..={MAX_USER_ROUNDS}"));
    }
    if !(1..=MAX_MODEL_ROUNDS).contains(&model_rounds_per_turn) {
        return Err(format!("model_rounds 必须在 1..={MAX_MODEL_ROUNDS}"));
    }
    if estimated_max_items(user_rounds, model_rounds_per_turn) > MAX_TOTAL_ITEMS {
        return Err("生成规模超出单线程 item 上限，请降低轮次".to_string());
    }
    Ok(())
}

/// Launch one generation job. Returns the job/session identifiers immediately;
/// progress is observable through `stress_job_status`.
pub fn start_stress_thread_job(request: StartStressJobRequest) -> Result<Value, String> {
    let StartStressJobRequest {
        thread_store,
        user_id,
        user_store,
        user_rounds,
        model_rounds_per_turn,
        title,
        seed,
        scenario,
        thread_updates,
    } = request;
    validate_stress_params(user_rounds, model_rounds_per_turn)?;
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Err("missing user identity".to_string());
    }
    let session_id = Uuid::new_v4().to_string();
    let title = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("渲染压测 {user_rounds}×{model_rounds_per_turn}"));
    let spec = crate::storage::StressThreadSpec {
        session_id: session_id.clone(),
        title,
        workspace_id: None,
        user_rounds,
        model_rounds_per_turn,
        seed,
        scenario,
    };
    // The storage layer re-validates against the same caps; fail fast here so
    // obviously bad requests never create a job entry.
    validate_spec(&spec).map_err(|error| error.to_string())?;
    let catalog_title = spec.title.clone();
    let catalog_workspace_id = spec.workspace_id.clone();
    let job_id = Uuid::new_v4().to_string();
    let job = Arc::new(Mutex::new(StressJob {
        user_id: user_id.clone(),
        session_id: session_id.clone(),
        total_rounds: user_rounds,
        model_rounds_per_turn,
        requested_seed: seed,
        scenario,
        created_time: now_ts(),
        state: StressJobState::Running {
            done_rounds: 0,
            items_written: 0,
        },
    }));
    {
        let mut map = jobs().lock();
        register_job(&mut map, job_id.clone(), Arc::clone(&job))?;
    }

    let owner = user_id.clone();
    let progress_session = session_id.clone();
    crate::core::long_task::spawn("stress_thread.generate", async move {
        // Keep the long generation off async workers. Blocking work cannot be
        // aborted by dropping its waiter; retain Running until the worker
        // really settles so admission cannot overlap a still-writing task.
        let job_for_progress = Arc::clone(&job);
        let session_for_progress = progress_session;
        let outcome: anyhow::Result<crate::storage::StressThreadStats> =
            crate::core::blocking::run_to_completion(
                crate::core::blocking::BlockingKind::Db,
                "stress_thread.generate",
                move || {
                    // Service stores bridge synchronous PostgreSQL operations
                    // through block_in_place; a current-thread runtime panics.
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .max_blocking_threads(4)
                        .enable_all()
                        .build()?;
                    runtime.block_on(thread_store.generate_stress_thread(
                        &owner,
                        &spec,
                        |done_rounds, items_written| {
                            update_progress(&job_for_progress, done_rounds, items_written);
                            if let Some(orchestrator) = thread_updates.as_ref() {
                                // The writer invokes progress only after its
                                // canonical and admission records commit.
                                orchestrator.notify_thread_committed(&session_for_progress, 0);
                            }
                        },
                    ))
                },
            )
            .await;
        match outcome {
            Ok(stats) => {
                // Register the durable service-facing session only after the
                // canonical generation and its projections have committed.
                // This keeps a failed generation from appearing as a usable
                // chat session while allowing ordinary session/history APIs to
                // rediscover completed stress threads after a process restart.
                let now = now_ts();
                let catalog_user_id = job.lock().user_id.clone();
                let record = crate::storage::ChatSessionRecord {
                    session_id: stats.session_id.clone(),
                    user_id: catalog_user_id,
                    title: catalog_title,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_message_at: now,
                    agent_id: None,
                    workspace_id: catalog_workspace_id,
                    tool_overrides: Vec::new(),
                    parent_session_id: None,
                    parent_message_id: None,
                    spawn_label: None,
                    spawned_by: None,
                };
                let catalog_result =
                    crate::core::blocking::run_db("stress_thread.catalog", move || {
                        user_store.upsert_chat_session(&record)
                    })
                    .await;
                let mut state = job.lock();
                if let Err(error) = catalog_result {
                    state.state = StressJobState::Failed {
                        error: format!("生成完成但会话目录写入失败: {error}"),
                        done_rounds: state.total_rounds,
                    };
                    return;
                }
                state.state = StressJobState::Completed {
                    stats: json!({
                        "session_id": stats.session_id,
                        "user_turns": stats.user_turns,
                        "items": stats.items,
                        "tool_calls": stats.tool_calls,
                        "model_requests": stats.model_requests,
                        "model_actions": stats.model_actions,
                        "rollout_records": stats.rollout_records,
                        "seed": stats.seed,
                        "profile": stats.profile,
                        "scenario": stats.scenario,
                        "initial_context_tokens": stats.initial_context_tokens,
                        "storage_bytes": stats.storage_bytes,
                    }),
                };
            }
            Err(error) => {
                let mut state = job.lock();
                let done = match &state.state {
                    StressJobState::Running { done_rounds, .. } => *done_rounds,
                    _ => 0,
                };
                state.state = StressJobState::Failed {
                    error: error.to_string(),
                    done_rounds: done,
                };
            }
        }
    });

    Ok(json!({
        "job_id": job_id,
        "session_id": session_id,
        "user_rounds": user_rounds,
        "model_rounds": model_rounds_per_turn,
        "requested_user_rounds": user_rounds,
        "requested_model_rounds_per_turn": model_rounds_per_turn,
        "requested_seed": seed,
        "scenario": scenario.as_str(),
        "state": "running",
    }))
}

fn update_progress(job: &Arc<Mutex<StressJob>>, done_rounds: i64, items_written: i64) {
    let mut state = job.lock();
    if let StressJobState::Running {
        done_rounds: done,
        items_written: items,
    } = &mut state.state
    {
        *done = done_rounds;
        *items = items_written;
    }
}

/// Snapshot one job. `requester` must match the launching user; jobs stay
/// private to their owner.
pub fn stress_job_status(user_id: &str, job_id: &str) -> Option<Value> {
    let job = Arc::clone(jobs().lock().get(job_id)?);
    let job = job.lock();
    if job.user_id != user_id {
        return None;
    }
    Some(job_snapshot(job_id, &job))
}

fn job_snapshot(job_id: &str, job: &StressJob) -> Value {
    let status = match &job.state {
        StressJobState::Running {
            done_rounds,
            items_written,
        } => json!({
            "state": "running",
            "done_rounds": done_rounds,
            "total_rounds": job.total_rounds,
            "items_written": items_written,
        }),
        StressJobState::Completed { stats } => {
            let mut payload = stats.clone();
            payload["state"] = json!("completed");
            payload["total_rounds"] = json!(job.total_rounds);
            payload
        }
        StressJobState::Failed { error, done_rounds } => json!({
            "state": "failed",
            "error": error,
            "done_rounds": done_rounds,
            "total_rounds": job.total_rounds,
        }),
    };
    json!({
        "job_id": job_id,
        "session_id": job.session_id,
        "total_rounds": job.total_rounds,
        "requested_user_rounds": job.total_rounds,
        "requested_model_rounds_per_turn": job.model_rounds_per_turn,
        "requested_seed": job.requested_seed,
        "scenario": job.scenario.as_str(),
        "created_time": job.created_time,
        "status": status,
    })
}

/// List jobs owned by one user (bounded, newest first).
pub fn list_stress_jobs(user_id: &str) -> Vec<Value> {
    let mut rows: Vec<(f64, Value)> = jobs()
        .lock()
        .iter()
        .filter_map(|(job_id, job)| {
            let job = job.lock();
            if job.user_id != user_id {
                return None;
            }
            Some((job.created_time, job_snapshot(job_id, &job)))
        })
        .collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(created_time: f64, state: StressJobState) -> Arc<Mutex<StressJob>> {
        Arc::new(Mutex::new(StressJob {
            user_id: "sample-user".into(),
            session_id: "sample-thread".into(),
            total_rounds: 2,
            model_rounds_per_turn: 3,
            requested_seed: Some(7),
            scenario: crate::storage::StressThreadScenario::Complex,
            created_time,
            state,
        }))
    }

    #[test]
    fn registry_rejects_another_job_until_the_worker_settles() {
        let mut map = HashMap::new();
        let active = job(
            1.0,
            StressJobState::Running {
                done_rounds: 0,
                items_written: 0,
            },
        );
        register_job(&mut map, "sample-active".into(), active.clone()).unwrap();
        assert!(register_job(
            &mut map,
            "sample-rejected".into(),
            job(2.0, StressJobState::Completed { stats: json!({}) })
        )
        .is_err());
        assert_eq!(map.len(), 1);
        active.lock().state = StressJobState::Completed { stats: json!({}) };
        register_job(
            &mut map,
            "sample-next".into(),
            job(
                2.0,
                StressJobState::Running {
                    done_rounds: 0,
                    items_written: 0,
                },
            ),
        )
        .unwrap();
    }

    #[test]
    fn registry_evicts_the_oldest_terminal_job_before_insertion() {
        let mut map = HashMap::new();
        for index in 0..MAX_TRACKED_JOBS {
            register_job(
                &mut map,
                format!("sample-{index}"),
                job(
                    index as f64,
                    StressJobState::Failed {
                        error: "sample failure".into(),
                        done_rounds: 0,
                    },
                ),
            )
            .unwrap();
        }
        register_job(
            &mut map,
            "sample-new".into(),
            job(20.0, StressJobState::Completed { stats: json!({}) }),
        )
        .unwrap();
        assert_eq!(map.len(), MAX_TRACKED_JOBS);
        assert!(!map.contains_key("sample-0"));
        assert!(map.contains_key("sample-new"));
    }

    #[test]
    fn validation_uses_the_canonical_total_item_bound() {
        assert!(validate_stress_params(1199, 1000).is_ok());
        assert!(validate_stress_params(1200, 1000).is_err());
        assert!(validate_stress_params(0, 1).is_err());
        assert!(validate_stress_params(1, MAX_MODEL_ROUNDS + 1).is_err());
    }

    #[test]
    fn completed_snapshot_preserves_request_shape_and_actual_counts() {
        let completed = job(
            1.0,
            StressJobState::Completed {
                stats: json!({"seed":7,"profile":"storage-stress-v1","initial_context_tokens":72,"user_turns":2,"items":10,
                    "tool_calls":2,"model_requests":6,"model_actions":8,"rollout_records":24,
                    "storage_bytes":{"rollout_raw_bytes":11,"rollout_zstd_bytes":0,"state_bytes":22,"state_wal_bytes":0,"queue_bytes":33,"queue_wal_bytes":0,"thread_history_bytes":44,"thread_history_wal_bytes":0,"total_bytes":110}}),
            },
        );
        let snapshot = job_snapshot("sample-job", &completed.lock());
        assert_eq!(snapshot["requested_user_rounds"], 2);
        assert_eq!(snapshot["requested_model_rounds_per_turn"], 3);
        assert_eq!(snapshot["requested_seed"], 7);
        assert_eq!(snapshot["status"]["state"], "completed");
        assert_eq!(snapshot["status"]["seed"], 7);
        assert_eq!(snapshot["status"]["profile"], "storage-stress-v1");
        assert!(
            snapshot["status"]["initial_context_tokens"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(snapshot["status"]["model_requests"], 6);
        assert_eq!(snapshot["status"]["model_actions"], 8);
        assert_eq!(snapshot["status"]["storage_bytes"]["total_bytes"], 110);
    }
}
