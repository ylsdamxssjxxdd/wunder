use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolErrorMeta {
    pub(crate) code: String,
    pub(crate) hint: Option<String>,
    pub(crate) retryable: bool,
    pub(crate) retry_after_ms: Option<u64>,
}

impl ToolErrorMeta {
    pub(crate) fn new(
        code: impl Into<String>,
        hint: Option<String>,
        retryable: bool,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self {
            code: code.into(),
            hint,
            retryable,
            retry_after_ms,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "hint": self.hint,
            "retryable": self.retryable,
            "retry_after_ms": self.retry_after_ms,
        })
    }
}

pub(crate) fn build_failed_tool_result(
    error: impl Into<String>,
    data: Value,
    meta: ToolErrorMeta,
    sandbox: bool,
) -> Value {
    let data = with_error_meta(data, meta.clone());
    json!({
        "ok": false,
        "data": data,
        "error": error.into(),
        "error_meta": meta.to_json(),
        "sandbox": sandbox,
    })
}

pub(crate) fn tool_timeout_meta(code: &str) -> ToolErrorMeta {
    ToolErrorMeta::new(
        code,
        Some(
            "Verify the outcome before retrying; side effects may already have occurred."
                .to_string(),
        ),
        false,
        None,
    )
}

pub(crate) fn with_error_meta(data: Value, meta: ToolErrorMeta) -> Value {
    let mut payload = ensure_object_payload(data);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_meta".to_string(), meta.to_json());
    }
    payload
}

/// 超时后唯一可靠的探活证据就是“有没有产出”。
///
/// 有 stdout/stderr 说明命令确实跑起来了，副作用（写文件、发请求、动远端状态）
/// 可能已经发生，这时报 `unknown` 会让模型以为“什么都没做”而直接重跑；
/// 只有完全没有任何输出时，才真的无从判断。
pub(crate) fn command_timeout_outcome(data: &Value) -> &'static str {
    let produced_output = ["stdout", "stderr"].iter().any(|key| {
        data.get(*key)
            .and_then(Value::as_str)
            .is_some_and(|text| !strip_runtime_timeout_note(text).trim().is_empty())
    });
    if produced_output {
        "likely_applied"
    } else {
        "unknown"
    }
}

/// 超时后执行层会往 stderr 补一行 `timeout after 2s`，那是我们自己的标记，
/// 不是命令的输出；把它当证据会让所有超时都误判成“已执行”。
fn strip_runtime_timeout_note(raw: &str) -> String {
    const PREFIX: &str = "timeout after ";
    let mut kept: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        let is_note = trimmed == "timeout"
            || trimmed
                .strip_prefix(PREFIX)
                .and_then(|rest| rest.strip_suffix('s'))
                .is_some_and(|seconds| {
                    !seconds.is_empty() && seconds.chars().all(|c| c.is_ascii_digit() || c == '.')
                });
        if !is_note {
            kept.push(line);
        }
    }
    kept.join("\n")
}

