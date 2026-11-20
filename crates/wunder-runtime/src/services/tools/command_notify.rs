//! 后台命令的完成通知。
//!
//! 命令工具在 `run_in_background` 模式下立即返回，进程继续由命令会话持有。
//! 这里负责在会话真正退出时把结果送回模型：线程仍在跑就走邮箱注入当前轮，
//! 线程空闲就复用子智能体完成唤醒那条持久任务链路（提交 agent message + wake）。
//!
//! 为了避免打扰，通知还有三道闸：用户已停止/归档线程时不唤醒；模型自己用
//! 命令会话读到过终态时不再重复通知；短窗口内的多条完成合并成一条消息。

use crate::core::{blocking, long_task};
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::services::runtime::thread::mailbox::AgentMessage;
use crate::services::subagents::{build_parent_auto_wake_request, parent_session_blocks_auto_wake};
use crate::services::tools::command_sessions::{
    CommandSessionBroker, CommandSessionSnapshot, CommandSessionStatus,
};
use crate::services::tools::ToolContext;
use crate::storage::StorageBackend;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::time::Duration;
use tracing::warn;

/// 跨重启恢复用的待跟踪记录；进程退出后仍留在 MetaStore 里。
pub(crate) const PENDING_BACKGROUND_PREFIX: &str = "bgcmd:";
const PENDING_MAX_AGE_S: f64 = 24.0 * 3600.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PendingBackgroundCommand {
    pub user_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub command_session_id: String,
    pub command: String,
    pub cwd: String,
    /// "sandbox" 可跨重启继续观察；"local" 的输出通道随进程消失。
    pub mode: String,
    pub started_at: f64,
    pub language: String,
    /// 写入该记录的进程实例；空值是旧版本记录，无法判活。
    #[serde(default)]
    pub owner_instance_id: String,
}

pub(crate) fn parse_pending_background(value: &str) -> Option<PendingBackgroundCommand> {
    serde_json::from_str(value).ok()
}

pub(crate) fn pending_background_is_stale(record: &PendingBackgroundCommand, now: f64) -> bool {
    (now - record.started_at).max(0.0) > PENDING_MAX_AGE_S
}

fn pending_background_key(user_id: &str, session_id: &str, command_session_id: &str) -> String {
    format!("{PENDING_BACKGROUND_PREFIX}{user_id}\u{1f}{session_id}\u{1f}{command_session_id}")
}

pub(crate) async fn persist_pending_command(
    storage: Arc<dyn StorageBackend>,
    record: &PendingBackgroundCommand,
) -> anyhow::Result<()> {
    let value = serde_json::to_string(record)?;
    let key = pending_background_key(
        &record.user_id,
        &record.session_id,
        &record.command_session_id,
    );
    blocking::run_db("tools.command.persist_background", move || {
        storage.set_meta(&key, &value)
    })
    .await?;
    Ok(())
}

pub(crate) async fn clear_pending_command(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    command_session_id: &str,
) -> anyhow::Result<()> {
    let key = pending_background_key(user_id, session_id, command_session_id);
    blocking::run_db("tools.command.clear_background", move || {
        storage.delete_meta_prefix(&key)
    })
    .await?;
    Ok(())
}

/// 同一线程的多条后台命令在这么短的窗口内先后结束就合并成一条通知。
const BATCH_WINDOW: Duration = Duration::from_millis(1_200);
/// 合并上限：并发进程上限是 16，这里再留一点余量。
const MAX_BATCH_ITEMS: usize = 24;
const MAX_COMMAND_CHARS: usize = 200;
const MAX_TAIL_CHARS: usize = 600;
const MAX_TOTAL_TAIL_CHARS: usize = 4_800;
/// 单条 agent message 的上限是 20000 字节，这里留出足够余量。
const MAX_PAYLOAD_BYTES: usize = 18_000;

