use super::{
    apply_patch_tool, build_model_tool_success, build_model_tool_success_with_hint,
    command_notify::{schedule_snapshot_notification, CommandNotifier},
    command_options::{
        apply_time_budget_secs, parse_command_budget, parse_command_session_input,
        parse_command_session_wait, parse_command_yield_time, parse_dry_run, CommandSessionInput,
        DEFAULT_COMMAND_TIMEOUT_S, DEFAULT_COMMAND_YIELD_MS, MAX_COMMAND_SESSION_POLL_MS,
        MAX_COMMAND_TIMEOUT_S,
    },
    command_output_guard::{
        derive_capture_policies, render_command_output, CommandOutputCapture,
        CommandOutputCaptureMeta, CommandOutputCollector, CommandOutputPolicy,
        DEFAULT_CAPTURE_TOTAL_BYTES,
    },
    command_sessions::{
        CommandProcessHandle, CommandSessionLaunchMode, CommandSessionSnapshot,
        CommandSessionStatus, CommandSessionStream, CommandSessionTracker, CommandStdinWrite,
    },
    execute_in_sandbox, recover_tool_args_value, resolve_tool_name,
    tool_error::{
        build_execute_command_failure_data, build_execute_command_failure_message,
        build_failed_tool_result, command_timeout_outcome, tool_timeout_meta, ToolErrorMeta,
    },
    ToolContext, ToolEventEmitter,
};
use crate::command_utils;
use crate::config::Config;
use crate::core::long_task;
use crate::core::process_tree::{ProcessTree, PIPE_EOF_GRACE};
use crate::core::python_runtime;
use crate::i18n;
use anyhow::{anyhow, Result};
#[cfg(windows)]
use encoding_rs::GBK;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio_util::sync::CancellationToken;
#[cfg(test)]
#[path = "command_tool_tests.rs"]
mod tests;
fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// 可交互进程的最长保留时间，避免挂死进程永久占用会话槽位。
const INTERACTIVE_COMMAND_MAX_S: f64 = 3600.0;

/// 同一条命令已经在本线程里跑着时返回那条会话：不再启动第二份。
///
/// 带单实例锁的程序（渲染器、桌面应用、服务端）重复启动只会被自己拦住，
/// 模型看到「锁被占用」再去 ps 排查，代价远高于直接复用既有会话。
fn running_duplicate_session(
    context: &ToolContext<'_>,
    command: &str,
    cwd: &Path,
) -> Option<CommandSessionSnapshot> {
    let broker = context.command_sessions.as_ref()?;
    let target = command.trim();
    if target.is_empty() {
        return None;
    }
    let cwd_text = cwd.to_string_lossy();
    broker
        .list_session_snapshots(context.user_id, context.session_id)
        .into_iter()
        .find(|snapshot| {
            snapshot.status == CommandSessionStatus::Running
                && snapshot.command.trim() == target
                && snapshot.cwd == cwd_text
        })
}

/// 复用已有会话的返回：形状与「首包 running」一致，并同步推进模型游标，
/// 下次用命令会话续读时不会把同段输出重放一遍。
fn build_reused_command_result(
    context: &ToolContext<'_>,
    snapshot: &CommandSessionSnapshot,
) -> Value {
    let guidance = i18n::t("tool.exec.reused_running");
    let truncated = snapshot.stdout_dropped_bytes > 0
        || snapshot.stderr_dropped_bytes > 0
        || snapshot.pty_dropped_bytes > 0;
    if !truncated {
        if let Some(broker) = context.command_sessions.as_ref() {
            broker.advance_model_cursor(
                context.user_id,
                context.session_id,
                &snapshot.command_session_id,
                snapshot.seq,
            );
        }
    }
    build_model_tool_success(
        "execute_command",
        "running",
        format!("Same command is already running in this thread. {guidance}"),
        json!({
            "command_session_id": &snapshot.command_session_id,
            "status": "running",
            "reused": true,
            "next_step": guidance,
            "results": [{
                "command": &snapshot.command,
                "command_index": snapshot.command_index,
                "command_session_id": &snapshot.command_session_id,
                "running": true,
                "seq": snapshot.seq,
                "stdout": &snapshot.stdout_tail,
                "stderr": &snapshot.stderr_tail,
            }],
        }),
    )
}

/// Detect commands that only idle (e.g. `sleep 5`) without doing real work.
fn is_bare_sleep_command(command: &str) -> bool {
    let trimmed = command.trim().trim_end_matches(';').trim();
    let mut parts = trimmed.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if !program.eq_ignore_ascii_case("sleep") && !program.eq_ignore_ascii_case("start-sleep") {
        return false;
    }
    let mut duration_args = 0usize;
    for part in parts {
        let cleaned = part.trim_end_matches(['s', 'm', 'h', 'd']);
        if cleaned.is_empty() || cleaned.parse::<f64>().is_err() {
            return false;
        }
        duration_args += 1;
    }
    duration_args >= 1
}

