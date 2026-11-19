//! 后台命令的完成通知。
//!
//! 命令工具在 `run_in_background` 模式下立即返回，进程继续由命令会话持有。
//! 这里负责在会话真正退出时把结果送回模型：线程仍在跑就走邮箱注入当前轮，
//! 线程空闲就复用子智能体完成唤醒那条持久任务链路（提交 agent message + wake）。
//!
//! 为了避免打扰，通知还有三道闸：用户已停止/归档线程时不唤醒；模型自己用
//! 命令会话读到过终态时不再重复通知；短窗口内的多条完成合并成一条消息。

use crate::core::long_task;
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
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::time::Duration;
use tracing::warn;

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
            language: i18n::get_language(),
        }))
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

/// 命令工具里的统一收口：终态会话生成通知项并交给合并窗口。
pub(crate) fn schedule_snapshot_notification(
    notifier: &Option<Arc<CommandNotifier>>,
    command_session_id: &str,
) {
    let Some(notifier) = notifier.as_ref() else {
        return;
    };
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
