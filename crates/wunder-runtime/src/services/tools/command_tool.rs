use super::{
    apply_patch_tool, build_model_tool_success, build_model_tool_success_with_hint,
    command_options::{apply_time_budget_secs, parse_command_budget, parse_dry_run},
    command_output_guard::{
        derive_capture_policies, render_command_output, CommandOutputCapture,
        CommandOutputCaptureMeta, CommandOutputCollector, CommandOutputPolicy,
        DEFAULT_CAPTURE_TOTAL_BYTES, STDERR_CAPTURE_POLICY, STDOUT_CAPTURE_POLICY,
    },
    command_sessions::{CommandSessionLaunchMode, CommandSessionStream, CommandSessionTracker},
    execute_in_sandbox, recover_tool_args_value, resolve_tool_name,
    tool_error::{
        build_execute_command_failure_data, build_execute_command_failure_message,
        build_failed_tool_result, ToolErrorMeta,
    },
    ToolContext, ToolEventEmitter, LOCAL_PTC_DIR_NAME, LOCAL_PTC_TIMEOUT_S,
};
use crate::command_utils;
use crate::config::Config;
use crate::core::long_task;
use crate::core::python_runtime;
use crate::i18n;
use anyhow::{anyhow, Result};
#[cfg(windows)]
use encoding_rs::GBK;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
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
}

pub(crate) fn compact_command_result_for_model(item: &Value) -> Value {
    let output_meta = item.get("output_meta").and_then(Value::as_object);
    json!({
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
    })
}

fn compact_command_results_for_model(items: &[Value]) -> Vec<Value> {
    items.iter().map(compact_command_result_for_model).collect()
}

async fn join_output_task(
    handle: Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
) -> Result<CommandOutputCapture> {
    match handle {
        Some(handle) => match handle.await {
            Ok(result) => result,
            Err(err) => Err(anyhow!(err.to_string())),
        },
        None => Ok(CommandOutputCapture::empty()),
    }
}