pub(crate) fn build_execute_command_failure_data(
    results: &[Value],
    command_count: usize,
    truncated: bool,
    omitted_bytes: usize,
    timed_out: bool,
) -> Value {
    let mut payload = Map::new();
    if let Some(last) = results.last() {
        if let Some(command) = last
            .get("command")
            .cloned()
            .filter(|value| !value.is_null())
        {
            payload.insert("command".to_string(), command);
        }
        if let Some(command_index) = last
            .get("command_index")
            .cloned()
            .filter(|value| !value.is_null())
        {
            payload.insert("command_index".to_string(), command_index);
        }
        if !timed_out {
            if let Some(returncode) = last
                .get("returncode")
                .cloned()
                .filter(|value| !value.is_null())
            {
                payload.insert("returncode".to_string(), returncode);
            }
        }
        // Keep both streams: build and test tools report the diagnosis on stderr
        // and the summary on stdout, and dropping stdout here forced a full
        // re-run of the failing command just to see the other half.
        if let Some(stderr) = compacted_text(last.get("stderr").and_then(Value::as_str)) {
            payload.insert("stderr".to_string(), Value::String(stderr));
        }
        if let Some(stdout) = compacted_text(last.get("stdout").and_then(Value::as_str)) {
            payload.insert("stdout".to_string(), Value::String(stdout));
        }
    }
    // Keep a small, structured diagnostic trail for the event/admin view. The
    // model observation path removes `data` on failures and only receives the
    // concise error above, so this does not inflate prompt context.
    let diagnostics = results
        .iter()
        .filter(|row| {
            row.get("returncode")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
                || row.get("timed_out").and_then(Value::as_bool) == Some(true)
        })
        .take(4)
        .filter_map(compact_command_diagnostic)
        .collect::<Vec<_>>();
    if !diagnostics.is_empty() {
        payload.insert("diagnostics".to_string(), Value::Array(diagnostics));
    }
    if timed_out {
        payload.insert("timed_out".to_string(), Value::Bool(true));
        let outcome = results
            .last()
            .map(command_timeout_outcome)
            .unwrap_or("unknown");
        payload.insert("execution_outcome".to_string(), json!(outcome));
    }
    if command_count > 1 {
        payload.insert(
            "command_count".to_string(),
            Value::from(command_count as u64),
        );
        payload.insert(
            "completed_commands".to_string(),
            Value::from(results.len().saturating_sub(1) as u64),
        );
    }
    if truncated {
        payload.insert("output_truncated".to_string(), Value::Bool(true));
        if omitted_bytes > 0 {
            payload.insert(
                "omitted_bytes".to_string(),
                Value::from(omitted_bytes as u64),
            );
        }
    }
    Value::Object(payload)
}

fn compact_command_diagnostic(row: &Value) -> Option<Value> {
    let command = row.get("command").and_then(Value::as_str)?.trim();
    if command.is_empty() {
        return None;
    }
    let mut diagnostic = Map::new();
    diagnostic.insert("command".to_string(), Value::String(command.to_string()));
    for key in ["command_index", "returncode"] {
        if let Some(value) = row.get(key).filter(|value| !value.is_null()) {
            diagnostic.insert(key.to_string(), value.clone());
        }
    }
    if let Some(output) = compacted_text(row.get("stderr").and_then(Value::as_str))
        .or_else(|| compacted_text(row.get("stdout").and_then(Value::as_str)))
    {
        diagnostic.insert("output".to_string(), Value::String(output));
    }
    Some(Value::Object(diagnostic))
}