fn build_command_plan(content: &str, allow_all: bool, background: bool) -> Vec<String> {
    if allow_all || background {
        return vec![content.to_string()];
    }
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn content_is_bare_sleep(content: &str) -> bool {
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    lines.len() == 1 && is_bare_sleep_command(lines[0])
}

/// 下游 `head`/`cut`/`grep -q` 提前关管道会让上游收到 SIGPIPE，shell 收尾
/// 常以 141 退出。这是输出被截断，不是命令本身失败；把它当失败会诱使模型
/// 反复重跑同一条正确命令。
fn is_broken_pipe_exit(command: &str, returncode: i32, stderr: &str) -> bool {
    if returncode == 141 {
        return true;
    }
    if returncode == 0 || !command.contains('|') {
        return false;
    }
    let lowered = stderr.to_ascii_lowercase();
    lowered.contains("broken pipe") || lowered.contains("epipe")
}

pub(crate) fn extract_direct_patch_from_command(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.starts_with("*** Begin Patch") && trimmed.ends_with("*** End Patch") {
        return Some(trimmed.to_string());
    }
    None
}

fn resolve_stream_chunk_size(config: &Config) -> usize {
    let size = config.server.stream_chunk_size;
    if size == 0 {
        1024
    } else {
        size
    }
}

fn safe_chunk_boundary(text: &str, max_bytes: usize) -> usize {
    if text.len() <= max_bytes {
        return text.len();
    }
    let mut index = max_bytes.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    if index == 0 {
        index = max_bytes.min(text.len());
        while index < text.len() && !text.is_char_boundary(index) {
            index += 1;
        }
        if index == 0 {
            index = text.len();
        }
    }
    index
}

#[allow(clippy::too_many_arguments)]
fn emit_tool_output_chunks(
    emitter: &ToolEventEmitter,
    tool_name: &str,
    command: &str,
    stream_name: &str,
    pending: &mut String,
    chunk_size: usize,
    force: bool,
    command_session: Option<&CommandSessionTracker>,
) {
    if pending.is_empty() {
        return;
    }
    let limit = chunk_size.max(1);
    loop {
        if pending.is_empty() {
            break;
        }
        if !force && pending.len() < limit {
            break;
        }
        let take_len = if pending.len() <= limit {
            pending.len()
        } else {
            safe_chunk_boundary(pending, limit)
        };
        if take_len == 0 {
            break;
        }
        let chunk = pending[..take_len].to_string();
        pending.replace_range(..take_len, "");
        if chunk.is_empty() {
            break;
        }
        let mut payload = serde_json::Map::new();
        payload.insert("tool".to_string(), Value::String(tool_name.to_string()));
        payload.insert("command".to_string(), Value::String(command.to_string()));
        payload.insert("stream".to_string(), Value::String(stream_name.to_string()));
        payload.insert("delta".to_string(), Value::String(chunk));
        if let Some(command_session) = command_session {
            command_session.decorate_legacy_payload(&mut payload);
        }
        emitter.emit("tool_output_delta", Value::Object(payload));
    }
}

fn command_session_stream_from_name(stream_name: &str) -> CommandSessionStream {
    if stream_name.eq_ignore_ascii_case("pty") {
        CommandSessionStream::Pty
    } else if stream_name.to_ascii_lowercase().contains("err") {
        CommandSessionStream::Stderr
    } else {
        CommandSessionStream::Stdout
    }
}

fn command_session_stream_from_value(value: &Value) -> CommandSessionStream {
    value
        .as_str()
        .map(command_session_stream_from_name)
        .unwrap_or(CommandSessionStream::Stdout)
}

fn resolve_command_session_for_index(
    sessions: &mut HashMap<usize, CommandSessionTracker>,
    context: &ToolContext<'_>,
    command: &str,
    cwd: &str,
    command_index: usize,
) -> Option<CommandSessionTracker> {
    if let Some(existing) = sessions.get(&command_index) {
        return Some(existing.clone());
    }
    let tracker = CommandSessionTracker::start(
        context,
        command,
        cwd,
        command_index,
        Some("sandbox".to_string()),
        CommandSessionLaunchMode::Shell,
        false,
        false,
    )?;
    sessions.insert(command_index, tracker.clone());
    Some(tracker)
}

async fn execute_command_in_sandbox_streaming(
    context: &ToolContext<'_>,
    args: &Value,
    commands: &[String],
    cwd: &Path,
) -> Option<Value> {
    if !crate::sandbox::sandbox_enabled(context.config) {
        return None;
    }
    let cwd_text = cwd.to_string_lossy().to_string();
    let command_lookup = commands
        .iter()
        .enumerate()
        .map(|(index, command)| (index, command.clone()))
        .collect::<HashMap<usize, String>>();
    let mut sessions = HashMap::<usize, CommandSessionTracker>::new();
    let mut exited_sessions = HashSet::<usize>::new();
    let result = crate::sandbox::execute_command_streaming(
        context.config,
        context.workspace.as_ref(),
        context.user_id,
        context.workspace_id,
        context.session_id,
        args,
        context.user_tool_bindings,
        |event| {
            let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
            let command_index = event
                .get("command_index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0);
            let command = event
                .get("command")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| command_lookup.get(&command_index).cloned())
                .unwrap_or_default();
            match event_type {
                "command_start" => {
                    let _ = resolve_command_session_for_index(
                        &mut sessions,
                        context,
                        &command,
                        cwd_text.as_str(),
                        command_index,
                    );
                }
                "delta" => {
                    let Some(tracker) = resolve_command_session_for_index(
                        &mut sessions,
                        context,
                        &command,
                        cwd_text.as_str(),
                        command_index,
                    ) else {
                        return;
                    };
                    let delta = event
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .as_bytes()
                        .to_vec();
                    tracker.emit_delta(
                        command_session_stream_from_value(
                            event.get("stream").unwrap_or(&Value::Null),
                        ),
                        &delta,
                    );
                }
                "command_exit" => {
                    if exited_sessions.insert(command_index) {
                        if let Some(tracker) = resolve_command_session_for_index(
                            &mut sessions,
                            context,
                            &command,
                            cwd_text.as_str(),
                            command_index,
                        ) {
                            let exit_code = event
                                .get("exit_code")
                                .and_then(Value::as_i64)
                                .and_then(|value| i32::try_from(value).ok());
                            let timed_out = event
                                .get("timed_out")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let error = event
                                .get("error")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                                .filter(|value| !value.trim().is_empty());
                            tracker.emit_exit(exit_code, timed_out, error);
                        }
                    }
                }
                _ => {}
            }
        },
    )
    .await;

    for (command_index, tracker) in sessions.iter() {
        if !exited_sessions.contains(command_index) {
            tracker.emit_exit(
                None,
                false,
                Some("sandbox stream ended without exit".to_string()),
            );
        }
    }

    let mut result = result?;
    let session_ids = sessions
        .iter()
        .map(|(index, tracker)| (*index, tracker.command_session_id().to_string()))
        .collect::<HashMap<usize, String>>();
    if let Some(results) = result
        .get_mut("data")
        .and_then(|data| data.get_mut("results"))
        .and_then(Value::as_array_mut)
    {
        for item in results {
            let Some(obj) = item.as_object_mut() else {
                continue;
            };
            let command_index = obj
                .get("command_index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0);
            if let Some(command_session_id) = session_ids.get(&command_index) {
                obj.insert(
                    "command_session_id".to_string(),
                    Value::String(command_session_id.clone()),
                );
            }
        }
    }
    Some(result)
}

async fn execute_command_in_sandbox_streaming_auto(
    context: &ToolContext<'_>,
    args: &Value,
    content: &str,
) -> Option<Value> {
    if content.trim().is_empty() {
        return None;
    }
    if !crate::sandbox::sandbox_enabled(context.config) {
        return None;
    }
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let cwd = if workdir.is_empty() {
        match context.workspace.ensure_user_root(context.workspace_id) {
            Ok(path) => path,
            Err(_) => return None,
        }
    } else {
        match context
            .workspace
            .resolve_path(context.workspace_id, workdir)
        {
            Ok(path) => path,
            Err(_) => return None,
        }
    };
    let allow_all = context
        .config
        .security
        .allow_commands
        .iter()
        .any(|item| item == "*");
    let commands = if allow_all {
        vec![content.to_string()]
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    let result = execute_command_in_sandbox_streaming(context, args, &commands, &cwd).await?;
    let transport_failed = result
        .get("error")
        .and_then(Value::as_str)
        .map(|value| value == "sandbox stream request failed")
        .unwrap_or(false);
    if transport_failed {
        return None;
    }
    Some(result)
}

fn apply_streaming_command_env(cmd: &mut tokio::process::Command) {
    cmd.env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONLEGACYWINDOWSSTDIO", "utf-8");
}

#[allow(clippy::too_many_arguments)]
async fn read_stream_output<R>(
    mut reader: R,
    emitter: Option<ToolEventEmitter>,
    tool_name: String,
    command: String,
    stream_name: &'static str,
    chunk_size: usize,
    capture_policy: CommandOutputPolicy,
    command_session: Option<CommandSessionTracker>,
) -> Result<CommandOutputCapture>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let read_size = chunk_size.max(256);
    let mut buffer = vec![0u8; read_size];
    let mut collector = CommandOutputCollector::new(capture_policy);
    let stream_emitter = if command_session.is_some() {
        None
    } else {
        emitter.as_ref().filter(|item| item.stream_enabled())
    };
    let command_stream = command_session_stream_from_name(stream_name);

    let mut pending_bytes = Vec::new();
    let mut pending_text = String::new();
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        collector.push_chunk(chunk);
        if let Some(command_session) = command_session.as_ref() {
            command_session.emit_delta(command_stream, chunk);
        }
        if stream_emitter.is_some() {
            pending_bytes.extend_from_slice(chunk);
            loop {
                match std::str::from_utf8(&pending_bytes) {
                    Ok(valid) => {
                        if !valid.is_empty() {
                            pending_text.push_str(valid);
                        }
                        pending_bytes.clear();
                        break;
                    }
                    Err(err) => {
                        let valid_up_to = err.valid_up_to();
                        if valid_up_to == 0 {
                            break;
                        }
                        let valid = &pending_bytes[..valid_up_to];
                        let text = std::str::from_utf8(valid).unwrap_or_default();
                        if !text.is_empty() {
                            pending_text.push_str(text);
                        }
                        pending_bytes.drain(..valid_up_to);
                    }
                }
            }
            if let Some(stream_emitter) = stream_emitter {
                emit_tool_output_chunks(
                    stream_emitter,
                    &tool_name,
                    &command,
                    stream_name,
                    &mut pending_text,
                    chunk_size,
                    false,
                    command_session.as_ref(),
                );
            }
        }
    }

    if let Some(stream_emitter) = stream_emitter {
        if !pending_bytes.is_empty() {
            pending_text.push_str(decode_command_output(&pending_bytes).as_str());
            pending_bytes.clear();
        }
        emit_tool_output_chunks(
            stream_emitter,
            &tool_name,
            &command,
            stream_name,
            &mut pending_text,
            chunk_size,
            true,
            command_session.as_ref(),
        );
    }

    Ok(collector.finish())
}

struct CommandRunResult {
    returncode: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    stdout_capture: CommandOutputCaptureMeta,
    stderr_capture: CommandOutputCaptureMeta,
    command_session_id: Option<String>,
    running: bool,
}

/// 会话级取消令牌的登记守卫。前台阻塞命令在执行期间把令牌挂到 broker 上，
/// 「后台任务」面板的终止按钮由此能取消它；函数返回（含 `?` 提前返回）时
/// 摘除，令牌不会残留到同一线程的下一条命令上。
struct SessionCancelGuard {
    broker: Arc<super::command_sessions::CommandSessionBroker>,
    id: String,
}

impl SessionCancelGuard {
    fn register(
        broker: &Arc<super::command_sessions::CommandSessionBroker>,
        command_session_id: &str,
    ) -> (Self, CancellationToken) {
        let token = broker.register_cancel(command_session_id);
        (
            Self {
                broker: Arc::clone(broker),
                id: command_session_id.to_string(),
            },
            token,
        )
    }
}

impl Drop for SessionCancelGuard {
    fn drop(&mut self) {
        self.broker.unregister_cancel(&self.id);
    }
}

/// 取消 watcher 只在前台等待期间需要；等待结束（完成或转后台）就 abort，
/// 否则每个命令都会留下一个永远挂在 `cancelled()` 上的长任务。
struct CancelWatcherGuard(Option<tokio::task::JoinHandle<()>>);

impl Drop for CancelWatcherGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

pub(crate) fn compact_command_result_for_model(item: &Value) -> Value {
    let output_meta = item.get("output_meta").and_then(Value::as_object);
    let mut compacted = json!({
        "command": item.get("command").cloned().unwrap_or(Value::Null),
        "command_index": item.get("command_index").cloned().unwrap_or(Value::Null),
        "command_session_id": item.get("command_session_id").cloned().unwrap_or(Value::Null),
        "returncode": item.get("returncode").cloned().unwrap_or(Value::Null),
        "stdout": item.get("stdout").cloned().unwrap_or(Value::Null),
        "stderr": item.get("stderr").cloned().unwrap_or(Value::Null),
        "truncated": output_meta
            .and_then(|meta| meta.get("truncated"))
            .cloned()
            .unwrap_or(Value::Null),
        "total_bytes": output_meta
            .and_then(|meta| meta.get("total_bytes"))
            .cloned()
            .unwrap_or(Value::Null),
        "omitted_bytes": output_meta
            .and_then(|meta| meta.get("omitted_bytes"))
            .cloned()
            .unwrap_or(Value::Null),
    });
    // 后台首包的 seq 是模型续读的游标（命令会话的 after_seq），必须透传。
    if let Some(seq) = item.get("seq").filter(|value| !value.is_null()) {
        compacted["seq"] = seq.clone();
    }
    if let Some(warning) = item.get("warning").filter(|value| !value.is_null()) {
        compacted["warning"] = warning.clone();
    }
    if let Some(exit_note) = item.get("exit_note").filter(|value| !value.is_null()) {
        compacted["exit_note"] = exit_note.clone();
    }
    if let Some(shell_note) = item.get("shell_note").filter(|value| !value.is_null()) {
        compacted["shell_note"] = shell_note.clone();
    }
    if item
        .get("output_truncated_by_pipe")
        .and_then(Value::as_bool)
        == Some(true)
    {
        compacted["output_truncated_by_pipe"] = json!(true);
    }
    compacted
}

fn compact_command_results_for_model(items: &[Value]) -> Vec<Value> {
    items.iter().map(compact_command_result_for_model).collect()
}

/// 按引用收走一个读任务句柄；被 `timeout` 中断后 future 可恢复，句柄
/// 不会丢失，宽限轮空时仍可 abort。
async fn join_output_task(
    handle: &mut Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
) -> Result<CommandOutputCapture> {
    match handle.take() {
        Some(handle) => match handle.await {
            Ok(result) => result,
            Err(err) => Err(anyhow!(err.to_string())),
        },
        None => Ok(CommandOutputCapture::empty()),
    }
}

/// 等两个输出读任务到达 EOF。宽限窗口内读不到 EOF，说明还有进程持有
/// 管道写端（孙进程在 shell 退出/被杀后幸存）：先树终止强制放行，再给
/// 一个窗口；仍读不到就丢弃读任务。函数必然在有限时间内返回，管道收尾
/// 不允许绑架工具调用的返回。
///
/// abort 句柄必须在把 JoinHandle 交进 drain future 之前取出：
/// `join_output_task` 会 `take()` 走句柄，future 被丢弃时 JoinHandle
/// 只是 detach（tokio 语义），外层 Option 已经空了，事后拿不到句柄。
async fn drain_output_captures(
    stdout_task: &mut Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    stderr_task: &mut Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    tree: &ProcessTree,
) -> Result<(CommandOutputCapture, CommandOutputCapture)> {
    let stdout_abort = stdout_task.as_ref().map(|handle| handle.abort_handle());
    let stderr_abort = stderr_task.as_ref().map(|handle| handle.abort_handle());
    {
        let drain = async {
            let stdout = join_output_task(stdout_task).await?;
            let stderr = join_output_task(stderr_task).await?;
            Ok::<_, anyhow::Error>((stdout, stderr))
        };
        tokio::pin!(drain);
        for _ in 0..2 {
            match tokio::time::timeout(PIPE_EOF_GRACE, drain.as_mut()).await {
                Ok(result) => return result,
                Err(_elapsed) => tree.terminate(),
            }
        }
        // drain 与它对句柄的借用在此丢弃；读任务由下面的 abort 句柄收掉。
    }
    if let Some(handle) = stdout_abort {
        handle.abort();
    }
    if let Some(handle) = stderr_abort {
        handle.abort();
    }
    tracing::warn!(
        "command output pipes did not reach EOF after tree terminate; reader tasks aborted"
    );
    Ok((CommandOutputCapture::empty(), CommandOutputCapture::empty()))
}

/// `ExitStatus::code()` 对“被信号杀死”的进程返回 None，工具层会把所有
/// 信号退出都报成 -1，SIGPIPE（141）因此和未知失败混在一起。按 shell 的
/// 习惯补成 128+signal，至少能区分“下游截断”和真正的错误。
fn exit_status_code(status: &std::process::ExitStatus) -> Option<i32> {
    if let Some(code) = status.code() {
        return Some(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        return status.signal().map(|signal| 128 + signal);
    }
    #[cfg(not(unix))]
    {
        None
    }
}

pub fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    #[cfg(windows)]
    {
        if looks_like_utf16_output(bytes) {
            if let Some(text) = decode_utf16_output(bytes) {
                return text;
            }
        }
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let utf8_lossy = String::from_utf8_lossy(bytes).to_string();

    #[cfg(windows)]
    {
        let (decoded, _, _) = GBK.decode(bytes);
        let gbk_text = decoded.into_owned();
        if should_prefer_decoded_text(&gbk_text, &utf8_lossy) {
            return gbk_text;
        }

        if let Some(text) = decode_utf16_output(bytes) {
            if should_prefer_decoded_text(&text, &utf8_lossy) {
                return text;
            }
        }
    }

    utf8_lossy
}

#[cfg(windows)]
fn looks_like_utf16_output(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return true;
    }

    let odd_bytes = bytes.len() / 2;
    if odd_bytes == 0 {
        return false;
    }

    let zero_odd = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    zero_odd * 100 >= odd_bytes * 20
}

#[cfg(windows)]
fn should_prefer_decoded_text(candidate: &str, fallback: &str) -> bool {
    if candidate.trim().is_empty() {
        return false;
    }

    let candidate_replacement = candidate.chars().filter(|ch| *ch == '\u{FFFD}').count();
    let fallback_replacement = fallback.chars().filter(|ch| *ch == '\u{FFFD}').count();

    if candidate_replacement < fallback_replacement {
        return true;
    }
    if candidate_replacement > fallback_replacement {
        return false;
    }

    fallback_replacement > 0 && contains_cjk(candidate) && !contains_cjk(fallback)
}

#[cfg(windows)]
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\u{3400}'..='\u{4DBF}'
                | '\u{4E00}'..='\u{9FFF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{20000}'..='\u{2A6DF}'
                | '\u{2A700}'..='\u{2B73F}'
                | '\u{2B740}'..='\u{2B81F}'
                | '\u{2B820}'..='\u{2CEAF}'
        )
    })
}