#[derive(Debug, Clone)]
pub(crate) struct CommandCompletion {
    pub command_session_id: String,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

impl CommandCompletion {
    pub(crate) fn from_snapshot(snapshot: &CommandSessionSnapshot) -> Self {
        let ended_at = snapshot.ended_at.unwrap_or_else(chrono::Utc::now);
        let duration_ms = ended_at
            .signed_duration_since(snapshot.started_at)
            .num_milliseconds()
            .max(0);
        Self {
            command_session_id: snapshot.command_session_id.clone(),
            command: single_line(&snapshot.command),
            cwd: snapshot.cwd.clone(),
            exit_code: snapshot.exit_code,
            timed_out: snapshot.timed_out,
            error: snapshot.error.clone(),
            duration_ms,
            stdout_tail: snapshot.stdout_tail.clone(),
            stderr_tail: snapshot.stderr_tail.clone(),
        }
    }
}

/// 持有唤醒链路需要的句柄。命令工具在启动后台命令时创建一份，
/// 完成后由 detached watcher / 沙盒 observer 调 `schedule`。
pub(crate) struct CommandNotifier {
    storage: Arc<dyn StorageBackend>,
    monitor: Option<Arc<MonitorState>>,
    orchestrator: Arc<Orchestrator>,
    broker: Arc<CommandSessionBroker>,
    user_id: String,
    session_id: String,
    /// 在命令发起时的语言上下文中取定；唤醒任务里没有 task-local 语言。
    language: String,
}

impl CommandNotifier {
    pub(crate) fn from_context(context: &ToolContext<'_>) -> Option<Arc<Self>> {
        let broker = context.command_sessions.as_ref()?;
        let orchestrator = context.orchestrator.as_ref()?;
        Some(Arc::new(Self {
            storage: context.storage.clone(),
            monitor: context.monitor.as_ref().map(Arc::clone),
            orchestrator: Arc::clone(orchestrator),
            broker: Arc::clone(broker),
            user_id: context.user_id.to_string(),
            session_id: context.session_id.to_string(),
            language: i18n::prompt_language().to_string(),
        }))
    }

