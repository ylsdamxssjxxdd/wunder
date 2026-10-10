//! Stress-thread generation jobs.
//!
//! One background blocking task per job; jobs live in a bounded in-memory
//! registry so the tool/API can poll progress. Bulk SQL lives in the SQLite
//! storage layer (`SqliteStorage::generate_stress_thread`); builds without the
//! sqlite-storage feature keep the tool surface but refuse to start jobs.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

const MAX_TRACKED_JOBS: usize = 8;
pub const MAX_USER_ROUNDS: i64 = 2000;
pub const MAX_MODEL_ROUNDS: i64 = 2000;
/// 1000×1000 (the default stress profile) samples at most ~5M items.
const MAX_TOTAL_ITEMS: i64 = 6_000_000;

pub struct StartStressJobRequest {
    pub db_path: String,
    pub user_id: String,
    pub user_rounds: i64,
    pub model_rounds_per_turn: i64,
    pub title: Option<String>,
}

struct StressJob {
    user_id: String,
    session_id: String,
    total_rounds: i64,
    created_time: f64,
    state: StressJobState,
}

enum StressJobState {
    Running { done_rounds: i64, items_written: i64 },
    Completed { stats: Value },
    Failed { error: String, done_rounds: i64 },
}

static JOBS: OnceLock<Mutex<HashMap<String, Arc<Mutex<StressJob>>>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, Arc<Mutex<StressJob>>>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn trim_finished_jobs(map: &mut HashMap<String, Arc<Mutex<StressJob>>>) {
    if map.len() <= MAX_TRACKED_JOBS {
        return;
    }
    // Running jobs always survive; oldest finished entries go first.
    let mut finished: Vec<(String, f64)> = map
        .iter()
        .filter(|(_, job)| matches!(job.lock().state, StressJobState::Completed { .. } | StressJobState::Failed { .. }))
        .map(|(id, job)| (id.clone(), job.lock().created_time))
        .collect();
    if finished.len() <= MAX_TRACKED_JOBS {
        return;
    }
    finished.sort_by(|a, b| a.1.total_cmp(&b.1));
    let remove_count = finished.len() - MAX_TRACKED_JOBS;
    for (id, _) in finished.into_iter().take(remove_count) {
        map.remove(&id);
    }
}

pub fn validate_stress_params(user_rounds: i64, model_rounds_per_turn: i64) -> Result<(), String> {
    if !(1..=MAX_USER_ROUNDS).contains(&user_rounds) {
        return Err(format!("user_rounds 必须在 1..={MAX_USER_ROUNDS}"));
    }
    if !(1..=MAX_MODEL_ROUNDS).contains(&model_rounds_per_turn) {
        return Err(format!("model_rounds 必须在 1..={MAX_MODEL_ROUNDS}"));
    }
    if user_rounds.saturating_mul(model_rounds_per_turn * 5 + 1) > MAX_TOTAL_ITEMS {
        return Err("生成规模超出单线程 item 上限，请降低轮次".to_string());
    }
    Ok(())
}

/// Launch one generation job. Returns the job/session identifiers immediately;
/// progress is observable through `stress_job_status`.
pub fn start_stress_thread_job(request: StartStressJobRequest) -> Result<Value, String> {
    let StartStressJobRequest {
        db_path,
        user_id,
        user_rounds,
        model_rounds_per_turn,
        title,
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
    let job_id = Uuid::new_v4().to_string();
    let job = Arc::new(Mutex::new(StressJob {
        user_id: user_id.clone(),
        session_id: session_id.clone(),
        total_rounds: user_rounds,
        created_time: now_ts(),
        state: StressJobState::Running { done_rounds: 0, items_written: 0 },
    }));
    {
        let mut map = jobs().lock();
        trim_finished_jobs(&mut map);
        map.insert(job_id.clone(), Arc::clone(&job));
    }

    #[cfg(any(feature = "sqlite-storage", test))]
    {
        let db_path = if db_path.trim().is_empty() {
            "./config/data/wunder.db".to_string()
        } else {
            db_path
        };
        let spec = crate::storage::StressThreadSpec {
            session_id: session_id.clone(),
            title,
            user_rounds,
            model_rounds_per_turn,
        };
        let owner = user_id.clone();
        crate::core::long_task::spawn("stress_thread.generate", async move {
            // The bulk writer is one long blocking call; keep it off the async
            // workers via the shared blocking pool. One dedicated storage
            // instance per job so the write connection never contends with the
            // runtime's pooled connection.
            let job_for_progress = Arc::clone(&job);
            let outcome = crate::core::blocking::run_db(
                "stress_thread.generate",
                move || {
                    let storage = crate::storage::SqliteStorage::new(db_path);
                    storage.generate_stress_thread(
                        &owner,
                        &spec,
                        |done_rounds, items_written| {
                            let mut state = job_for_progress.lock();
                            if let StressJobState::Running {
                                done_rounds: done,
                                items_written: items,
                            } = &mut state.state
                            {
                                *done = done_rounds;
                                *items = items_written;
                            }
                        },
                    )
                },
            )
            .await;
            let mut state = job.lock();
            match outcome {
                Ok(stats) => {
                    state.state = StressJobState::Completed {
                        stats: json!({
                            "session_id": stats.session_id,
                            "user_turns": stats.user_turns,
                            "items": stats.items,
                            "tool_calls": stats.tool_calls,
                        }),
                    };
                }
                Err(error) => {
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
    }
    #[cfg(not(any(feature = "sqlite-storage", test)))]
    {
        let _ = (db_path, title);
        let mut state = job.lock();
        state.state = StressJobState::Failed {
            error: "当前构建未启用 sqlite 存储，无法生成压测线程".to_string(),
            done_rounds: 0,
        };
    }

    Ok(json!({
        "job_id": job_id,
        "session_id": session_id,
        "user_rounds": user_rounds,
        "model_rounds": model_rounds_per_turn,
        "state": "running",
    }))
}

/// Snapshot one job. `requester` must match the launching user; jobs stay
/// private to their owner.
pub fn stress_job_status(user_id: &str, job_id: &str) -> Option<Value> {
    let job = Arc::clone(jobs().lock().get(job_id)?);
    let job = job.lock();
    if job.user_id != user_id {
        return None;
    }
    let status = match &job.state {
        StressJobState::Running { done_rounds, items_written } => json!({
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
    Some(json!({
        "job_id": job_id,
        "session_id": job.session_id,
        "total_rounds": job.total_rounds,
        "created_time": job.created_time,
        "status": status,
    }))
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
            let status = match &job.state {
                StressJobState::Running { done_rounds, items_written } => json!({
                    "state": "running",
                    "done_rounds": done_rounds,
                    "items_written": items_written,
                }),
                StressJobState::Completed { stats } => {
                    let mut payload = stats.clone();
                    payload["state"] = json!("completed");
                    payload
                }
                StressJobState::Failed { error, done_rounds } => {
                    json!({"state": "failed", "error": error, "done_rounds": done_rounds})
                }
            };
            Some((
                job.created_time,
                json!({
                    "job_id": job_id,
                    "session_id": job.session_id,
                    "total_rounds": job.total_rounds,
                    "created_time": job.created_time,
                    "status": status,
                }),
            ))
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