#[cfg(windows)]
fn decode_utf16_output(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }

    let (is_big_endian, start) = if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, 2)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, 2)
    } else {
        (false, 0)
    };

    let payload = &bytes[start..];
    if payload.is_empty() || !payload.len().is_multiple_of(2) {
        return None;
    }

    let units = payload
        .chunks_exact(2)
        .map(|chunk| {
            if is_big_endian {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.trim_matches('\u{FEFF}').to_string())
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_spawned_child_streaming(
    context: &ToolContext<'_>,
    mut child: tokio::process::Child,
    tool_name: &str,
    command_text: &str,
    timeout: Option<Duration>,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
    command_session: Option<CommandSessionTracker>,
    cancel: Option<CancellationToken>,
    tree: ProcessTree,
) -> Result<CommandRunResult> {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut stdout_task = stdout.map(|stdout| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        let command_session = command_session.clone();
        long_task::spawn("tools.command.stdout_reader", async move {
            read_stream_output(
                stdout,
                emitter,
                tool_name,
                command_text,
                "stdout",
                chunk_size,
                stdout_policy,
                command_session,
            )
            .await
        })
    });
    let mut stderr_task = stderr.map(|stderr| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        let command_session = command_session.clone();
        long_task::spawn("tools.command.stderr_reader", async move {
            read_stream_output(
                stderr,
                emitter,
                tool_name,
                command_text,
                "stderr",
                chunk_size,
                stderr_policy,
                command_session,
            )
            .await
        })
    });

    let result = async {
        let mut timed_out = false;
        let mut cancelled = false;
        // 与 detached watcher 同一张选票：取消信号一到就杀子进程并收尾，
        // 不等超时也不等自然退出。无令牌一侧退化为永不就绪的等待。
        let cancel_wait = async {
            match cancel.as_ref() {
                Some(token) => token.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        };
        let status = tokio::select! {
            _ = cancel_wait => {
                cancelled = true;
                tree.terminate();
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
            result = async {
                if let Some(timeout) = timeout {
                    match tokio::time::timeout(timeout, child.wait()).await {
                        Ok(result) => Ok::<_, anyhow::Error>(Some(result?)),
                        Err(_) => {
                            timed_out = true;
                            tree.terminate();
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            Ok(None)
                        }
                    }
                } else {
                    Ok(Some(child.wait().await?))
                }
            } => result?,
        };

        let drained = drain_output_captures(&mut stdout_task, &mut stderr_task, &tree).await;
        // 进程已退出、输出已收干（或读任务已收掉）：解除 Drop 兜底，
        // 避免对已经消亡的 pid / 进程组再次终止。
        tree.disarm();
        let (stdout_capture, stderr_capture) = drained?;
        Ok::<_, anyhow::Error>((status, timed_out, cancelled, stdout_capture, stderr_capture))
    }
    .await;

    let (status, timed_out, cancelled, stdout_capture, stderr_capture) = match result {
        Ok(value) => value,
        Err(err) => {
            if let Some(command_session) = command_session.as_ref() {
                command_session.emit_exit(None, false, Some(err.to_string()));
            }
            return Err(err);
        }
    };
    let stdout = render_command_output(&stdout_capture, decode_command_output);
    let mut stderr = render_command_output(&stderr_capture, decode_command_output);
    if cancelled {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str("command cancelled");
    }
    let exit_code = status.as_ref().and_then(exit_status_code);
    let returncode = exit_code.unwrap_or(-1);
    if let Some(command_session) = command_session.as_ref() {
        command_session.emit_exit(
            exit_code,
            timed_out,
            cancelled.then(|| "command cancelled".to_string()),
        );
    }

    Ok(CommandRunResult {
        returncode,
        stdout,
        stderr,
        timed_out,
        stdout_capture: stdout_capture.meta,
        stderr_capture: stderr_capture.meta,
        command_session_id: command_session.map(|item| item.command_session_id().to_string()),
        running: false,
    })
}

/// 让进程脱离这次工具调用：子进程、输出读任务与进程树守卫全部交给一个
/// 监视任务持有，工具调用只等一条「最终结果」通知。
///
/// 这样即使上层看门狗把工具调用打断（future 被丢弃），进程也不会陪葬：
/// 它继续留在命令会话里，可查询、可等待、可终止。反过来，工具在等待窗口
/// 内正常拿到结果时，语义与原来的前台阻塞路径完全一致。
#[allow(clippy::too_many_arguments)]
async fn supervise_spawned_child(
    broker: Arc<super::command_sessions::CommandSessionBroker>,
    mut child: Child,
    timeout: Option<Duration>,
    command_session: CommandSessionTracker,
    mut stdout_task: Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    mut stderr_task: Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    tree: ProcessTree,
    child_stdin: Option<tokio::process::ChildStdin>,
    result_tx: tokio::sync::oneshot::Sender<Result<CommandRunResult>>,
) -> Result<()> {
    let session_id = command_session.command_session_id().to_string();
    let cleanup_session_id = session_id.clone();
    // 写端来自调用方提前 take 出来的那一份；tokio 的 wait 不会关掉它。
    let process = Arc::new(CommandProcessHandle::new(child_stdin));
    if !broker.register_process(&session_id, Arc::clone(&process)) {
        tree.terminate();
        let _ = child.kill().await;
        let _ = child.wait().await;
        command_session.emit_exit(
            None,
            false,
            Some("active command session limit reached".to_string()),
        );
        return Err(anyhow!("active command session limit reached"));
    }
    let tracker = command_session.clone();
    let cancel = process.cancellation_token();
    long_task::spawn("tools.command.detached", async move {
        let (wait_result, timed_out, error) = if let Some(timeout) = timeout {
            tokio::select! {
                _ = cancel.cancelled() => { tree.terminate(); let _ = child.kill().await; let _ = child.wait().await; (None, false, Some("command cancelled".to_string())) }
                result = tokio::time::timeout(timeout, child.wait()) => match result {
                    Ok(Ok(status)) => (Some(status), false, None),
                    Ok(Err(error)) => (None, false, Some(error.to_string())),
                    Err(_) => { tree.terminate(); let _ = child.kill().await; let _ = child.wait().await; (None, true, None) }
                }
            }
        } else {
            tokio::select! {
                _ = cancel.cancelled() => { tree.terminate(); let _ = child.kill().await; let _ = child.wait().await; (None, false, Some("command cancelled".to_string())) }
                result = child.wait() => match result {
                    Ok(status) => (Some(status), false, None),
                    Err(error) => (None, false, Some(error.to_string()))
                }
            }
        };
        // 自然退出也可能留下持有管道的孙进程；宽限内读不到 EOF 就树终止，
        // 保证 watcher 一定收尾、broker 槽位一定释放。
        let finished = complete_detachable_child(
            wait_result,
            timed_out,
            &mut stdout_task,
            &mut stderr_task,
            &tree,
            tracker,
            error,
        )
        .await;
        broker.remove_process(&cleanup_session_id);
        // 调用方可能早已离开（工具超时/取消），发送失败是正常情况。
        let _ = result_tx.send(finished);
    });
    Ok(())
}

fn start_command_output_readers(
    context: &ToolContext<'_>,
    child: &mut Child,
    tool_name: &str,
    command_text: &str,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
    command_session: CommandSessionTracker,
) -> (
    Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
) {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let stdout_task = child.stdout.take().map(|stdout| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        let command_session = command_session.clone();
        long_task::spawn("tools.command.stdout_reader", async move {
            read_stream_output(
                stdout,
                emitter,
                tool_name,
                command_text,
                "stdout",
                chunk_size,
                stdout_policy,
                Some(command_session),
            )
            .await
        })
    });
    let stderr_task = child.stderr.take().map(|stderr| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        long_task::spawn("tools.command.stderr_reader", async move {
            read_stream_output(
                stderr,
                emitter,
                tool_name,
                command_text,
                "stderr",
                chunk_size,
                stderr_policy,
                Some(command_session),
            )
            .await
        })
    });
    (stdout_task, stderr_task)
}