pub(crate) fn build_execute_command_failure_message(results: &[Value], timed_out: bool) -> String {
    if timed_out {
        let mut message = String::from("命令执行超时。");
        if let Some(last) = results.last().and_then(|row| {
            row.get("stderr")
                .and_then(Value::as_str)
                .map(strip_runtime_timeout_note)
                .and_then(|text| last_meaningful_line(Some(&text)))
                .or_else(|| last_meaningful_line(row.get("stdout").and_then(Value::as_str)))
        }) {
            message.push_str("最后一行输出：");
            message.push_str(&last);
            message.push('。');
        }
        return message;
    }
    let Some(last) = results.last() else {
        return "命令执行失败。".to_string();
    };
    let Some(returncode) = last.get("returncode").and_then(Value::as_i64) else {
        return "命令执行失败。".to_string();
    };
    let mut message = String::from("命令退出码 ");
    message.push_str(&returncode.to_string());
    message.push('。');
    let stderr_raw = last.get("stderr").and_then(Value::as_str);
    let stdout_raw = last.get("stdout").and_then(Value::as_str);
    let stderr_blank = stderr_raw
        .map(|text| text.trim().is_empty())
        .unwrap_or(true);
    if stderr_blank {
        if let Some(stdout) = compact_command_output(stdout_raw) {
            if stdout.total_lines > 0 {
                message.push_str("stdout 共 ");
                message.push_str(&stdout.total_lines.to_string());
                message.push_str(" 行");
                if stdout.omitted_lines > 0 {
                    message.push_str("（仅保留前 ");
                    message.push_str(&COMPACT_HEAD_LINES.to_string());
                    message.push_str(" 行与后 ");
                    message.push_str(&COMPACT_TAIL_LINES.to_string());
                    message.push_str(" 行，中间 ");
                    message.push_str(&stdout.omitted_lines.to_string());
                    message.push_str(" 行省略）");
                }
                message.push_str("，所需信息可能已在其中，可基于它继续而不必重跑。");
            }
        }
    } else if let Some(stderr) = compact_command_output(stderr_raw) {
        if stderr.omitted_lines > 0 {
            message.push_str("stderr 共 ");
            message.push_str(&stderr.total_lines.to_string());
            message.push_str(" 行（仅保留头部与尾部，中间 ");
            message.push_str(&stderr.omitted_lines.to_string());
            message.push_str(" 行省略）。");
        }
    }
    // 模型侧只消费这条摘要，看不到失败 data 里的完整 stderr；把最后一行带出来，
    // 参数错误、路径不存在这类问题才不用靠盲猜和重试。
    if let Some(last) =
        last_meaningful_line(stderr_raw).or_else(|| last_meaningful_line(stdout_raw))
    {
        message.push_str("最后一行输出：");
        message.push_str(&last);
        message.push('。');
    }
    if let Some(note) = broken_pipe_note(stderr_raw, Some(returncode)) {
        message.push_str(note);
    }
    message
}

fn last_meaningful_line(text: Option<&str>) -> Option<String> {
    let line = text?
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut truncated = line.chars().take(240).collect::<String>();
    if line.chars().count() > 240 {
        truncated.push('…');
    }
    Some(truncated)
}

/// 失败命令摘要保留的行数：头尾各留几行。
///
/// 只留尾部会让调用方以为「输出前半截丢了」，只留头部又看不到真正的报错，所以
/// 头尾都留，并把总行数与省略行数一起说清楚。
const COMPACT_HEAD_LINES: usize = 3;
const COMPACT_TAIL_LINES: usize = 3;
const COMPACT_MAX_CHARS: usize = 480;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompactedOutput {
    pub(crate) text: String,
    pub(crate) total_lines: usize,
    pub(crate) kept_lines: usize,
    pub(crate) omitted_lines: usize,
}

fn compacted_text(raw: Option<&str>) -> Option<String> {
    compact_command_output(raw).map(|output| output.text)
}

fn compact_command_output(raw: Option<&str>) -> Option<CompactedOutput> {
    let text = raw.map(str::trim).filter(|value| !value.is_empty())?;
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let total_lines = lines.len();
    let budget = COMPACT_HEAD_LINES + COMPACT_TAIL_LINES;
    if total_lines <= budget {
        return Some(CompactedOutput {
            text: lines.join("\n"),
            total_lines,
            kept_lines: total_lines,
            omitted_lines: 0,
        });
    }
    let half = COMPACT_MAX_CHARS / 2;
    let head = trim_tail_chars(&lines[..COMPACT_HEAD_LINES].join("\n"), half);
    let tail = trim_head_chars(&lines[total_lines - COMPACT_TAIL_LINES..].join("\n"), half);
    let omitted_lines = total_lines - budget;
    let mut marker = String::from("...(");
    marker.push_str(&omitted_lines.to_string());
    marker.push_str(" lines omitted)...");
    let mut text = head;
    text.push('\n');
    text.push_str(&marker);
    text.push('\n');
    text.push_str(&tail);
    Some(CompactedOutput {
        text,
        total_lines,
        kept_lines: budget,
        omitted_lines,
    })
}