pub(crate) fn decode_command_output(bytes: &[u8]) -> String {
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
) -> Result<CommandRunResult> {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = stdout.map(|stdout| {
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
    let stderr_task = stderr.map(|stderr| {
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
        let status = if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, child.wait()).await {
                Ok(result) => Some(result?),
                Err(_) => {
                    timed_out = true;
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    None
                }
            }
        } else {
            Some(child.wait().await?)
        };

        let stdout_capture = join_output_task(stdout_task).await?;
        let stderr_capture = join_output_task(stderr_task).await?;
        Ok::<_, anyhow::Error>((status, timed_out, stdout_capture, stderr_capture))
    }
    .await;

    let (status, timed_out, stdout_capture, stderr_capture) = match result {
        Ok(value) => value,
        Err(err) => {
            if let Some(command_session) = command_session.as_ref() {
                command_session.emit_exit(None, false, Some(err.to_string()));
            }
            return Err(err);
        }
    };
    let stdout = render_command_output(&stdout_capture, decode_command_output);
    let stderr = render_command_output(&stderr_capture, decode_command_output);
    let exit_code = status.and_then(|value| value.code());
    let returncode = exit_code.unwrap_or(-1);
    if let Some(command_session) = command_session.as_ref() {
        command_session.emit_exit(exit_code, timed_out, None);
    }

    Ok(CommandRunResult {
        returncode,
        stdout,
        stderr,
        timed_out,
        stdout_capture: stdout_capture.meta,
        stderr_capture: stderr_capture.meta,
        command_session_id: command_session.map(|item| item.command_session_id().to_string()),
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
) -> Result<CommandRunResult> {
    let command_text = command.to_string();
    let command_env = python_runtime::resolve_desktop_command_env();
    let command_overrides = command_utils::CommandProgramOverrides {
        pip_bin: command_env.command_overrides.pip_bin.clone(),
        git_bin: command_env.command_overrides.git_bin.clone(),
        rg_bin: command_env.command_overrides.rg_bin.clone(),
    };
    let (mut cmd, used_direct) = if let Some(cmd) =
        command_utils::build_direct_command_with_overrides(
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
        (command_utils::build_shell_command(command, cwd), false)
    };
    python_runtime::apply_desktop_command_env(&mut cmd, &command_env);
    apply_streaming_command_env(&mut cmd);
    let initial_launch_mode = if used_direct {
        CommandSessionLaunchMode::Direct
    } else {
        CommandSessionLaunchMode::Shell
    };
    let initial_shell_name =
        (!used_direct).then(|| command_utils::resolve_shell_name(command).to_string());
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let (child, launch_mode, shell_name) = match cmd.spawn() {
        Ok(child) => (
            child,
            if used_direct {
                CommandSessionLaunchMode::Direct
            } else {
                CommandSessionLaunchMode::Shell
            },
            (!used_direct).then(|| command_utils::resolve_shell_name(command).to_string()),
        ),
        Err(err) if used_direct && command_utils::is_not_found_error(&err) => {
            let mut cmd = command_utils::build_shell_command(command, cwd);
            python_runtime::apply_desktop_command_env(&mut cmd, &command_env);
            apply_streaming_command_env(&mut cmd);
            cmd.kill_on_drop(true);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            let fallback_shell_name = command_utils::resolve_shell_name(command).to_string();
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
    run_spawned_child_streaming(
        context,
        child,
        tool_name,
        &command_text,
        timeout,
        stdout_policy,
        stderr_policy,
        command_session,
    )
    .await
}

async fn run_ptc_python_script_streaming(
    context: &ToolContext<'_>,
    script_path: &Path,
    workdir: &Path,
    timeout: Option<Duration>,
) -> Result<CommandRunResult> {
    #[cfg(windows)]
    let candidates: &[(&str, &[&str])] = &[("py", &["-3"]), ("python", &[]), ("python3", &[])];
    #[cfg(not(windows))]
    let candidates: &[(&str, &[&str])] = &[("python3", &[]), ("python", &[])];

    let tool_name = resolve_tool_name("ptc");
    let script_text = script_path.to_string_lossy().to_string();
    let mut last_error: Option<anyhow::Error> = None;
    let mut tried = Vec::new();

    if let Some(runtime) = python_runtime::resolve_python_runtime() {
        let program = runtime.bin.to_string_lossy().to_string();
        tried.push(program.clone());
        let mut cmd = tokio::process::Command::new(&program);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        apply_streaming_command_env(&mut cmd);
        python_runtime::apply_python_env(&mut cmd, &runtime);
        command_utils::apply_platform_spawn_options(&mut cmd);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let command_text = format!("{program} {script_text}");
        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                    STDOUT_CAPTURE_POLICY,
                    STDERR_CAPTURE_POLICY,
                    None,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => {}
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
            }
        }
    }
    let system_python_runtime = python_runtime::desktop_python_runtime_mode_is_system();
    for (program, prefix_args) in candidates {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(*prefix_args);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        apply_streaming_command_env(&mut cmd);
        if system_python_runtime {
            python_runtime::apply_system_python_env_if_configured(&mut cmd);
        }
        command_utils::apply_platform_spawn_options(&mut cmd);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut parts = Vec::new();
        parts.push((*program).to_string());
        parts.extend(prefix_args.iter().map(|value| (*value).to_string()));
        parts.push(script_text.clone());
        let command_text = parts.join(" ");
        tried.push((*program).to_string());

        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                    STDOUT_CAPTURE_POLICY,
                    STDERR_CAPTURE_POLICY,
                    None,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => continue,
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
                break;
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow!("python interpreter not found (tried: {})", tried.join(", "))))
}

pub(crate) async fn execute_command(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    let dry_run = parse_dry_run(&args);
    let command_budget = parse_command_budget(&args);
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
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
    if let Some(result) = execute_command_in_sandbox_streaming_auto(context, &args, &content).await
    {
        if !dry_run {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
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
    let timeout_s = parse_timeout_secs(args.get("timeout_s"))
        .unwrap_or(0.0)
        .max(0.0);
    let timeout_s = apply_time_budget_secs(timeout_s, &command_budget);
    let timeout = if timeout_s > 0.0 {
        Some(Duration::from_secs_f64(timeout_s))
    } else {
        None
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
    let execute_tool_name = resolve_tool_name("execute_command");
    let (stdout_policy, stderr_policy) =
        derive_capture_policies(command_budget.output_budget_bytes);
    let effective_output_budget_bytes = stdout_policy
        .max_bytes()
        .saturating_add(stderr_policy.max_bytes());
    let commands = if allow_all {
        vec![content.clone()]
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
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
        let run = run_command_streaming(
            context,
            &command,
            &cwd,
            timeout,
            &execute_tool_name,
            stdout_policy,
            stderr_policy,
            command_index,
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
        results.push(json!({
            "command": command,
            "command_index": command_index,
            "command_session_id": run.command_session_id,
            "returncode": run.returncode,
            "stdout": run.stdout,
            "stderr": run.stderr,
            "output_meta": {
                "truncated": command_truncated,
                "total_bytes": command_total_bytes,
                "omitted_bytes": command_omitted_bytes,
                "stdout": run.stdout_capture.to_json(),
                "stderr": run.stderr_capture.to_json(),
            },
        }));
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
                ToolErrorMeta::new(
                    "TOOL_EXEC_TIMEOUT",
                    Some(
                        "命令执行超时，可拆分脚本或提高 timeout/budget.time_budget_ms 后重试。"
                            .to_string(),
                    ),
                    true,
                    Some(500),
                ),
                false,
            ));
        }
        if run.returncode != 0 {
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
                    "TOOL_EXEC_NON_ZERO_EXIT",
                    Some("命令返回非 0，请先根据 stderr 修正后再重试。".to_string()),
                    false,
                    None,
                ),
                false,
            ));
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

pub(crate) fn normalize_ptc_script_name(
    raw_filename: &str,
) -> std::result::Result<PathBuf, &'static str> {
    let filename = raw_filename.trim();
    if filename.is_empty() {
        return Err("tool.ptc.filename_required");
    }

    let mut script_name = PathBuf::from(filename);
    if script_name.file_name().and_then(|name| name.to_str()) != Some(filename) {
        return Err("tool.ptc.filename_invalid");
    }
    if script_name.extension().is_none() {
        script_name.set_extension("py");
    }
    if !script_name
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("py"))
        .unwrap_or(false)
    {
        return Err("tool.ptc.ext_invalid");
    }

    Ok(script_name)
}

fn build_ptc_exec_error(detail: impl Into<String>) -> Value {
    build_failed_tool_result(
        i18n::t_with_params(
            "tool.ptc.exec_error",
            &HashMap::from([("detail".to_string(), detail.into())]),
        ),
        json!({}),
        ToolErrorMeta::new(
            "TOOL_PTC_EXEC_ERROR",
            Some(
                "Inspect stderr/stdout and the saved script path, then fix the Python script or workdir."
                    .to_string(),
            ),
            false,
            None,
        ),
        false,
    )
}

pub(crate) async fn execute_ptc(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if let Some(result) = execute_in_sandbox(context, "ptc", &args).await {
        context.workspace.mark_tree_dirty(context.workspace_id);
        return Ok(result);
    }

    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let script_name = match normalize_ptc_script_name(filename) {
        Ok(name) => name,
        Err(key) => {
            return Ok(build_failed_tool_result(
                i18n::t(key),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_PTC_INVALID_FILENAME",
                    Some(
                        "Use a simple Python filename like helper.py without path separators."
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };

    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    if content.trim().is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.ptc.content_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_PTC_CONTENT_REQUIRED",
                Some("Provide the full Python script content in content.".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, content);
    let workdir_path = if workdir.is_empty() {
        if context
            .config
            .server
            .mode
            .trim()
            .eq_ignore_ascii_case("cli")
        {
            let configured_root = context.config.workspace.root.trim();
            if configured_root.is_empty() {
                context.workspace.ensure_user_root(context.workspace_id)?
            } else {
                PathBuf::from(configured_root)
            }
        } else {
            context.workspace.ensure_user_root(context.workspace_id)?
        }
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };

    if let Err(err) = tokio::fs::create_dir_all(&workdir_path).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let ptc_root = context
        .workspace
        .resolve_path(context.workspace_id, LOCAL_PTC_DIR_NAME)?;
    if let Err(err) = tokio::fs::create_dir_all(&ptc_root).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let script_path = ptc_root.join(script_name);
    if let Err(err) = tokio::fs::write(&script_path, content).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let output = match run_ptc_python_script_streaming(
        context,
        &script_path,
        &workdir_path,
        Some(Duration::from_secs(LOCAL_PTC_TIMEOUT_S)),
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return Ok(build_ptc_exec_error(err.to_string())),
    };

    let data = json!({
        "path": context
            .workspace
            .display_path(context.workspace_id, &script_path),
        "workdir": context
            .workspace
            .display_path(context.workspace_id, &workdir_path),
        "returncode": output.returncode,
        "stdout": output.stdout,
        "stderr": output.stderr,
    });

    context.workspace.mark_tree_dirty(context.workspace_id);

    if output.timed_out {
        let detail = format!("timeout after {}s", LOCAL_PTC_TIMEOUT_S);
        return Ok(build_failed_tool_result(
            i18n::t_with_params(
                "tool.ptc.exec_error",
                &HashMap::from([("detail".to_string(), detail)]),
            ),
            data,
            ToolErrorMeta::new(
                "TOOL_PTC_TIMEOUT",
                Some(
                    "Shorten the script, reduce external waits, or switch to execute_command for simpler shell work."
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }

    if output.returncode != 0 {
        return Ok(build_failed_tool_result(
            i18n::t("tool.ptc.exec_failed"),
            data,
            ToolErrorMeta::new(
                "TOOL_PTC_EXEC_FAILED",
                Some("Inspect stderr and fix the Python script before retrying.".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    Ok(build_model_tool_success(
        "ptc",
        "completed",
        format!("Executed Python script {}.", script_path.display()),
        data,
    ))
}