async fn complete_detachable_child(
    status: Option<std::process::ExitStatus>,
    timed_out: bool,
    stdout_task: &mut Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    stderr_task: &mut Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
    tree: &ProcessTree,
    command_session: CommandSessionTracker,
    exit_error: Option<String>,
) -> Result<CommandRunResult> {
    let drained = drain_output_captures(stdout_task, stderr_task, tree).await;
    tree.disarm();
    let (stdout_capture, stderr_capture) = drained?;
    let stdout = render_command_output(&stdout_capture, decode_command_output);
    let mut stderr = render_command_output(&stderr_capture, decode_command_output);
    if let Some(error) = exit_error.as_deref() {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(error);
    }
    let exit_code = status.as_ref().and_then(exit_status_code);
    command_session.emit_exit(exit_code, timed_out, exit_error);
    Ok(CommandRunResult {
        returncode: exit_code.unwrap_or(-1),
        stdout,
        stderr,
        timed_out,
        stdout_capture: stdout_capture.meta,
        stderr_capture: stderr_capture.meta,
        command_session_id: Some(command_session.command_session_id().to_string()),
        running: false,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_command_streaming(
    context: &ToolContext<'_>,
    command: &str,
    cwd: &Path,
    timeout: Option<Duration>,
    tool_name: &str,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
    command_index: usize,
    yield_time: Option<Duration>,
    background: bool,
    notifier: Option<Arc<CommandNotifier>>,
    force_shell_launch: bool,
) -> Result<CommandRunResult> {
    let command_text = command.to_string();
    let command_env = python_runtime::resolve_desktop_command_env();
    let command_overrides = command_utils::CommandProgramOverrides {
        pip_bin: command_env.command_overrides.pip_bin.clone(),
        git_bin: command_env.command_overrides.git_bin.clone(),
        rg_bin: command_env.command_overrides.rg_bin.clone(),
    };
    // A shell script override means the host shell is a gateway into another
    // runtime (安卓版's Ubuntu). Direct-spawning `uname` or `git` would silently
    // bypass that runtime and answer from Android/toybox instead.
    // `force_shell_launch` covers the other case where the text is known to be a
    // script rather than a single program: an unwrapped nested PowerShell
    // wrapper (`Write-Output ...` has no cmdlet on PATH, so a direct spawn would
    // fail once before falling back to the shell).
    let force_shell =
        force_shell_launch || std::env::var_os(command_utils::SHELL_SCRIPT_ENV).is_some();
    let shell_command =
        || command_utils::build_shell_command_with_shell(command, cwd, &command_env.shell);
    let (mut cmd, used_direct) = if force_shell {
        (shell_command(), false)
    } else if let Some(cmd) = command_utils::build_direct_command_with_overrides(
        command,
        cwd,
        command_env
            .python_runtime
            .as_ref()
            .map(|runtime| runtime.bin.as_path()),
        command_overrides,
    ) {
        (cmd, true)
    } else if let Some(cmd) = command_utils::build_direct_command(command, cwd) {
        (cmd, true)
    } else {
        (shell_command(), false)
    };
    python_runtime::apply_desktop_command_env(&mut cmd, &command_env);
    apply_streaming_command_env(&mut cmd);
    let initial_launch_mode = if used_direct {
        CommandSessionLaunchMode::Direct
    } else {
        CommandSessionLaunchMode::Shell
    };
    let initial_shell_name =
        (!used_direct).then(|| command_utils::shell_display_name(&command_env.shell));
    // 只有会转后台会话（可 write_stdin）的命令才需要 stdin 管道。前台阻塞
    // 命令保持管道开着，等 stdin 的程序（凭据询问、input()、交互安装器）
    // 只能永远挂住——这就是 pwsh「挂死」的常见起点。null 让它们立刻 EOF。
    // 判定必须与下面的 detach 条件完全一致：yield_time 为 0 时走的是前台
    // 阻塞路径，不能因为 Some(0) 就留下一个永不关闭的 stdin 写端。
    let detach_capable = (background || yield_time.is_some_and(|wait| wait > Duration::ZERO))
        && context.command_sessions.is_some();
    cmd.kill_on_drop(true);
    cmd.stdin(if detach_capable {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let (mut child, launch_mode, shell_name) = match cmd.spawn() {
        Ok(child) => (
            child,
            if used_direct {
                CommandSessionLaunchMode::Direct
            } else {
                CommandSessionLaunchMode::Shell
            },
            (!used_direct).then(|| command_utils::resolve_shell_name(&command_env.shell)),
        ),
        Err(err) if used_direct && command_utils::is_not_found_error(&err) => {
            let mut cmd =
                command_utils::build_shell_command_with_shell(command, cwd, &command_env.shell);
            python_runtime::apply_desktop_command_env(&mut cmd, &command_env);
            apply_streaming_command_env(&mut cmd);
            cmd.kill_on_drop(true);
            cmd.stdin(if detach_capable {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
            let fallback_shell_name = command_utils::shell_display_name(&command_env.shell);
            match cmd.spawn() {
                Ok(child) => (
                    child,
                    CommandSessionLaunchMode::Shell,
                    Some(fallback_shell_name),
                ),
                Err(fallback_err) => {
                    if let Some(command_session) = CommandSessionTracker::start(
                        context,
                        &command_text,
                        &cwd.to_string_lossy(),
                        command_index,
                        Some(fallback_shell_name),
                        CommandSessionLaunchMode::Shell,
                        false,
                        false,
                    ) {
                        command_session.emit_failed_to_start(fallback_err.to_string());
                    }
                    return Err(anyhow!(fallback_err));
                }
            }
        }
        Err(err) => {
            if let Some(command_session) = CommandSessionTracker::start(
                context,
                &command_text,
                &cwd.to_string_lossy(),
                command_index,
                initial_shell_name.clone(),
                initial_launch_mode,
                false,
                false,
            ) {
                command_session.emit_failed_to_start(err.to_string());
            }
            return Err(anyhow!(err));
        }
    };
    // 关键：stdin 写端必须自己拿着，不能让 tokio 的 `Child::wait` 代管。
    // tokio 在开始 wait 时会主动关掉未被取走的 stdin（避免子进程等待输入
    // 永不退出），于是「先等 100ms 再转后台」的命令一进入等待就再也收不到
    // write_stdin 的输入——命令读到 EOF 直接结束，模型还以为它挂了。
    // 提前 take 出来，等待期间写端仍由我们持有，转后台时原样交给会话句柄。
    let child_stdin = child.stdin.take();
    // 树守卫挂得越早，覆盖孙进程越完整（Job 成员随创建继承）。
    let tree = ProcessTree::attach(&child);
    let command_session = CommandSessionTracker::start(
        context,
        &command_text,
        &cwd.to_string_lossy(),
        command_index,
        shell_name,
        launch_mode,
        false,
        false,
    );
    let Some(command_session) = command_session else {
        return run_spawned_child_streaming(
            context,
            child,
            tool_name,
            &command_text,
            timeout,
            stdout_policy,
            stderr_policy,
            None,
            None,
            tree,
        )
        .await;
    };
    if !background && !yield_time.is_some_and(|wait| wait > Duration::ZERO) {
        // 前台阻塞路径：执行期间登记会话级取消令牌，面板的「终止」由此取消它，
        // 守卫在函数返回时摘除。
        let (_cancel_guard, session_cancel) = context
            .command_sessions
            .as_ref()
            .map(|broker| {
                SessionCancelGuard::register(broker, command_session.command_session_id())
            })
            .unzip();
        return run_spawned_child_streaming(
            context,
            child,
            tool_name,
            &command_text,
            timeout,
            stdout_policy,
            stderr_policy,
            Some(command_session),
            session_cancel,
            tree,
        )
        .await;
    }
    // 可脱离路径必须有 broker 才能接管进程；拿不到就退回前台阻塞，语义不变。
    let Some(broker) = context.command_sessions.as_ref().cloned() else {
        return run_spawned_child_streaming(
            context,
            child,
            tool_name,
            &command_text,
            timeout,
            stdout_policy,
            stderr_policy,
            Some(command_session),
            None,
            tree,
        )
        .await;
    };
    // 读任务先起，勤输出的进程不会在初次等待期间把管道写满。
    let (stdout_task, stderr_task) = start_command_output_readers(
        context,
        &mut child,
        tool_name,
        &command_text,
        stdout_policy,
        stderr_policy,
        command_session.clone(),
    );
    // 进程从这一刻起由监视任务持有：工具调用被打断也不再连坐子进程。
    let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
    let session_id = command_session.command_session_id().to_string();
    supervise_spawned_child(
        broker,
        child,
        timeout,
        command_session,
        stdout_task,
        stderr_task,
        tree,
        child_stdin,
        result_tx,
    )
    .await?;
    if background {
        // 后台模式不等窗口：已经跑完的直接交完整结果，仍在跑的交给完成通知。
        match result_rx.try_recv() {
            Ok(result) => return result,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                if let Some(notifier) = notifier {
                    let notify_session_id = session_id.clone();
                    let started_at = context
                        .command_sessions
                        .as_ref()
                        .and_then(|broker| broker.snapshot(&notify_session_id))
                        .map(|snapshot| snapshot.started_at.timestamp_millis() as f64 / 1000.0)
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as f64 / 1000.0);
                    notifier
                        .persist_pending(
                            &notify_session_id,
                            &command_text,
                            &cwd.to_string_lossy(),
                            context.workspace_id,
                            "local",
                            started_at,
                        )
                        .await;
                    long_task::spawn("tools.command.completion_notify", async move {
                        // 监督任务退出前已写入会话终态；等它结束后直接读快照。
                        let _ = result_rx.await;
                        schedule_snapshot_notification(&Some(notifier), &notify_session_id).await;
                    });
                }
                return Ok(running_command_result(session_id));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                // 监督任务异常提前结束：仍按运行中交还会话，让会话守卫收尾。
                return Ok(running_command_result(session_id));
            }
        }
    }
    let wait = yield_time.unwrap_or_else(|| Duration::from_millis(DEFAULT_COMMAND_YIELD_MS));
    // 等待窗口内退出就把完整结果交回去；窗口到点则只交会话，进程继续跑。
    match tokio::time::timeout(wait, &mut result_rx).await {
        Ok(Ok(result)) => return result,
        Ok(Err(_)) => {}
        Err(_) => {
            // 恰好在窗口末尾退出：给收尾一点余量，避免把已完成报成运行中。
            if let Ok(Ok(result)) =
                tokio::time::timeout(Duration::from_millis(500), &mut result_rx).await
            {
                return result;
            }
        }
    }
    Ok(running_command_result(session_id))
}

fn running_command_result(session_id: String) -> CommandRunResult {
    CommandRunResult {
        returncode: -1,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        stdout_capture: CommandOutputCaptureMeta::empty(),
        stderr_capture: CommandOutputCaptureMeta::empty(),
        command_session_id: Some(session_id),
        running: true,
    }
}

/// `命令会话` 不带 id 时：列出本线程的命令任务。
///
/// 这是「到底跑没跑」的只读答案。工具调用被超时打断、消息里没拿到会话 id，
/// 或者模型只是想确认后台还有哪些进程时，用它代替 `ps` 与反复猜。
fn command_session_list(context: &ToolContext<'_>) -> Value {
    let Some(broker) = context.command_sessions.as_ref() else {
        return build_failed_tool_result(
            &i18n::t("tool.command_session.unavailable"),
            json!({}),
            ToolErrorMeta::new("TOOL_COMMAND_SESSION_UNAVAILABLE", None, false, None),
            false,
        );
    };
    let now = chrono::Utc::now();
    let mut sessions = broker
        .list_session_snapshots(context.user_id, context.session_id)
        .into_iter()
        .map(|snapshot| {
            let view = snapshot.task_view(now);
            json!({
                "command_session_id": view.id,
                "status": view.status,
                "running": view.running,
                "exit_code": view.exit_code,
                "timed_out": snapshot.timed_out,
                "command": view.command,
                "cwd": view.cwd,
                "elapsed_ms": view.elapsed_ms,
                "seq": snapshot.seq,
                "stdout_tail": tail_excerpt(&snapshot.stdout_tail),
                "stderr_tail": tail_excerpt(&snapshot.stderr_tail),
                "output_truncated": snapshot.stdout_dropped_bytes > 0
                    || snapshot.stderr_dropped_bytes > 0
                    || snapshot.pty_dropped_bytes > 0,
            })
        })
        .collect::<Vec<_>>();
    // 还在跑的排前面：模型最需要知道的就是「现在有什么在跑」。
    sessions.sort_by(|left, right| {
        let running = |value: &Value| value.get("running").and_then(Value::as_bool) != Some(true);
        running(left).cmp(&running(right)).then_with(|| {
            let elapsed = |value: &Value| {
                value
                    .get("elapsed_ms")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            };
            elapsed(right).cmp(&elapsed(left))
        })
    });
    let total = sessions.len();
    let running = sessions
        .iter()
        .filter(|value| value.get("running").and_then(Value::as_bool) == Some(true))
        .count();
    sessions.truncate(20);
    build_model_tool_success(
        "command_session",
        "listed",
        i18n::t("tool.command_session.list_summary"),
        json!({
            "sessions": sessions,
            "total": total,
            "running": running,
            "next_step": i18n::t("tool.command_session.list_next_step"),
        }),
    )
}

/// 会话尾部输出只带最后一小段：够看清进度，不把上下文撑爆。
fn tail_excerpt(text: &str) -> String {
    const MAX_CHARS: usize = 1_200;
    let trimmed = text.trim_end();
    let count = trimmed.chars().count();
    if count <= MAX_CHARS {
        return trimmed.to_string();
    }
    let tail: String = trimmed.chars().skip(count - MAX_CHARS).collect();
    format!("…{}", tail)
}

/// 后台命令会话的唯一入口：空 input 读新输出，非空 input 写 stdin 后再读，
/// Ctrl-C 中断。设计对齐 codex 的 unified exec：
///
/// - 一次调用只有「读一眼」和「写一次」两种意图，不再额外要求 action；
/// - 空输入等待新增输出，非空输入写 stdin 后在同一窗口等待新增输出；
/// - 会话已退出不是错误，仍然返回最后一次输出，避免模型拿不到结果；
/// - 没有可写 stdin 时给出可执行的替代做法，而不是让模型反复重试。
pub(crate) async fn command_session_control(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let session_id = args
        .get("command_session_id")
        .or_else(|| args.get("session_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if session_id.is_empty() {
        return Ok(command_session_list(context));
    }
    let input = parse_command_session_input(args);
    let wait = parse_command_session_wait(args);
    let yield_time_ms = wait.yield_time_ms;
    // 普通写入与轮询只等待新增输出；中断和 EOF 首次调用就等待会话收尾，
    // 避免 EOT 已关闭 stdin、调用方却先看到 running 而误以为必须再发一次。
    let wait_for_exit = wait.wait_for_exit
        || matches!(
            input,
            CommandSessionInput::Interrupt | CommandSessionInput::Eof
        );
    let after_seq = args.get("after_seq").and_then(Value::as_u64);
    let broker = context.command_sessions.as_ref();
    let managed = broker.is_some_and(|broker| {
        broker
            .snapshot_for_scope(context.user_id, context.session_id, session_id)
            .is_some()
    });
    let sandbox_backed = session_id.starts_with("sandcmd_");
    // 非 tty 会话没有可写 stdin，中断必须落到终止上；沙盒会话的输入要回到
    // 沙盒侧，本地没有它的子进程句柄。
    let mut stdin_note: Option<String> = None;
    if matches!(input, CommandSessionInput::Interrupt) {
        if let Some(broker) = broker {
            broker.terminate_task(context.user_id, context.session_id, session_id);
            // 模型主动终止：完成通知无需再报告一次取消。
            broker.mark_model_observed(context.user_id, context.session_id, session_id);
        }
        if sandbox_backed
            && !crate::sandbox::cancel_command_session(
                context.config,
                context.user_id,
                context.session_id,
                session_id,
            )
            .await
        {
            return Ok(build_failed_tool_result(
                &i18n::t("tool.command_session.sandbox_unavailable"),
                json!({ "command_session_id": session_id }),
                ToolErrorMeta::new("TOOL_COMMAND_SESSION_UNAVAILABLE", None, true, Some(200)),
                false,
            ));
        }
        stdin_note = Some(i18n::t("tool.command_session.interrupted"));
    }
    if sandbox_backed && (input.uses_stdin_control() || !managed) {
        let Some(result) = crate::sandbox::control_command_session(
            context.config,
            context.user_id,
            context.session_id,
            session_id,
            match input {
                CommandSessionInput::Text(text) => text,
                CommandSessionInput::Eof => "\x04",
                _ => "",
            },
            crate::sandbox::CommandSessionPollOptions {
                after_seq: if managed { 0 } else { after_seq.unwrap_or(0) },
                yield_time_ms: if managed { 0 } else { yield_time_ms },
                wait_for_exit,
                include_snapshot: !managed,
            },
            input.uses_stdin_control(),
        )
        .await
        else {
            return Ok(build_failed_tool_result(
                &i18n::t("tool.command_session.sandbox_unavailable"),
                json!({ "command_session_id": session_id }),
                ToolErrorMeta::new("TOOL_COMMAND_SESSION_UNAVAILABLE", None, true, Some(200)),
                false,
            ));
        };
        if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let error = result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if error.contains("stdin is unavailable") {
                return Ok(build_failed_tool_result(
                    &i18n::t("tool.command_session.stdin_closed"),
                    json!({
                        "command_session_id": session_id,
                        "stdin_note": i18n::t("tool.command_session.stdin_closed_hint"),
                    }),
                    ToolErrorMeta::new(
                        "TOOL_COMMAND_STDIN_CLOSED",
                        Some(i18n::t("tool.command_session.stdin_closed_hint")),
                        false,
                        None,
                    ),
                    false,
                ));
            }
            return Ok(result);
        }
        if !managed {
            let mut data = compact_sandbox_poll_result(
                result.get("data").cloned().unwrap_or_else(|| json!({})),
            );
            if matches!(input, CommandSessionInput::Interrupt)
                && data
                    .get("error")
                    .and_then(Value::as_str)
                    .is_some_and(|error| error.starts_with("command cancelled"))
            {
                if let Value::Object(ref mut map) = data {
                    map.insert("error".to_string(), Value::Null);
                    map.insert("interrupted".to_string(), Value::Bool(true));
                }
            }
            if data.get("status").and_then(Value::as_str) != Some("running") {
                if let Some(broker) = broker {
                    broker.mark_model_observed(context.user_id, context.session_id, session_id);
                }
            }
            return Ok(build_command_session_result(data));
        }
    }
    let Some(broker) = broker else {
        return Ok(build_failed_tool_result(
            &i18n::t("tool.command_session.unavailable"),
            json!({}),
            ToolErrorMeta::new("TOOL_COMMAND_SESSION_UNAVAILABLE", None, false, None),
            false,
        ));
    };
    if matches!(
        input,
        CommandSessionInput::Text(_) | CommandSessionInput::Eof
    ) && !sandbox_backed
    {
        let operation = match input {
            CommandSessionInput::Text(text) => {
                broker
                    .write_stdin(
                        context.user_id,
                        context.session_id,
                        session_id,
                        text.as_bytes(),
                    )
                    .await
            }
            CommandSessionInput::Eof => {
                broker
                    .close_stdin(context.user_id, context.session_id, session_id)
                    .await
            }
            _ => unreachable!(),
        }
        .map_err(|error| anyhow!(error))?;
        match operation {
            CommandStdinWrite::Written => {}
            CommandStdinWrite::StdinClosed => {
                let data = broker
                    .poll(
                        context.user_id,
                        context.session_id,
                        session_id,
                        after_seq,
                        Duration::ZERO,
                        false,
                    )
                    .await
                    .map_err(|error| anyhow!(error))?;
                return Ok(build_failed_tool_result(
                    &i18n::t("tool.command_session.stdin_closed"),
                    with_stdin_note(data, i18n::t("tool.command_session.stdin_closed_hint")),
                    ToolErrorMeta::new(
                        "TOOL_COMMAND_STDIN_CLOSED",
                        Some(i18n::t("tool.command_session.stdin_closed_hint")),
                        false,
                        None,
                    ),
                    false,
                ));
            }
            CommandStdinWrite::SessionExited => {
                stdin_note = Some(i18n::t("tool.command_session.stdin_after_exit"));
            }
        }
    }
    let data = broker
        .poll(
            context.user_id,
            context.session_id,
            session_id,
            after_seq,
            Duration::from_millis(yield_time_ms),
            wait_for_exit,
        )
        .await
        .map_err(|error| anyhow!(error))?;
    let mut data = data;
    // 模型自己要求的中断不是故障：把终止事件换算成「已中断」的成功结果，
    // 只保留它关心的状态与已产出的输出。
    if matches!(input, CommandSessionInput::Interrupt)
        && data
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.starts_with("command cancelled"))
    {
        if let Value::Object(ref mut map) = data {
            map.insert("error".to_string(), Value::Null);
            map.insert("interrupted".to_string(), Value::Bool(true));
        }
    }
    let mut data = match stdin_note {
        Some(note) => with_stdin_note(data, note),
        None => data,
    };
    if let CommandSessionInput::Text(text) = input {
        data["interaction_input"] = Value::String(text.to_string());
    }
    Ok(build_command_session_result(data))
}

/// 输入侧的情况单独占一个字段：`next_step` 已被轮询提示占用。
fn with_stdin_note(mut data: Value, note: String) -> Value {
    if let Value::Object(ref mut map) = data {
        map.insert("stdin_note".to_string(), Value::String(note));
    }
    data
}

fn command_session_failure_message(data: &Value) -> String {
    let code = data.get("exit_code").and_then(Value::as_i64);
    let detail = data
        .get("error")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            data.get("stderr")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .and_then(|text| text.lines().rev().find(|line| !line.trim().is_empty()))
        })
        .map(str::trim)
        .unwrap_or_default();
    let timed_out = data.get("timed_out").and_then(Value::as_bool) == Some(true);
    match (timed_out, code) {
        (true, _) => "Command session timed out; partial output is preserved.".to_string(),
        (false, Some(code)) if !detail.is_empty() => {
            format!("Command failed with exit code {code}: {detail}")
        }
        (false, Some(code)) => format!("Command failed with exit code {code}."),
        (false, None) if !detail.is_empty() => format!("Command failed: {detail}"),
        (false, None) => "Command failed; inspect status and incremental output.".to_string(),
    }
}

fn build_command_session_result(mut data: Value) -> Value {
    let running = data.get("status").and_then(Value::as_str) == Some("running");
    if !running
        && data
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.starts_with("command cancelled"))
    {
        if let Value::Object(ref mut map) = data {
            map.insert("error".to_string(), Value::Null);
            map.insert("interrupted".to_string(), Value::Bool(true));
        }
    }
    if !running
        && (data.get("timed_out").and_then(Value::as_bool) == Some(true)
            || data
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|error| !error.is_empty())
            || data
                .get("exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code < 0))
    {
        return build_failed_tool_result(
            command_session_failure_message(&data),
            data,
            ToolErrorMeta::new(
                "TOOL_COMMAND_SESSION_FAILED",
                Some(
                    "用同一 command_session_id 可继续读取增量输出；stderr 已包含在数据中。"
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        );
    }
    // 正的非零退出码随数据返回：命令确实执行完了，退出语义由模型结合输出判断。
    if !running
        && data
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code > 0)
    {
        data["exit_note"] = json!(i18n::t("tool.exec.non_zero_exit_note"));
    }
    if running {
        data["next_step"] = json!(i18n::t("tool.command_session.keep_waiting"));
    }
    build_model_tool_success(
        "command_session",
        if running { "running" } else { "completed" },
        i18n::t("tool.command_session.summary"),
        data,
    )
}

/// The model consumes each output byte once. Keep cumulative snapshots in the
/// broker/UI; do not send both the delta array and its text to the model.
fn compact_sandbox_poll_result(mut data: Value) -> Value {
    if let Value::Object(ref mut map) = data {
        let dropped = map.get("dropped").and_then(Value::as_bool) == Some(true);
        let deltas = map.remove("deltas");
        let mut stdout = String::new();
        let mut stderr = String::new();
        if !dropped {
            for delta in deltas
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(text) = delta.get("delta").and_then(Value::as_str) {
                    if delta.get("stream").and_then(Value::as_str) == Some("stderr") {
                        stderr.push_str(text);
                    } else {
                        stdout.push_str(text);
                    }
                }
            }
        }
        if dropped {
            map.insert("output_truncated".into(), Value::Bool(true));
        } else {
            map.insert("stdout".into(), Value::String(stdout));
            map.insert("stderr".into(), Value::String(stderr));
        }
    }
    data
}

/// A single runtime task owns remote output forwarding. Model polls only read
/// the local broker, so they cannot duplicate events or delay live output.
/// 启动恢复复用的 observer 入口：与正常后台会话共用同一套输出转发、退出结算
/// 与完成通知逻辑。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_sandbox_command_observer(
    config: Config,
    user_id: String,
    session_id: String,
    tracker: CommandSessionTracker,
    broker: Arc<super::command_sessions::CommandSessionBroker>,
    notifier: Option<Arc<CommandNotifier>>,
    cancel: tokio_util::sync::CancellationToken,
    start_seq: u64,
) {
    long_task::spawn(
        "tools.sandbox_command_output",
        observe_sandbox_command(
            config, user_id, session_id, tracker, broker, notifier, cancel, start_seq,
        ),
    );
}

#[allow(clippy::too_many_arguments)]
async fn observe_sandbox_command(
    config: Config,
    user_id: String,
    session_id: String,
    tracker: CommandSessionTracker,
    broker: Arc<super::command_sessions::CommandSessionBroker>,
    notifier: Option<Arc<CommandNotifier>>,
    cancel: tokio_util::sync::CancellationToken,
    start_seq: u64,
) {
    let id = tracker.command_session_id().to_string();
    let mut after_seq = start_seq;
    let mut failures = 0;
    loop {
        let result = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = crate::sandbox::cancel_command_session(&config, &user_id, &session_id, &id).await;
                tracker.emit_exit(None, false, Some("command cancelled".to_string()));
                schedule_snapshot_notification(&notifier, &id).await;
                break;
            }
            result = crate::sandbox::control_command_session(
                &config, &user_id, &session_id, &id, "",
                crate::sandbox::CommandSessionPollOptions {
                    after_seq, yield_time_ms: MAX_COMMAND_SESSION_POLL_MS,
                    ..Default::default()
                }, false,
            ) => result,
        };
        let data = result
            .as_ref()
            .filter(|value| value.get("ok").and_then(Value::as_bool) == Some(true))
            .and_then(|value| value.get("data"));
        let Some(data) = data else {
            failures += 1;
            if failures >= 3 {
                let _ = crate::sandbox::cancel_command_session(&config, &user_id, &session_id, &id)
                    .await;
                tracker.emit_exit(
                    None,
                    false,
                    Some("sandbox command output unavailable".to_string()),
                );
                schedule_snapshot_notification(&notifier, &id).await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };
        failures = 0;
        if data.get("dropped").and_then(Value::as_bool) == Some(true) {
            broker.mark_output_gap(&id);
        }
        emit_sandbox_output(&tracker, data, after_seq);
        after_seq = after_seq.max(data.get("seq").and_then(Value::as_u64).unwrap_or(after_seq));
        if data.get("status").and_then(Value::as_str) != Some("running") {
            tracker.emit_exit(
                data.get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok()),
                data.get("timed_out")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                data.get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            );
            schedule_snapshot_notification(&notifier, &id).await;
            break;
        }
    }
    broker.remove_process(&id);
}

