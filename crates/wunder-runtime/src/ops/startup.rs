//! 进程启动收敛：上一进程遗留的 running/cancelling 没有执行器，只有持久
//! 监控行还在。按存活会话锁结算其残留受理并把监控行落为终态，避免管理端
//! 永久显示“运行中”。多副本共享同一 PostgreSQL 时，持锁的实例不受影响。
use crate::core::blocking;
use crate::state::AppState;
use anyhow::Result;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{info, warn};

const HISTORY_SCAN_LIMIT: i64 = 5000;
/// 与受理孤儿结算的宽限期一致：刚被队列领取的任务仍在交接窗口内。
const QUEUE_TAKEOVER_GRACE_S: f64 = 15.0;
/// 会话锁每 5s 续期；心跳超过 3 个周期仍未刷新说明持有进程已经消失，
/// 即使锁尚未到期也不能继续挡住启动收敛。
const LIVE_LOCK_HEARTBEAT_GRACE_S: f64 = 15.0;

pub async fn settle_restarted_activity(state: Arc<AppState>) -> Result<usize> {
    let storage = state.storage.clone();
    let records = blocking::run_db("startup.settle.list", move || {
        storage.load_recent_monitor_records(HISTORY_SCAN_LIMIT)
    })
    .await?;
    let now = Utc::now().timestamp_millis() as f64 / 1000.0;

    let mut by_user: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for payload in records {
        let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(status, "running" | "cancelling" | "queued") {
            continue;
        }
        let Some(session) = payload.get("session_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(user) = payload.get("user_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let session = session.trim();
        let user = user.trim();
        if session.is_empty() || user.is_empty() {
            continue;
        }
        by_user
            .entry(user.to_string())
            .or_default()
            .push((session.to_string(), status.to_string()));
    }

    let mut settled = 0usize;
    for (user, sessions) in by_user {
        let storage = state.storage.clone();
        let owner = user.clone();
        let live = match blocking::run_db("startup.settle.locks", move || {
            storage.list_session_locks_by_user(&owner)
        })
        .await
        {
            Ok(locks) => locks
                .into_iter()
                .filter(|lock| {
                    lock.expires_at > now && lock.updated_time > now - LIVE_LOCK_HEARTBEAT_GRACE_S
                })
                .map(|lock| lock.session_id.trim().to_string())
                .collect::<HashSet<_>>(),
            Err(error) => {
                // 无法确认存活锁时保持原状，宁可留下旧状态也不误伤其它实例。
                warn!(user = %user, %error, "startup settle skipped: session locks unavailable");
                continue;
            }
        };
        for (session, status) in sessions {
            if live.contains(&session) {
                continue;
            }
            // 队列刚领取的任务处于锁交接窗口，留给执行器接管。
            if fresh_running_task(&state, &session).await {
                continue;
            }
            if status == "queued" && queued_has_tasks(&state, &session).await {
                continue;
            }
            let turns = match state
                .kernel
                .thread_runtime
                .settle_thread_orphans(&session)
                .await
            {
                Ok(turns) => turns,
                Err(error) => {
                    warn!(session = %session, %error, "startup settle failed for thread");
                    continue;
                }
            };
            if state.monitor.force_settle_session(&session) {
                settled += 1;
                info!(session = %session, turns, "settled restarted activity");
            }
        }
    }
    Ok(settled)
}

/// 排队线程仍有可执行任务时，属于正常的队列恢复路径。
/// 无法读取任务台账时按“有任务”保守处理。
async fn queued_has_tasks(state: &AppState, session: &str) -> bool {
    let thread_id = format!("thread_{session}");
    for status in ["pending", "retry"] {
        let storage = state.storage.clone();
        let thread = thread_id.clone();
        let has = blocking::run_db("startup.settle.queued", move || {
            storage
                .list_agent_tasks_by_thread(&thread, Some(status), 1)
                .map(|tasks| !tasks.is_empty())
        })
        .await
        .unwrap_or(true);
        if has {
            return true;
        }
    }
    false
}

/// 最近 15s 内被领取的 running 任务说明执行器正在接管，不能再按孤儿结算。
async fn fresh_running_task(state: &AppState, session: &str) -> bool {
    let storage = state.storage.clone();
    let thread = format!("thread_{session}");
    let fresh_after = Utc::now().timestamp_millis() as f64 / 1000.0 - QUEUE_TAKEOVER_GRACE_S;
    blocking::run_db("startup.settle.takeover", move || {
        storage
            .list_agent_tasks_by_thread(&thread, Some("running"), 1)
            .map(|tasks| tasks.iter().any(|task| task.updated_at > fresh_after))
    })
    .await
    .unwrap_or(true)
}