    /// 启动恢复等没有 ToolContext 的场景用它重新组装通知器。
    pub(crate) fn from_parts(
        storage: Arc<dyn StorageBackend>,
        monitor: Option<Arc<MonitorState>>,
        orchestrator: Arc<Orchestrator>,
        broker: Arc<CommandSessionBroker>,
        user_id: String,
        session_id: String,
        language: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            storage,
            monitor,
            orchestrator,
            broker,
            user_id,
            session_id,
            language,
        })
    }

    pub(crate) async fn persist_pending(
        &self,
        command_session_id: &str,
        command: &str,
        cwd: &str,
        workspace_id: &str,
        mode: &str,
        started_at: f64,
    ) {
        let record = PendingBackgroundCommand {
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
            workspace_id: workspace_id.to_string(),
            command_session_id: command_session_id.to_string(),
            command: command.to_string(),
            cwd: cwd.to_string(),
            mode: mode.to_string(),
            started_at,
            language: self.language.clone(),
            owner_instance_id: crate::instance_liveness::current_instance()
                .unwrap_or_default()
                .to_string(),
        };
        if let Err(err) = persist_pending_command(self.storage.clone(), &record).await {
            warn!(
                "persist background command failed: command_session_id={}, error={err}",
                command_session_id
            );
        }
    }

    pub(crate) async fn clear_pending(&self, command_session_id: &str) {
        if let Err(err) = clear_pending_command(
            self.storage.clone(),
            &self.user_id,
            &self.session_id,
            command_session_id,
        )
        .await
        {
            warn!(
                "clear background command record failed: command_session_id={}, error={err}",
                command_session_id
            );
        }
    }

    /// 从命令会话快照生成通知项；仍在运行或会话已被回收时返回 None。
    pub(crate) fn completion_for(&self, command_session_id: &str) -> Option<CommandCompletion> {
        let snapshot = self.broker.snapshot(command_session_id)?;
        (snapshot.status != CommandSessionStatus::Running)
            .then(|| CommandCompletion::from_snapshot(&snapshot))
    }

    pub(crate) fn schedule(self: &Arc<Self>, completion: CommandCompletion) {
        let key = batch_key(&self.user_id, &self.session_id);
        let mut hub = completion_hub().lock();
        let entry = hub.entry(key.clone()).or_insert_with(|| CompletionBatch {
            notifier: Arc::clone(self),
            items: Vec::new(),
            scheduled: false,
        });
        entry.items.push(completion);
        if entry.items.len() > MAX_BATCH_ITEMS {
            entry.items.remove(0);
        }
        if entry.scheduled {
            return;
        }
        entry.scheduled = true;
        drop(hub);
        long_task::spawn("services.command_completion_notify", async move {
            tokio::time::sleep(BATCH_WINDOW).await;
            flush_batch(&key).await;
        });
    }

    async fn deliver(&self, mut items: Vec<CommandCompletion>) {
        // 用户停止/归档线程后不唤醒；与子智能体自动唤醒复用同一道闸。
        if self
            .monitor
            .as_ref()
            .is_some_and(|monitor| monitor.is_cancelled(&self.session_id))
        {
            return;
        }
        match parent_session_blocks_auto_wake(
            self.storage.as_ref(),
            self.monitor.as_deref(),
            &self.user_id,
            &self.session_id,
        ) {
            Ok(true) => return,
            Ok(false) => {}
            Err(err) => {
                warn!(
                    "inspect command completion wake gate failed: session_id={}, error={err}",
                    self.session_id
                );
                return;
            }
        }
        // 模型自己已经通过命令会话读到终态：不重复通知。
        items.retain(|item| !self.broker.is_model_observed(&item.command_session_id));
        if items.is_empty() {
            return;
        }
        let mut payload = completion_payload(&items, &self.language);
        let mut text = serde_json::to_string(&payload).unwrap_or_default();
        // 多语言/大输出可能把 JSON 撑过上限：去掉输出尾部再试一次，
        // 会话 id 与退出状态必须保留。
        if text.len() > MAX_PAYLOAD_BYTES {
            payload = completion_payload_with_tail_budget(&items, 0, &self.language);
            text = serde_json::to_string(&payload).unwrap_or_default();
        }
        if text.is_empty() || text.len() > MAX_PAYLOAD_BYTES {
            return;
        }
        let message = AgentMessage {
            id: format!("cmd_done_{}", items[0].command_session_id),
            source: String::new(),
            kind: "command_completion".to_string(),
            text,
            cancellation: None,
        };
        // 线程正在跑：注入当前轮的下一个模型轮次。
        if let Some(monitor) = self.monitor.as_ref() {
            match monitor
                .mailboxes
                .send(&self.user_id, &self.session_id, message.clone())
            {
                Ok(true) => return,
                Ok(false) | Err(_) => {}
            }
        }
        // 线程空闲：走持久 agent message，入账后唤醒一次新轮次。
        let request = match build_parent_auto_wake_request(
            self.storage.as_ref(),
            &self.user_id,
            &self.session_id,
            None,
            payload,
        ) {
            Ok(request) => request,
            Err(err) => {
                warn!(
                    "build command completion wake request failed: session_id={}, error={err}",
                    self.session_id
                );
                return;
            }
        };
        let runtime = self.orchestrator.task_runtime.read().upgrade();
        let Some(runtime) = runtime else {
            warn!(
                "command completion wake skipped: thread runtime unavailable, session_id={}",
                self.session_id
            );
            return;
        };
        if let Err(err) = runtime.submit_agent_message(request, &message).await {
            warn!(
                "submit command completion wake failed: session_id={}, error={err}",
                self.session_id
            );
        }
    }
}