fn emit_sandbox_output(tracker: &CommandSessionTracker, data: &Value, after_seq: u64) {
    for delta in data
        .get("deltas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if delta.get("seq").and_then(Value::as_u64).unwrap_or(0) > after_seq {
            if let Some(text) = delta.get("delta").and_then(Value::as_str) {
                tracker.emit_delta(
                    command_session_stream_from_value(delta.get("stream").unwrap_or(&Value::Null)),
                    text.as_bytes(),
                );
            }
        }
    }
}

/// Block until a sandbox background command exits, using the session
/// poll-with-wait loop (Notify-driven on the sandbox side) instead of sleeps.
///
/// 到点仍未退出时**不杀进程**：返回 `running` 数据与已消费的输出序号，由
/// 调用方转入后台会话继续轮询。渲染这类长命令的超时不应等于失败重跑。
async fn await_sandbox_command_exit(
    context: &ToolContext<'_>,
    command_session_id: &str,
    timeout_s: f64,
    tracker: Option<&CommandSessionTracker>,
) -> Option<(Value, u64)> {
    // HTTP 客户端已单独保留网络余量；这里只计用户请求的等待窗口，
    // 否则任何较短的 yield_time_ms 都会被隐性放大到约 15 秒。
    let deadline =
        (timeout_s > 0.0).then(|| std::time::Instant::now() + Duration::from_secs_f64(timeout_s));
    let mut after_seq = 0u64;
    let mut dropped = false;
    loop {
        let yield_time_ms = deadline
            .map(|deadline| {
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis() as u64
            })
            .unwrap_or(MAX_COMMAND_SESSION_POLL_MS)
            .clamp(1, MAX_COMMAND_SESSION_POLL_MS);
        let result = crate::sandbox::control_command_session(
            context.config,
            context.user_id,
            context.session_id,
            command_session_id,
            "",
            crate::sandbox::CommandSessionPollOptions {
                after_seq,
                yield_time_ms,
                include_snapshot: true,
                ..Default::default()
            },
            false,
        )
        .await?;
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        let mut data = result.get("data")?.clone();
        if let Some(tracker) = tracker {
            emit_sandbox_output(tracker, &data, after_seq);
        }
        dropped |= data
            .get("dropped")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        after_seq = after_seq.max(data.get("seq").and_then(Value::as_u64).unwrap_or(after_seq));
        let running = data.get("status").and_then(Value::as_str) == Some("running");
        let deadline_exceeded =
            deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline);
        if running && !deadline_exceeded {
            continue;
        }
        let stdout = data.get("stdout").cloned().unwrap_or_else(|| json!(""));
        let stderr = data.get("stderr").cloned().unwrap_or_else(|| json!(""));
        let output_truncated = dropped
            || data
                .get("output_truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        if let Value::Object(ref mut map) = data {
            map.remove("deltas");
            map.insert("stdout".into(), stdout);
            map.insert("stderr".into(), stderr);
            map.insert("output_truncated".into(), json!(output_truncated));
            map.insert("dropped".into(), json!(dropped));
            if running {
                map.insert("status".into(), json!("running"));
                map.insert("reason".into(), json!("yielded"));
                map.insert("yield_time_ms".into(), json!((timeout_s * 1000.0) as u64));
                map.insert("poll_after_seq".into(), json!(after_seq));
            }
        }
        if running {
            // 会话仍在后台运行：不要发退出事件，后续 observer 接着推送增量。
            return Some((data, after_seq));
        }
        if let Some(tracker) = tracker {
            tracker.emit_exit(
                data.get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok()),
                data.get("timed_out")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                data.get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            );
        }
        return Some((data, after_seq));
    }
}

