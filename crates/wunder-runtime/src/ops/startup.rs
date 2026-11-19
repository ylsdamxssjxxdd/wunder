//! 进程启动收敛：上一进程遗留的 running/cancelling 没有执行器，只有持久
//! 监控行还在。按存活会话锁结算其残留受理并把监控行落为终态，避免管理端
//! 永久显示“运行中”。多副本共享同一 PostgreSQL 时，持锁的实例不受影响。
use crate::core::blocking;
use crate::services::tools::command_notify::{
    parse_pending_background, pending_background_is_stale, CommandCompletion, CommandNotifier,
    PENDING_BACKGROUND_PREFIX,
};
use crate::services::tools::command_sessions::{
    CommandProcessHandle, CommandSessionLaunchMode, CommandSessionStartSpec, CommandSessionTracker,
};
use crate::services::tools::spawn_sandbox_command_observer;
use crate::state::AppState;
use crate::storage::StorageBackend;
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
        // `waiting` 是停在问询/审批上的运行态：进程都没了，它同样没有执行器。
        if !matches!(status, "running" | "cancelling" | "queued" | "waiting") {
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
        // 任务台账里的 running 行不属于监控缓存的清理范围：队列只领取
        // pending/retry，被中途杀掉的执行留下的 running 行会永久留存，
        // 让线程一直显得「在运行」。这里先按存活锁把孤儿行落成终态。
        match state
            .kernel
            .thread_runtime
            .settle_orphaned_running_tasks(&user)
            .await
        {
            Ok(0) => {}
            Ok(count) => info!(user = %user, count, "settled orphaned running tasks"),
            Err(error) => warn!(user = %user, %error, "settle orphaned running tasks failed"),
        }
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

/// 恢复上一进程遗留的后台命令：沙盒会话仍在远端运行时重新挂 observer，
/// 本地或已不可达的会话以「失联」通知结算一次。
pub async fn recover_background_commands(state: Arc<AppState>) -> Result<usize> {
    let storage = state.storage.clone();
    let prefix = PENDING_BACKGROUND_PREFIX.to_string();
    let records = blocking::run_db("startup.background_commands.list", move || {
        storage.list_meta_prefix(&prefix)
    })
    .await?;
    if records.is_empty() {
        return Ok(0);
    }
    let config = state.config_store.get().await;
    let now = Utc::now().timestamp_millis() as f64 / 1000.0;
    let sandbox_enabled = crate::sandbox::sandbox_enabled(&config);
    let mut recovered = 0usize;
    for (key, value) in records {
        let Some(record) = parse_pending_background(&value) else {
            delete_pending_command(state.storage.clone(), key).await;
            continue;
        };
        if pending_background_is_stale(&record, now) {
            delete_pending_command(state.storage.clone(), key).await;
            continue;
        }
        let broker = state.control.command_sessions.clone();
        let notifier = CommandNotifier::from_parts(
            state.storage.clone(),
            Some(state.monitor.clone()),
            state.kernel.orchestrator.clone(),
            broker.clone(),
            record.user_id.clone(),
            record.session_id.clone(),
            record.language.clone(),
        );
        if record.mode == "sandbox" && sandbox_enabled {
            let spec = CommandSessionStartSpec {
                command_session_id: Some(record.command_session_id.clone()),
                tool_call_id: None,
                user_id: record.user_id.clone(),
                session_id: record.session_id.clone(),
                workspace_id: record.workspace_id.clone(),
                command_index: 0,
                command: record.command.clone(),
                cwd: record.cwd.clone(),
                shell: Some("sandbox".to_string()),
                launch_mode: CommandSessionLaunchMode::Shell,
                tty: false,
                interactive: false,
                started_at: chrono::DateTime::from_timestamp_millis(
                    (record.started_at * 1000.0) as i64,
                ),
            };
            let Some(tracker) = CommandSessionTracker::start_from_broker(broker.clone(), spec)
            else {
                continue;
            };
            let handle = Arc::new(CommandProcessHandle::new(None));
            if !broker.register_process(&record.command_session_id, Arc::clone(&handle)) {
                // 后台名额已满：保留记录，下一次启动再试。
                continue;
            }
            spawn_sandbox_command_observer(
                config.clone(),
                record.user_id.clone(),
                record.session_id.clone(),
                tracker,
                broker,
                Some(notifier),
                handle.cancellation_token(),
                0,
            );
            recovered += 1;
            continue;
        }
        // 本地进程的输出通道随上一进程消失；沙盒未启用时同样无法继续跟踪。
        let duration_ms = ((now - record.started_at) * 1000.0).max(0.0) as i64;
        notifier.schedule(CommandCompletion {
            command_session_id: record.command_session_id.clone(),
            command: record.command.clone(),
            cwd: record.cwd.clone(),
            exit_code: None,
            timed_out: false,
            error: Some(crate::i18n::t_in_language(
                "tool.command_completion.restart_lost",
                &record.language,
            )),
            duration_ms,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
        });
        notifier.clear_pending(&record.command_session_id).await;
        recovered += 1;
    }
    Ok(recovered)
}

async fn delete_pending_command(storage: Arc<dyn StorageBackend>, key: String) {
    let _ = blocking::run_db("startup.background_commands.delete", move || {
        storage.delete_meta_prefix(&key)
    })
    .await;
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