/// 命令工具里的统一收口：终态会话先清掉待恢复记录，再生成通知项交给合并窗口。
pub(crate) async fn schedule_snapshot_notification(
    notifier: &Option<Arc<CommandNotifier>>,
    command_session_id: &str,
) {
    let Some(notifier) = notifier.as_ref() else {
        return;
    };
    notifier.clear_pending(command_session_id).await;
    if let Some(completion) = notifier.completion_for(command_session_id) {
        notifier.schedule(completion);
    }
}

struct CompletionBatch {
    notifier: Arc<CommandNotifier>,
    items: Vec<CommandCompletion>,
    scheduled: bool,
}

static COMPLETION_HUB: OnceLock<Mutex<HashMap<String, CompletionBatch>>> = OnceLock::new();

fn completion_hub() -> &'static Mutex<HashMap<String, CompletionBatch>> {
    COMPLETION_HUB.get_or_init(|| Mutex::new(HashMap::new()))
}

fn batch_key(user_id: &str, session_id: &str) -> String {
    format!("{user_id}\u{1f}{session_id}")
}

async fn flush_batch(key: &str) {
    let (notifier, items) = {
        let mut hub = completion_hub().lock();
        let Some(entry) = hub.get_mut(key) else {
            return;
        };
        entry.scheduled = false;
        let items = std::mem::take(&mut entry.items);
        let notifier = Arc::clone(&entry.notifier);
        if items.is_empty() {
            hub.remove(key);
        }
        (notifier, items)
    };
    if items.is_empty() {
        return;
    }
    notifier.deliver(items).await;
    // 投递期间可能又进来新的完成项，只清理确实空下来的批次。
    let mut hub = completion_hub().lock();
    if hub
        .get(key)
        .is_some_and(|entry| entry.items.is_empty() && !entry.scheduled)
    {
        hub.remove(key);
    }
}

fn completion_payload(items: &[CommandCompletion], language: &str) -> Value {
    completion_payload_with_tail_budget(items, MAX_TOTAL_TAIL_CHARS, language)
}