/// Build the model-facing result for a finished sandbox command, mirroring the
/// local blocking semantics: timeout is a failure; a non-zero exit code is
/// command semantics carried in the data, not a tool failure.
fn build_sandbox_command_result(mut data: Value) -> Value {
    if let Value::Object(ref mut map) = data {
        map.remove("deltas");
    }
    let timed_out = data
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if timed_out {
        let outcome = command_timeout_outcome(&data);
        data["execution_outcome"] = json!(outcome);
        if outcome == "likely_applied" {
            data["next_step_hint"] = json!(
                "命令在超时前已产生输出，说明它确实执行过；重试前先核实是否已经产生副作用（文件、进程、远端状态）。"
            );
        }
        return build_failed_tool_result(
            i18n::t("tool.exec.timeout"),
            data,
            tool_timeout_meta("TOOL_EXEC_TIMEOUT"),
            false,
        );
    }
    let exit_code = data.get("exit_code").and_then(Value::as_i64);
    let stderr_text = data
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let command_text = data
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if is_broken_pipe_exit(
        command_text,
        exit_code
            .and_then(|code| i32::try_from(code).ok())
            .unwrap_or(-1),
        stderr_text,
    ) {
        if let Value::Object(ref mut map) = data {
            map.insert(
                "warning".to_string(),
                json!("管道被下游提前关闭（SIGPIPE/EPIPE），输出已按下游需要截断；这不是命令本身的失败。"),
            );
            map.insert("output_truncated_by_pipe".to_string(), json!(true));
        }
        return build_model_tool_success(
            "execute_command",
            "completed",
            "Sandbox command output was truncated by a downstream pipe.",
            data,
        );
    }
    match exit_code {
        Some(0) => {}
        Some(code) if code > 0 => {
            // 正的非零退出码是命令语义，不是工具失败；保留输出由模型判断。
            if let Value::Object(ref mut map) = data {
                map.insert(
                    "exit_note".to_string(),
                    json!(i18n::t("tool.exec.non_zero_exit_note")),
                );
            }
        }
        _ => {
            // 负退出码（被信号终止/取消）或缺失退出码：命令没有正常执行完。
            let detail = data
                .get("error")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    data.get("stderr")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                })
                .map(str::trim)
                .map(ToString::to_string)
                .unwrap_or_else(|| "command was terminated before exiting".to_string());
            return build_failed_tool_result(
                detail,
                data,
                ToolErrorMeta::new(
                    "TOOL_EXEC_TERMINATED",
                    Some("命令被信号终止或取消，没有正常执行完。".to_string()),
                    false,
                    None,
                ),
                false,
            );
        }
    }
    build_model_tool_success(
        "execute_command",
        "completed",
        "Sandbox command completed.",
        data,
    )
}