fn trim_tail_chars(raw: &str, limit: usize) -> String {
    if raw.chars().count() <= limit {
        return raw.to_string();
    }
    let mut text = raw.chars().take(limit).collect::<String>();
    text.push_str(" ...");
    text
}

fn trim_head_chars(raw: &str, limit: usize) -> String {
    if raw.chars().count() <= limit {
        return raw.to_string();
    }
    let skipped = raw.chars().count() - limit;
    let mut text = String::from("... ");
    text.push_str(&raw.chars().skip(skipped).collect::<String>());
    text
}

/// SIGPIPE（下游 head/cut 提前关管道）常被误读成「命令本身出了问题」。
fn broken_pipe_note(stderr: Option<&str>, returncode: Option<i64>) -> Option<&'static str> {
    let stderr_says_pipe = stderr.is_some_and(|text| {
        let lowered = text.to_ascii_lowercase();
        lowered.contains("broken pipe") || lowered.contains("epipe")
    });
    if returncode == Some(141) || stderr_says_pipe {
        Some("管道被下游提前关闭（SIGPIPE/EPIPE），通常是 head、cut 一类截断所致，不代表命令本身失败；要全量输出就别在命令里截断。")
    } else {
        None
    }
}

fn ensure_object_payload(data: Value) -> Value {
    if data.is_object() {
        return data;
    }
    let mut map = Map::new();
    map.insert("result".to_string(), data);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_error_meta_wraps_non_object_payload() {
        let payload = with_error_meta(
            Value::String("bad".to_string()),
            ToolErrorMeta::new("TOOL_ERROR", None, false, None),
        );
        let obj = payload.as_object().expect("payload should be object");
        assert_eq!(obj.get("result"), Some(&Value::String("bad".to_string())));
        assert!(obj.get("error_meta").is_some());
    }

    #[test]
    fn build_failed_tool_result_embeds_error_meta_into_data() {
        let payload = build_failed_tool_result(
            "failed",
            json!({ "detail": "bad request" }),
            ToolErrorMeta::new(
                "TOOL_FAILED",
                Some("retry later".to_string()),
                true,
                Some(250),
            ),
            false,
        );
        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .expect("data object");
        let meta = data
            .get("error_meta")
            .and_then(Value::as_object)
            .expect("embedded error_meta");
        assert_eq!(
            meta.get("code"),
            Some(&Value::String("TOOL_FAILED".to_string()))
        );
        assert_eq!(meta.get("retryable"), Some(&Value::Bool(true)));
    }

    #[test]
    fn build_execute_command_failure_data_keeps_a_stdout_tail_as_well() {
        let payload = build_execute_command_failure_data(
            &[json!({
                "command": "cargo test",
                "command_index": 0,
                "returncode": 101,
                "stdout": "running 3 tests\ntest result: FAILED. 1 passed; 2 failed\n",
                "stderr": "thread 'x' panicked at src/lib.rs\nerror: test failed\n"
            })],
            1,
            false,
            0,
            false,
        );
        assert!(
            payload["stderr"]
                .as_str()
                .is_some_and(|text| text.contains("panicked")),
            "stderr: {payload}"
        );
        assert!(
            payload["stdout"]
                .as_str()
                .is_some_and(|text| text.contains("test result: FAILED")),
            "stdout dropped: {payload}"
        );
    }

    #[test]
    fn build_execute_command_failure_data_keeps_only_actionable_tail() {
        let payload = build_execute_command_failure_data(
            &[json!({
                "command": "python draw_heart.py",
                "command_index": 0,
                "returncode": 1,
                "stderr": "Traceback (most recent call last):\n  File \"/tmp/draw_heart.py\", line 6\n    y = 1\nIndentationError: unindent does not match any outer indentation level\n",
                "stdout": ""
            })],
            1,
            false,
            0,
            false,
        );
        assert_eq!(payload["command"], json!("python draw_heart.py"));
        assert_eq!(payload["returncode"], json!(1));
        assert_eq!(
            payload["stderr"],
            json!("Traceback (most recent call last):\n  File \"/tmp/draw_heart.py\", line 6\n    y = 1\nIndentationError: unindent does not match any outer indentation level")
        );
        assert!(payload.get("stdout").is_none());
        assert_eq!(payload["diagnostics"][0]["command_index"], json!(0));
        assert!(payload["diagnostics"][0]["output"]
            .as_str()
            .is_some_and(|text| text.contains("IndentationError")));
    }

    #[test]
    fn build_execute_command_failure_message_points_to_partial_stdout_when_stderr_blank() {
        let message = build_execute_command_failure_message(
            &[json!({
                "command": "grep needle notes.txt",
                "returncode": 1,
                "stdout": "needle first hit\nneedle second hit\n",
                "stderr": ""
            })],
            false,
        );
        assert!(message.contains("命令退出码 1。"));
        assert!(message.contains("stdout 共 2 行"));
        assert!(message.contains("可基于它继续而不必重跑"));
    }

    #[test]
    fn failure_message_reports_omitted_lines_instead_of_hiding_them() {
        let stdout = (1..=20)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = build_execute_command_failure_message(
            &[json!({
                "command": "make",
                "returncode": 2,
                "stdout": stdout,
                "stderr": ""
            })],
            false,
        );
        assert!(message.contains("stdout 共 20 行"), "{message}");
        assert!(message.contains("中间 14 行省略"), "{message}");
    }

    #[test]
    fn compact_output_keeps_head_and_tail() {
        let stdout = (1..=10)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let compacted = compact_command_output(Some(&stdout)).expect("compacted");
        assert_eq!(compacted.total_lines, 10);
        assert_eq!(compacted.omitted_lines, 4);
        assert!(compacted.text.starts_with("line 1"), "{}", compacted.text);
        assert!(compacted.text.ends_with("line 10"), "{}", compacted.text);
        assert!(
            compacted.text.contains("4 lines omitted"),
            "{}",
            compacted.text
        );
    }

    #[test]
    fn timeout_message_keeps_last_partial_output_line() {
        let message = build_execute_command_failure_message(
            &[json!({
                "command": "long-task",
                "returncode": -1,
                "stdout": "step 1 done\n",
                "stderr": "warning: slow\ntimeout after 2s"
            })],
            true,
        );
        assert!(message.contains("命令执行超时"), "{message}");
        assert!(message.contains("warning: slow"), "{message}");
        assert!(!message.contains("timeout after 2s"), "{message}");
    }

    #[test]
    fn broken_pipe_is_called_out_instead_of_looking_like_a_failure() {
        let message = build_execute_command_failure_message(
            &[json!({
                "command": "grep -rn foo . | head -30",
                "returncode": 141,
                "stdout": "",
                "stderr": ""
            })],
            false,
        );
        assert!(message.contains("SIGPIPE"), "{message}");
    }

    #[test]
    fn build_execute_command_failure_message_keeps_plain_message_when_stderr_present() {
        let message = build_execute_command_failure_message(
            &[json!({
                "command": "false",
                "returncode": 2,
                "stdout": "partial\n",
                "stderr": "boom\n"
            })],
            false,
        );
        assert_eq!(message, "命令退出码 2。最后一行输出：boom。");
    }

    #[test]
    fn build_execute_command_failure_data_falls_back_to_stdout_when_stderr_blank() {
        let payload = build_execute_command_failure_data(
            &[json!({
                "command": "grep needle notes.txt",
                "command_index": 0,
                "returncode": 1,
                "stderr": "",
                "stdout": "needle first hit\nneedle second hit\n"
            })],
            1,
            false,
            0,
            false,
        );
        assert!(payload.get("stderr").is_none());
        assert_eq!(
            payload["stdout"],
            json!("needle first hit\nneedle second hit")
        );
    }
}