fn completion_payload_with_tail_budget(
    items: &[CommandCompletion],
    mut tail_budget: usize,
    language: &str,
) -> Value {
    let commands = items
        .iter()
        .map(|item| {
            let (stdout, stdout_cut) =
                tail_excerpt(&item.stdout_tail, MAX_TAIL_CHARS.min(tail_budget));
            tail_budget = tail_budget.saturating_sub(stdout.chars().count());
            let (stderr, stderr_cut) =
                tail_excerpt(&item.stderr_tail, MAX_TAIL_CHARS.min(tail_budget));
            tail_budget = tail_budget.saturating_sub(stderr.chars().count());
            json!({
                "command_session_id": item.command_session_id,
                "command": item.command,
                "cwd": item.cwd,
                "exit_code": item.exit_code,
                "timed_out": item.timed_out,
                "error": item.error,
                "duration_ms": item.duration_ms,
                "stdout_tail": stdout,
                "stderr_tail": stderr,
                "output_tail_truncated": stdout_cut || stderr_cut,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "type": "command_completion",
        "summary": completion_summary(items.len(), language),
        "count": items.len(),
        "commands": commands,
        "note": i18n::t_in_language("tool.command_completion.hint", language),
        "next_step": i18n::t_in_language("tool.command_completion.next_step", language),
    })
}

fn completion_summary(count: usize, language: &str) -> String {
    if count <= 1 {
        return i18n::t_in_language("tool.command_completion.header", language);
    }
    let mut params = HashMap::new();
    params.insert("count".to_string(), count.to_string());
    i18n::t_with_params_in_language("tool.command_completion.header_many", &params, language)
}

fn single_line(command: &str) -> String {
    let line = command
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if line.chars().count() > MAX_COMMAND_CHARS {
        let head: String = line.chars().take(MAX_COMMAND_CHARS - 1).collect();
        format!("{head}…")
    } else {
        line
    }
}

fn tail_excerpt(text: &str, limit: usize) -> (String, bool) {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return (String::new(), false);
    }
    if limit == 0 {
        return (String::new(), true);
    }
    let count = trimmed.chars().count();
    if count <= limit {
        return (trimmed.to_string(), false);
    }
    let tail: String = trimmed.chars().skip(count - limit).collect();
    (format!("…{tail}"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion(id: &str, stdout: &str) -> CommandCompletion {
        CommandCompletion {
            command_session_id: id.to_string(),
            command: "long running task".to_string(),
            cwd: "/workspace".to_string(),
            exit_code: Some(0),
            timed_out: false,
            error: None,
            duration_ms: 1_500,
            stdout_tail: stdout.to_string(),
            stderr_tail: String::new(),
        }
    }

    #[test]
    fn payload_keeps_session_ids_and_bounds_tails() {
        let items = vec![
            completion("cmd_a", &"x".repeat(10_000)),
            completion("cmd_b", "done"),
        ];
        let payload = completion_payload(&items, "zh-CN");
        assert_eq!(payload["type"], "command_completion");
        assert_eq!(payload["count"], 2);
        assert_eq!(payload["commands"][0]["command_session_id"], "cmd_a");
        assert_eq!(payload["commands"][1]["command_session_id"], "cmd_b");
        assert_eq!(payload["commands"][1]["stdout_tail"], "done");
        let stdout = payload["commands"][0]["stdout_tail"]
            .as_str()
            .expect("stdout tail");
        assert!(stdout.chars().count() <= MAX_TAIL_CHARS + 1);
        assert_eq!(payload["commands"][0]["output_tail_truncated"], true);
        assert!(serde_json::to_string(&payload).unwrap().len() < MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn summary_distinguishes_single_and_many() {
        let single = completion_summary(1, "zh-CN");
        let many = completion_summary(3, "zh-CN");
        assert_ne!(single, many);
        assert!(many.contains('3'));
    }

    #[tokio::test]
    #[cfg(feature = "sqlite-storage")]
    async fn pending_background_records_roundtrip_and_clear() {
        use crate::storage::SqliteStorage;
        let dir = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            dir.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .into_owned(),
        ));
        storage.ensure_initialized().unwrap();
        let record = PendingBackgroundCommand {
            user_id: "user_a".to_string(),
            session_id: "sess_1".to_string(),
            workspace_id: "ws_1".to_string(),
            command_session_id: "sandcmd_x".to_string(),
            command: "sleep 60".to_string(),
            cwd: "/workspace".to_string(),
            mode: "sandbox".to_string(),
            started_at: 1_000.0,
            language: "zh-CN".to_string(),
            owner_instance_id: "fixture-instance".to_string(),
        };
        persist_pending_command(storage.clone(), &record)
            .await
            .unwrap();
        let listed = storage.list_meta_prefix(PENDING_BACKGROUND_PREFIX).unwrap();
        assert_eq!(listed.len(), 1);
        let parsed = parse_pending_background(&listed[0].1).expect("pending record");
        assert_eq!(parsed.command_session_id, "sandcmd_x");
        assert!(!pending_background_is_stale(&parsed, 1_001.0));
        assert!(pending_background_is_stale(
            &parsed,
            1_000.0 + PENDING_MAX_AGE_S + 1.0
        ));
        clear_pending_command(storage.clone(), "user_a", "sess_1", "sandcmd_x")
            .await
            .unwrap();
        assert!(storage
            .list_meta_prefix(PENDING_BACKGROUND_PREFIX)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn tail_excerpt_keeps_the_end_of_output() {
        let (tail, cut) = tail_excerpt("0123456789", 4);
        assert!(cut);
        assert!(tail.ends_with("6789"));
        let (tail, cut) = tail_excerpt("ok", 10);
        assert!(!cut);
        assert_eq!(tail, "ok");
    }
}