pub(crate) async fn execute_command(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    let dry_run = parse_dry_run(&args);
    let command_budget = parse_command_budget(&args);
    let content = args
        .get("content")
        .or_else(|| args.get("cmd"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let run_in_background = args
        .get("run_in_background")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(patch_input) = extract_direct_patch_from_command(&content) {
        // Route accidental inline patch payloads to apply_patch to keep edit semantics stable.
        let payload = json!({
            "input": patch_input,
            "dry_run": dry_run,
        });
        let mut result = apply_patch_tool::apply_patch(context, &payload).await?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "intercepted_from".to_string(),
                Value::String("execute_command".to_string()),
            );
        }
        if !dry_run {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }
    // 裸 sleep 在后台是合法作业（延迟执行）；只有前台空等才拦截。
    if !run_in_background && content_is_bare_sleep(&content) {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.sleep_not_allowed"),
            json!({ "command": content }),
            ToolErrorMeta::new(
                "TOOL_EXEC_SLEEP_NOT_ALLOWED",
                Some("不要用 sleep 空等：未结束的命令会返回 command_session_id，请用 write_stdin 等待输出。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    // dsh parity: the model supplies a short human-readable `description`
    // (5-10 words) that serves as the UI title for this command call. Surface it
    // on every command-session event emitted for this invocation.
    let scoped_context = args
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|description| {
            context.event_emitter.as_ref().map(|emitter| {
                context.with_event_emitter(Some(emitter.with_field(
                    "command_description",
                    Value::String(description.to_string()),
                )))
            })
        });
    let context = scoped_context.as_ref().unwrap_or(context);
    let background_notifier = if run_in_background {
        CommandNotifier::from_context(context)
    } else {
        None
    };
    if run_in_background && !dry_run && background_notifier.is_none() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.background_unavailable"),
            json!({ "run_in_background": true }),
            ToolErrorMeta::new(
                "TOOL_EXEC_BACKGROUND_UNAVAILABLE",
                Some(
                    "当前运行环境没有命令会话或线程唤醒能力，请去掉 run_in_background 后前台执行。"
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }
    if run_in_background
        && !dry_run
        && !context
            .command_sessions
            .as_ref()
            .is_some_and(|broker| broker.has_process_capacity())
    {
        return Ok(build_failed_tool_result(
            "command process capacity reached",
            json!({ "run_in_background": true }),
            ToolErrorMeta::new("TOOL_COMMAND_SESSION_CAPACITY", None, true, Some(500)),
            false,
        ));
    }
    // 后台名额满时退回前台阻塞：短命令照常跑完，长命令由自己的 timeout_s
    // 回收并如实报超时，不会出现「进程跑着、会话却登记不上」的半截状态。
    let yield_time = context
        .command_sessions
        .as_ref()
        .filter(|broker| broker.has_process_capacity())
        .map(|_| parse_command_yield_time(&args));
    let timeout_s = parse_timeout_secs(args.get("timeout_s"))
        .map(|value| value.max(0.0))
        .unwrap_or(DEFAULT_COMMAND_TIMEOUT_S);
    let timeout_s = apply_time_budget_secs(timeout_s, &command_budget);
    let timeout_s = if timeout_s > 0.0 {
        timeout_s.min(MAX_COMMAND_TIMEOUT_S)
    } else {
        DEFAULT_COMMAND_TIMEOUT_S
    };
    // 等待窗口与进程寿命分离：到点返回会话，不把仍在工作的进程误判为超时。
    // 后台模式同样要按长寿命处理，否则默认 120s 会把仍在跑的任务误杀。
    let process_timeout_s = if run_in_background || yield_time.is_some() {
        INTERACTIVE_COMMAND_MAX_S
    } else {
        timeout_s
    };
    let timeout = Some(Duration::from_secs_f64(process_timeout_s));
    if crate::sandbox::sandbox_enabled(context.config) && !dry_run && !content.is_empty() {
        // 沙盒启动接口按 yield_time_ms 决定首次返回前等多久；后台模式尽快交还会话，
        // 是否结束由常驻 observer 负责。
        let mut sandbox_args = args.clone();
        if run_in_background {
            if let Value::Object(map) = &mut sandbox_args {
                map.insert("run_in_background".to_string(), json!(true));
                map.insert("yield_time_ms".to_string(), json!(50));
                // 服务端对后台会话只按显式 timeout_s 计时；把工具层的长寿命
                // 窗口传过去，避免后台任务被默认 120s 误杀。
                map.insert("timeout_s".to_string(), json!(process_timeout_s));
            }
        }
        if let Some(result) = crate::sandbox::launch_command_session(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            &sandbox_args,
            context.user_tool_bindings,
        )
        .await
        {
            if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                context.workspace.mark_tree_dirty(context.workspace_id);
                let mut data = result.get("data").cloned().unwrap_or_else(|| json!({}));
                let command_session_id = data
                    .get("command_session_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let tracker = command_session_id.as_ref().and_then(|command_session_id| {
                    CommandSessionTracker::start_with_id(
                        context,
                        Some(command_session_id.clone()),
                        &content,
                        args.get("workdir").and_then(Value::as_str).unwrap_or(""),
                        0,
                        Some("sandbox".to_string()),
                        CommandSessionLaunchMode::Shell,
                        false,
                        false,
                    )
                });
                let mut completed = data
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status.eq_ignore_ascii_case("exited"));
                let mut escalated_after_seq: Option<u64> = None;
                if completed {
                    if let Some(tracker) = tracker.as_ref() {
                        if let Some(stderr) = data.get("stderr").and_then(Value::as_str) {
                            tracker.emit_delta(CommandSessionStream::Stderr, stderr.as_bytes());
                        }
                        if let Some(stdout) = data.get("stdout").and_then(Value::as_str) {
                            tracker.emit_delta(CommandSessionStream::Stdout, stdout.as_bytes());
                        }
                        tracker.emit_exit(
                            data.get("exit_code")
                                .and_then(Value::as_i64)
                                .and_then(|value| i32::try_from(value).ok()),
                            data.get("timed_out")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            data.get("error")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                        );
                    }
                } else {
                    // Every command shares the same short initial wait. A still-running
                    // process remains attached to its session instead of being killed.
                    let Some(command_session_id) = command_session_id.as_deref() else {
                        return Ok(build_failed_tool_result(
                            "sandbox command session id is missing",
                            json!({}),
                            ToolErrorMeta::new(
                                "TOOL_COMMAND_SESSION_UNAVAILABLE",
                                Some("沙盒未返回 command_session_id，请重试。".to_string()),
                                true,
                                Some(500),
                            ),
                            false,
                        ));
                    };
                    // 后台模式立即交还会话，由常驻 observer 盯退出并触发通知。
                    if !run_in_background {
                        // 阻塞等待期间也要能被「终止」：把会话级取消令牌挂到
                        // broker，用户停止/线程取消时由 watcher 取消远端会话，
                        // 不能等到工具超时才发现命令还在跑。
                        let (_sandbox_cancel_guard, sandbox_cancel) = context
                            .command_sessions
                            .as_ref()
                            .map(|broker| SessionCancelGuard::register(broker, command_session_id))
                            .unzip();
                        let _cancel_watcher = CancelWatcherGuard(sandbox_cancel.map(|cancel| {
                            let cancel_config = context.config.clone();
                            let cancel_user = context.user_id.to_string();
                            let cancel_session = context.session_id.to_string();
                            let cancel_id = command_session_id.to_string();
                            long_task::spawn("tools.sandbox_command_cancel", async move {
                                cancel.cancelled().await;
                                let _ = crate::sandbox::cancel_command_session(
                                    &cancel_config,
                                    &cancel_user,
                                    &cancel_session,
                                    &cancel_id,
                                )
                                .await;
                            })
                        }));
                        match await_sandbox_command_exit(
                            context,
                            command_session_id,
                            yield_time
                                .map(|wait| wait.as_secs_f64())
                                .unwrap_or(DEFAULT_COMMAND_YIELD_MS as f64 / 1000.0),
                            tracker.as_ref(),
                        )
                        .await
                        {
                            Some((final_data, resume_after_seq)) => {
                                let still_running =
                                    final_data.get("status").and_then(Value::as_str)
                                        == Some("running");
                                data = final_data;
                                completed = !still_running;
                                if still_running {
                                    escalated_after_seq = Some(resume_after_seq);
                                }
                            }
                            None => {
                                return Ok(build_failed_tool_result(
                                    "sandbox command session poll failed",
                                    json!({ "command_session_id": command_session_id }),
                                    ToolErrorMeta::new(
                                        "TOOL_COMMAND_SESSION_UNAVAILABLE",
                                        Some("轮询沙盒命令会话失败，可用命令会话工具按该 command_session_id 重试。".to_string()),
                                        true,
                                        Some(500),
                                    ),
                                    false,
                                ));
                            }
                        }
                    }
                }
                if completed {
                    return Ok(build_sandbox_command_result(data));
                }
                if let (Some(tracker), Some(broker)) = (tracker, context.command_sessions.as_ref())
                {
                    let handle = Arc::new(CommandProcessHandle::new(None));
                    if !broker.register_process(tracker.command_session_id(), Arc::clone(&handle)) {
                        let _ = crate::sandbox::cancel_command_session(
                            context.config,
                            context.user_id,
                            context.session_id,
                            tracker.command_session_id(),
                        )
                        .await;
                        tracker.emit_exit(
                            None,
                            false,
                            Some("command process capacity reached".to_string()),
                        );
                        return Ok(build_failed_tool_result(
                            "command process capacity reached",
                            data,
                            ToolErrorMeta::new(
                                "TOOL_COMMAND_SESSION_CAPACITY",
                                None,
                                true,
                                Some(500),
                            ),
                            false,
                        ));
                    }
                    if run_in_background {
                        if let Some(notifier) = background_notifier.as_ref() {
                            let record_session_id = tracker.command_session_id().to_string();
                            let started_at = broker
                                .snapshot(&record_session_id)
                                .map(|snapshot| {
                                    snapshot.started_at.timestamp_millis() as f64 / 1000.0
                                })
                                .unwrap_or_else(|| {
                                    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
                                });
                            notifier
                                .persist_pending(
                                    &record_session_id,
                                    &content,
                                    args.get("workdir").and_then(Value::as_str).unwrap_or(""),
                                    context.workspace_id,
                                    "sandbox",
                                    started_at,
                                )
                                .await;
                        }
                    }
                    long_task::spawn(
                        "tools.sandbox_command_output",
                        observe_sandbox_command(
                            context.config.clone(),
                            context.user_id.to_string(),
                            context.session_id.to_string(),
                            tracker,
                            Arc::clone(broker),
                            background_notifier.clone(),
                            handle.cancellation_token(),
                            escalated_after_seq.unwrap_or(0),
                        ),
                    );
                }
                let guidance = if run_in_background {
                    i18n::t("tool.exec.background_notify_next_step")
                } else {
                    i18n::t("tool.exec.background_next_step")
                };
                if let Value::Object(ref mut map) = data {
                    map.insert("background".to_string(), json!(run_in_background));
                    map.insert("next_step".to_string(), Value::String(guidance.clone()));
                    if !run_in_background {
                        map.insert(
                            "yield_time_ms".to_string(),
                            json!(yield_time.map(|wait| wait.as_millis())),
                        );
                    }
                }
                let message = if run_in_background {
                    format!("Command started in the background. {guidance}")
                } else {
                    format!("Command is still running. {guidance}")
                };
                return Ok(build_model_tool_success(
                    "execute_command",
                    "running",
                    message,
                    data,
                ));
            }
            return Ok(result);
        }
    }
    if let Some(result) = execute_in_sandbox(context, "执行命令", &args).await {
        if !dry_run {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }

    if content.is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.command_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_EXEC_COMMAND_REQUIRED",
                Some("请在 content 中提供要执行的命令或脚本文本。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, &content);

    let allow_commands = &context.config.security.allow_commands;
    let allow_all = allow_commands.iter().any(|item| item == "*");
    let normalized_allow_commands = if allow_all {
        Vec::new()
    } else {
        allow_commands
            .iter()
            .map(|item| item.trim().to_lowercase())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    };
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let cwd = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.workspace_id)?
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };
    if !cwd.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.workdir_not_found"),
            json!({ "workdir": workdir }),
            ToolErrorMeta::new(
                "TOOL_EXEC_WORKDIR_NOT_FOUND",
                Some("请确认 workdir 路径存在且在允许范围内。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    if !cwd.is_dir() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.workdir_not_dir"),
            json!({ "workdir": workdir }),
            ToolErrorMeta::new(
                "TOOL_EXEC_WORKDIR_NOT_DIR",
                Some("请将 workdir 指向目录而非文件。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    let mut results = Vec::new();
    let mut guarded_total_bytes: usize = 0;
    let mut guarded_omitted_bytes: usize = 0;
    let mut guarded_total_commands: usize = 0;
    let mut guarded_truncated_commands: usize = 0;
    // 只有在 PowerShell 语境下才处理「再包一层 powershell」：cmd 与 POSIX 下
    // 那个子进程是合法用法，改动它等于改语义。
    let nested_powershell_enabled = python_runtime::resolve_desktop_command_env().shell.kind
        == command_utils::ShellKind::PowerShell;
    let execute_tool_name = resolve_tool_name("execute_command");
    let (stdout_policy, stderr_policy) =
        derive_capture_policies(command_budget.output_budget_bytes);
    let effective_output_budget_bytes = stdout_policy
        .max_bytes()
        .saturating_add(stderr_policy.max_bytes());
    // 后台模式把整段文本当成一条脚本作业：顺序语义不变，只有一个会话、
    // 一个退出码和一条完成通知；前台仍按行拆分，保留逐条输出与失败处理。
    let commands = build_command_plan(&content, allow_all, run_in_background);
    // 单条命令才做「已有一模一样的会话在跑」的复用检查；批量脚本按顺序照跑。
    let single_command = commands.len() == 1;
    if commands.is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.command_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_EXEC_COMMAND_REQUIRED",
                Some("请在 content 中提供要执行的命令或脚本文本。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    if let Some(max_commands) = command_budget.max_commands {
        if commands.len() > max_commands {
            return Ok(build_failed_tool_result(
                format!(
                    "command count {} exceeds budget limit {}",
                    commands.len(),
                    max_commands
                ),
                json!({
                    "command_count": commands.len(),
                    "max_commands": max_commands,
                }),
                ToolErrorMeta::new(
                    "TOOL_EXEC_BUDGET_COMMAND_LIMIT",
                    Some("请减少单次执行命令数量，或提高 max_commands 预算。".to_string()),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    }
    if dry_run {
        return Ok(build_model_tool_success(
            "execute_command",
            "dry_run",
            "Validated command plan without execution.",
            json!({
                "dry_run": true,
                "workdir": cwd.to_string_lossy().to_string(),
                "command_count": commands.len(),
                "commands": commands,
                "timeout_s": timeout_s,
                "budget": command_budget.to_json(),
                "output_guard": {
                    "default_total_bytes": DEFAULT_CAPTURE_TOTAL_BYTES,
                    "effective_total_bytes": effective_output_budget_bytes,
                },
                "sandbox": false,
            }),
        ));
    }
    // 整段脚本必须逐行通过白名单，不能只检查第一行；后台作业会交给 shell
    // 顺序执行，越权行同样会被放出去。
    if run_in_background && !allow_all {
        for line in content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let lower = line.to_lowercase();
            if !normalized_allow_commands
                .iter()
                .any(|item| lower.starts_with(item))
            {
                return Ok(build_failed_tool_result(
                    i18n::t("tool.exec.not_allowed"),
                    json!({ "command": line }),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_NOT_ALLOWED",
                        Some("后台脚本里的每一行都必须在 allow_commands 白名单内。".to_string()),
                        false,
                        None,
                    ),
                    false,
                ));
            }
        }
    }
    for (command_index, command) in commands.into_iter().enumerate() {
        if command.trim().is_empty() {
            continue;
        }
        if !allow_all {
            let lower = command.to_lowercase();
            if !normalized_allow_commands
                .iter()
                .any(|item| lower.starts_with(item))
            {
                return Ok(build_failed_tool_result(
                    i18n::t("tool.exec.not_allowed"),
                    json!({
                        "command": command,
                    }),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_NOT_ALLOWED",
                        Some("命令不在 allow_commands 白名单内。".to_string()),
                        false,
                        None,
                    ),
                    false,
                ));
            }
        }
        // 模型习惯把整条命令再包一层 `powershell -Command "..."`。当前 shell 本来
        // 就是 pwsh 时，这层包装只会多出一次文本解析：`$var`/`$_` 被外层提前展开，
        // `\"` 与 `` `" `` 被外层吃掉。冗余包装直接剥掉；显式指向系统 Windows
        // PowerShell 的调用保留子进程，但把内层脚本改走 -EncodedCommand。
        let mut shell_note: Option<String> = None;
        let mut force_shell_launch = false;
        let command = if nested_powershell_enabled {
            match command_utils::rewrite_nested_powershell(&command) {
                Some(command_utils::NestedPowerShell::Unwrap(script)) => {
                    shell_note = Some(i18n::t("tool.exec.shell_unwrapped"));
                    // 剥出来的是脚本文本，不是单个程序：直接交给 shell，别先试一次
                    // 直启（`Write-Output` 不是可执行文件，只会多一次失败）。
                    force_shell_launch = true;
                    script
                }
                Some(command_utils::NestedPowerShell::Recode(text)) => {
                    shell_note = Some(i18n::t("tool.exec.shell_reencoded"));
                    text
                }
                None => command,
            }
        } else {
            command
        };
        // 同一条命令已经在本线程跑着：直接交回那条会话，不重复启动。
        // 渲染器、桌面应用这类单实例程序第二次启动只会被自己拦住，模型再
        // 靠 ps 判断"到底跑没跑"，白白丢掉一轮。
        if !run_in_background && single_command && yield_time.is_some() {
            if let Some(snapshot) = running_duplicate_session(context, &command, &cwd) {
                context.workspace.mark_tree_dirty(context.workspace_id);
                return Ok(build_reused_command_result(context, &snapshot));
            }
        }
        let run = run_command_streaming(
            context,
            &command,
            &cwd,
            timeout,
            &execute_tool_name,
            stdout_policy,
            stderr_policy,
            command_index,
            if run_in_background { None } else { yield_time },
            run_in_background,
            background_notifier.clone(),
            force_shell_launch,
        )
        .await?;
        let command_total_bytes = run
            .stdout_capture
            .total_bytes
            .saturating_add(run.stderr_capture.total_bytes);
        let command_omitted_bytes = run
            .stdout_capture
            .omitted_bytes
            .saturating_add(run.stderr_capture.omitted_bytes);
        let command_truncated = run.stdout_capture.truncated || run.stderr_capture.truncated;
        guarded_total_bytes = guarded_total_bytes.saturating_add(command_total_bytes);
        guarded_omitted_bytes = guarded_omitted_bytes.saturating_add(command_omitted_bytes);
        guarded_total_commands = guarded_total_commands.saturating_add(1);
        if command_truncated {
            guarded_truncated_commands = guarded_truncated_commands.saturating_add(1);
        }
        let mut result_entry = json!({
            "command": command,
            "command_index": command_index,
            "command_session_id": run.command_session_id,
            "returncode": (!run.running).then_some(run.returncode),
            "stdout": run.stdout,
            "stderr": run.stderr,
            "output_meta": {
                "truncated": command_truncated,
                "total_bytes": command_total_bytes,
                "omitted_bytes": command_omitted_bytes,
                "stdout": run.stdout_capture.to_json(),
                "stderr": run.stderr_capture.to_json(),
            },
        });
        if run.returncode == 0 && !run.stderr.trim().is_empty() {
            result_entry["warning"] = Value::String(
                "退出码为 0 但 stderr 非空；若命令包含管道，PowerShell/Cmd 下中间命令的失败可能不影响退出码，请检查 stderr 确认结果可信。"
                    .to_string(),
            );
        }
        if let Some(note) = shell_note {
            result_entry["shell_note"] = Value::String(note);
        }
        results.push(result_entry);
        if run.running {
            context.workspace.mark_tree_dirty(context.workspace_id);
            let guidance = if run_in_background {
                i18n::t("tool.exec.background_notify_next_step")
            } else {
                i18n::t("tool.exec.background_next_step")
            };
            let session_id = run.command_session_id.clone().unwrap_or_default();
            let partial_output = context.command_sessions.as_ref().and_then(|broker| {
                broker.snapshot_for_scope(context.user_id, context.session_id, &session_id)
            });
            if let (Some(snapshot), Some(last)) = (
                partial_output.as_ref(),
                results.last_mut().and_then(Value::as_object_mut),
            ) {
                last.insert("running".to_string(), Value::Bool(true));
                last.insert("stdout".to_string(), json!(snapshot.stdout_tail));
                last.insert("stderr".to_string(), json!(snapshot.stderr_tail));
                // 首包展示到 seq 为止的输出；模型游标同步推进，下一次会话轮询
                // 只返回新增输出。否则整段内容会在第一次轮询时被重放一遍。
                last.insert("seq".to_string(), json!(snapshot.seq));
                let tail_truncated = snapshot.stdout_dropped_bytes > 0
                    || snapshot.stderr_dropped_bytes > 0
                    || snapshot.pty_dropped_bytes > 0;
                if !tail_truncated {
                    if let Some(broker) = context.command_sessions.as_ref() {
                        broker.advance_model_cursor(
                            context.user_id,
                            context.session_id,
                            &session_id,
                            snapshot.seq,
                        );
                    }
                }
            }
            let mut data = json!({
                "command_session_id": session_id,
                "status": "running",
                "background": run_in_background,
                "next_step": guidance,
                "results": compact_command_results_for_model(&results),
            });
            if !run_in_background {
                data["yield_time_ms"] = json!(parse_command_yield_time(&args).as_millis());
            }
            let message = if run_in_background {
                format!("Command started in the background. {guidance}")
            } else {
                format!("Command is still running. {guidance}")
            };
            return Ok(build_model_tool_success(
                "execute_command",
                "running",
                message,
                data,
            ));
        }
        if run.timed_out {
            let detail = if timeout_s > 0.0 {
                format!("timeout after {timeout_s}s")
            } else {
                "timeout".to_string()
            };
            if let Some(last) = results.last_mut().and_then(Value::as_object_mut) {
                let previous = last
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let merged = if previous.trim().is_empty() {
                    detail.clone()
                } else {
                    format!("{previous}\n{detail}")
                };
                last.insert("stderr".to_string(), Value::String(merged));
            }
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(build_failed_tool_result(
                build_execute_command_failure_message(&results, true),
                build_execute_command_failure_data(
                    &results,
                    guarded_total_commands,
                    guarded_truncated_commands > 0,
                    guarded_omitted_bytes,
                    true,
                ),
                tool_timeout_meta("TOOL_EXEC_TIMEOUT"),
                false,
            ));
        }
        if is_broken_pipe_exit(&command, run.returncode, &run.stderr) {
            // 下游 head/cut 提前关闭管道是正常截断，不是命令失败；保留退出码
            // 与提示，继续执行后续命令。
            if let Some(last) = results.last_mut().and_then(Value::as_object_mut) {
                last.insert(
                    "warning".to_string(),
                    Value::String(
                        "管道被下游提前关闭（SIGPIPE/EPIPE），输出已按下游需要截断；这不是命令本身的失败，需要全量输出时去掉 head/cut 等截断。"
                            .to_string(),
                    ),
                );
                last.insert("output_truncated_by_pipe".to_string(), Value::Bool(true));
            }
            continue;
        }
        if run.returncode < 0 {
            // 负退出码表示被信号终止或用户取消：命令没有正常执行完，是真正的失败。
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(build_failed_tool_result(
                build_execute_command_failure_message(&results, false),
                build_execute_command_failure_data(
                    &results,
                    guarded_total_commands,
                    guarded_truncated_commands > 0,
                    guarded_omitted_bytes,
                    false,
                ),
                ToolErrorMeta::new(
                    "TOOL_EXEC_TERMINATED",
                    Some("命令被信号终止或取消，没有正常执行完。".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
        if run.returncode > 0 {
            // 正的非零退出码是命令语义（grep 无匹配、pkill 无进程、diff 有差异），
            // 不是工具执行失败；保留完整输出与退出码，由模型判断是否需要修正。
            if let Some(last) = results.last_mut().and_then(Value::as_object_mut) {
                last.insert(
                    "exit_note".to_string(),
                    Value::String(i18n::t("tool.exec.non_zero_exit_note")),
                );
            }
        }
    }
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(build_model_tool_success_with_hint(
        "execute_command",
        "completed",
        format!("Executed {guarded_total_commands} commands."),
        json!({
            "results": compact_command_results_for_model(&results),
            "budget": command_budget.to_json(),
            "output_guard": {
                "truncated": guarded_truncated_commands > 0,
                "commands": guarded_total_commands,
                "truncated_commands": guarded_truncated_commands,
                "total_bytes": guarded_total_bytes,
                "omitted_bytes": guarded_omitted_bytes,
                "effective_total_bytes": effective_output_budget_bytes,
            },
            "sandbox": false,
        }),
        (guarded_truncated_commands > 0).then(|| {
            "Command output was truncated by the output guard. Narrow the command or raise output_budget_bytes only if needed."
                .to_string()
        }),
    ))
}
