use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use wunder_server::schemas::StreamEvent;

use crate::command_session_display::{
    compact_text_preview, CommandSessionDisplayState, CommandSessionDisplayStatus,
    CommandSessionUpdate, CommandSessionView,
};
use crate::patch_diff::{build_patch_diff_preview, format_patch_diff_preview_lines};
use crate::tool_display::summarize_tool_result;

const MAX_INLINE_JSON_CHARS: usize = 180;
const MAX_PATCH_RESULT_FILES: usize = 24;
const COMMAND_LIVE_PRINT_HEAD_CHARS: usize = 2_000;
const COMMAND_LIVE_PRINT_TAIL_CHARS: usize = 1_500;

#[derive(Debug, Clone, Default)]
pub struct FinalEvent {
    pub answer: String,
    pub usage: Option<Value>,
    pub stop_reason: Option<String>,
}

pub struct StreamRenderer {
    json: bool,
    /// One-shot exec mode: tool/command/approval output goes to stderr so a
    /// pipeline can read the reply from stdout without the agent's trace.
    progress_on_stderr: bool,
    /// ANSI colour for the progress stream only; the answer itself stays plain
    /// so a captured reply never carries escape sequences.
    color: bool,
    line_open: bool,
    saw_delta: bool,
    saw_tool_activity: bool,
    last_visible_was_tool: bool,
    is_zh: bool,
    command_sessions: CommandSessionDisplayState,
    command_live: HashMap<String, CommandLivePrintState>,
    command_start_printed: HashSet<String>,
    command_terminal_status_printed: HashSet<String>,
}

#[derive(Debug, Default)]
struct CommandLivePrintState {
    printed_chars: usize,
    suppressed_chars: usize,
    tail: String,
    output_header_printed: bool,
    at_line_start: bool,
}

fn event_payload(data: &Value) -> &Value {
    data.get("data").unwrap_or(data)
}

/// A tool result is a failure when its payload says so at any of the levels the
/// engine nests it in.
fn tool_result_failed(payload: &Value) -> bool {
    let result = payload.get("result").unwrap_or(payload);
    let ok = result
        .get("ok")
        .and_then(Value::as_bool)
        .or_else(|| payload.get("ok").and_then(Value::as_bool))
        .unwrap_or(true);
    !ok
}

impl StreamRenderer {
    pub fn new(json: bool, language: &str) -> Self {
        Self {
            json,
            progress_on_stderr: false,
            color: false,
            line_open: false,
            saw_delta: false,
            saw_tool_activity: false,
            last_visible_was_tool: false,
            is_zh: crate::locale::is_zh_language(language),
            command_sessions: CommandSessionDisplayState::default(),
            command_live: HashMap::new(),
            command_start_printed: HashSet::new(),
            command_terminal_status_printed: HashSet::new(),
        }
    }

    /// Route everything that is not the reply itself to stderr. Used by the
    /// non-interactive `exec` entry point, mirroring the codex contract that
    /// stdout carries the answer and stderr carries the trace.
    pub fn with_progress_on_stderr(mut self) -> Self {
        self.progress_on_stderr = true;
        self
    }

    /// Colour the progress stream. Semantic only: dim for structure, red for
    /// failure, green for success - never for the plain answer text.
    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    fn paint(&self, text: &str, code: &str) -> String {
        if !self.color || text.is_empty() {
            return text.to_string();
        }
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    }

    fn dim(&self, text: &str) -> String {
        self.paint(text, "2")
    }

    fn red(&self, text: &str) -> String {
        self.paint(text, "31;1")
    }

    fn green(&self, text: &str) -> String {
        self.paint(text, "32;1")
    }

    /// A tool result: failures are red, successes keep the neutral weight.
    fn progress_result_line(&self, line: &str, failed: bool) {
        let line = if failed {
            self.red(line)
        } else {
            line.to_string()
        };
        self.progress_line(&line);
    }

    fn progress_line(&self, text: &str) {
        if self.progress_on_stderr {
            let stderr = io::stderr();
            let mut lock = stderr.lock();
            let _ = writeln!(lock, "{text}");
        } else {
            println!("{text}");
        }
    }

    fn progress_chunk(&self, text: &str) {
        if self.progress_on_stderr {
            let stderr = io::stderr();
            let mut lock = stderr.lock();
            let _ = write!(lock, "{text}");
            let _ = lock.flush();
        } else {
            print!("{text}");
            io::stdout().flush().ok();
        }
    }

    pub fn render_event(&mut self, event: &StreamEvent) -> Result<Option<FinalEvent>> {
        if self.json {
            println!("{}", serde_json::to_string(event)?);
            return Ok(parse_final(event));
        }

        let payload = event_payload(&event.data);
        // The runtime collapses every `*_delta` frame into `thread_item_delta`
        // and keeps the semantic stream type in `source_event`; resolve it back
        // so the streaming arms below keep receiving live deltas.
        let kind = payload
            .get("source_event")
            .and_then(Value::as_str)
            .unwrap_or_else(|| event.event.as_str());
        match kind {
            "llm_output_delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        print!("{delta}");
                        io::stdout().flush().ok();
                        self.line_open = true;
                        self.saw_delta = true;
                        self.last_visible_was_tool = false;
                    }
                }
            }
            "llm_output" => {
                if !self.saw_delta {
                    if let Some(content) = payload.get("content").and_then(Value::as_str) {
                        if !content.is_empty() {
                            print!("{content}");
                            io::stdout().flush().ok();
                            self.line_open = true;
                            // Non-streamed replies are printed here; marking them
                            // as emitted keeps the final event from printing a
                            // second copy on stdout.
                            self.saw_delta = true;
                            self.last_visible_was_tool = false;
                        }
                    }
                }
            }
            "progress" => {
                // Skip progress events in tool-only workflow rendering.
            }
            "tool_call" => {
                self.ensure_newline();
                self.saw_tool_activity = true;
                self.last_visible_was_tool = true;
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = payload.get("args").unwrap_or(&Value::Null);
                let repair = payload.get("repair");
                if is_execute_command_tool_name(tool) {
                    if self.command_sessions.register_tool_call(payload).is_none() {
                        let line = format_tool_call_line(tool, args, repair, self.is_zh);
                        let line = self.dim(&line);
                        self.progress_line(&line);
                    }
                } else {
                    let line = format_tool_call_line(tool, args, repair, self.is_zh);
                    let line = self.dim(&line);
                    self.progress_line(&line);
                }
            }
            "command_session_start" => {
                self.ensure_newline();
                self.saw_tool_activity = true;
                self.last_visible_was_tool = true;
                if let Some(update) = self.command_sessions.register_start(payload) {
                    self.render_command_session_start(&update);
                }
            }
            "command_session_delta" => {
                self.saw_tool_activity = true;
                self.last_visible_was_tool = true;
                if let Some(update) = self.command_sessions.register_delta(payload) {
                    self.render_command_session_delta(&update, payload);
                }
            }
            "command_session_status" | "command_session_exit" | "command_session_summary" => {
                self.ensure_newline();
                self.saw_tool_activity = true;
                self.last_visible_was_tool = true;
                if let Some(update) = self.command_sessions.register_status(payload) {
                    self.render_command_session_terminal(&update, false);
                }
            }
            "tool_result" => {
                self.ensure_newline();
                self.saw_tool_activity = true;
                self.last_visible_was_tool = true;
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let failed = tool_result_failed(payload);
                if is_apply_patch_tool_name(tool) {
                    for line in format_apply_patch_result_lines(tool, payload, self.is_zh) {
                        self.progress_result_line(&line, failed);
                    }
                } else if is_execute_command_tool_name(tool) {
                    let updates = self.command_sessions.register_tool_result_all(payload);
                    if !updates.is_empty() {
                        for update in updates {
                            self.render_command_session_terminal(&update, true);
                        }
                    } else {
                        for line in format_generic_tool_result_lines(tool, payload, self.is_zh) {
                            self.progress_result_line(&line, failed);
                        }
                    }
                } else {
                    for line in format_generic_tool_result_lines(tool, payload, self.is_zh) {
                        self.progress_result_line(&line, failed);
                    }
                }
                let tool_key = tool.trim().to_ascii_lowercase();
                let is_question_tool = tool_key == "question_panel"
                    || tool_key == "ask_panel"
                    || tool.contains("问询面板");
                if is_question_tool {
                    for panel_payload in [
                        payload.get("data"),
                        payload.get("result"),
                        payload.get("result").and_then(|value| value.get("data")),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if render_question_panel_lines(panel_payload, self.progress_on_stderr) {
                            break;
                        }
                    }
                }
            }
            "question_panel" => {
                self.ensure_newline();
                self.last_visible_was_tool = true;
                let _ = render_question_panel_lines(payload, self.progress_on_stderr);
            }
            "error" => {
                self.ensure_newline();
                self.last_visible_was_tool = false;
                let message = crate::error_display::format_error_message(payload)
                    .unwrap_or_else(|| compact_json(payload));
                eprintln!("[error] {message}");
            }
            "final" => {
                self.ensure_newline();
                let mut final_event = parse_final(event).unwrap_or_default();
                if final_event.answer.trim().is_empty()
                    && self.saw_tool_activity
                    && self.last_visible_was_tool
                {
                    final_event.answer = tool_only_completion_fallback(self.is_zh);
                }
                if !self.saw_delta && !final_event.answer.is_empty() {
                    println!("{}", final_event.answer);
                }
                self.last_visible_was_tool = false;
                return Ok(Some(final_event));
            }
            _ => {}
        }
        Ok(None)
    }

    pub fn finish(&mut self) {
        self.ensure_newline();
    }

    fn ensure_newline(&mut self) {
        if self.line_open {
            println!();
            self.line_open = false;
        }
    }

    fn render_command_session_start(&mut self, update: &CommandSessionUpdate) {
        let primary = self.command_sessions.primary_for_view(&update.view);
        if !self.command_start_printed.insert(primary) {
            return;
        }
        let command = update.view.command.trim();
        if command.is_empty() {
            self.progress_line("- Running command");
        } else {
            self.progress_line("- Running command");
            self.progress_line(&format!("  command: `{command}`"));
        }
    }

    fn render_command_session_delta(&mut self, update: &CommandSessionUpdate, payload: &Value) {
        let primary = self.command_sessions.primary_for_view(&update.view);
        let delta = payload
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if delta.is_empty() {
            return;
        }

        if !self
            .command_live
            .get(primary.as_str())
            .is_some_and(|state| state.output_header_printed)
        {
            self.ensure_newline();
        }

        let mut marker = None;
        let mut chunk = None;
        {
            let state = self.command_live.entry(primary).or_default();
            let remaining = COMMAND_LIVE_PRINT_HEAD_CHARS.saturating_sub(state.printed_chars);
            if remaining > 0 {
                let (prefix, truncated) = take_chars(delta, remaining);
                if !prefix.is_empty() {
                    chunk = Some(format_command_live_chunk(state, prefix.as_str()));
                    state.printed_chars =
                        state.printed_chars.saturating_add(prefix.chars().count());
                    // Progress on stderr never leaves a stdout line open.
                    self.line_open = command_live_line_open(state) && !self.progress_on_stderr;
                }
                if truncated {
                    let rest = delta
                        .chars()
                        .skip(prefix.chars().count())
                        .collect::<String>();
                    append_tail(
                        &mut state.tail,
                        rest.as_str(),
                        COMMAND_LIVE_PRINT_TAIL_CHARS,
                    );
                    state.suppressed_chars =
                        state.suppressed_chars.saturating_add(rest.chars().count());
                    marker = Some(state.suppressed_chars);
                }
            } else {
                append_tail(&mut state.tail, delta, COMMAND_LIVE_PRINT_TAIL_CHARS);
                let was_zero = state.suppressed_chars == 0;
                state.suppressed_chars =
                    state.suppressed_chars.saturating_add(delta.chars().count());
                if was_zero {
                    marker = Some(state.suppressed_chars);
                }
            }
        }
        if let Some(chunk) = chunk {
            self.progress_chunk(chunk.as_str());
        }
        if let Some(suppressed_chars) = marker {
            self.print_command_suppression_marker(suppressed_chars);
        }
    }

    fn print_command_suppression_marker(&mut self, suppressed_chars: usize) {
        self.ensure_newline();
        let line = format!(
            "  ... live output truncated, keeping tail ({suppressed_chars} chars omitted so far)"
        );
        self.progress_line(&line);
    }

    fn render_command_session_terminal(
        &mut self,
        update: &CommandSessionUpdate,
        from_tool_result: bool,
    ) {
        if !update.view.is_terminal() {
            return;
        }
        let primary = self.command_sessions.primary_for_view(&update.view);
        let status_already_printed = self
            .command_terminal_status_printed
            .contains(primary.as_str());
        let live = self
            .command_live
            .remove(primary.as_str())
            .unwrap_or_default();
        if live.suppressed_chars > 0 && !live.tail.trim().is_empty() {
            self.ensure_newline();
            self.progress_line("  tail:");
            for line in compact_output_lines(live.tail.as_str(), 8, 1_500) {
                self.progress_line(&format!("    {line}"));
            }
        }

        if from_tool_result && !update.had_output_before && live.printed_chars == 0 {
            let lines = if status_already_printed {
                command_session_output_lines(&update.view)
            } else {
                self.command_terminal_status_printed.insert(primary);
                command_session_result_lines(&update.view)
            };
            for line in lines {
                self.progress_line(&line);
            }
            return;
        }

        if status_already_printed {
            return;
        }
        self.command_terminal_status_printed.insert(primary);
        let line = command_session_status_line(&update.view);
        if !line.is_empty() {
            let line = if update.view.success() {
                self.green(&line)
            } else {
                self.red(&line)
            };
            self.progress_line(&line);
        }
    }
}

fn format_command_live_chunk(state: &mut CommandLivePrintState, chunk: &str) -> String {
    let mut output = String::new();
    if !state.output_header_printed {
        output.push_str("  output:\n");
        state.output_header_printed = true;
        state.at_line_start = true;
    }
    for ch in chunk.chars() {
        if state.at_line_start {
            output.push_str("    ");
            state.at_line_start = false;
        }
        output.push(ch);
        if ch == '\n' {
            state.at_line_start = true;
        }
    }
    output
}

fn command_live_line_open(state: &CommandLivePrintState) -> bool {
    state.output_header_printed && !state.at_line_start
}

fn format_tool_call_line(tool: &str, args: &Value, repair: Option<&Value>, is_zh: bool) -> String {
    let repair_suffix = repair
        .and_then(format_repair_badge)
        .map(|badge| format!(" {badge}"))
        .unwrap_or_default();
    let tool_is_zh = is_zh;
    let mut lines = Vec::new();
    if is_execute_command_tool_name(tool) {
        if let Some(command) = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(if tool_is_zh {
                format!("• 调用 {tool}{repair_suffix}")
            } else {
                format!("• Called {tool}{repair_suffix}")
            });
            lines.push(format!("  └ `{command}`"));
            return lines.join("\n");
        }
    }

    if is_apply_patch_tool_name(tool) {
        lines.push(if tool_is_zh {
            format!("• 调用 {tool}{repair_suffix}")
        } else {
            format!("• Called {tool}{repair_suffix}")
        });
        if let Some(patch) = extract_patch_input(args)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let summary = summarize_patch_input(patch);
            if !summary.is_empty() {
                lines.push(format!("  └ {summary}"));
            }
            append_patch_preview_lines(
                &mut lines,
                extract_patch_preview_lines(patch, 8, tool_is_zh),
            );
        }
        return lines.join("\n");
    }

    if args.is_null() {
        return if tool_is_zh {
            format!("• 调用 {tool}{repair_suffix}\n  └ {{}}")
        } else {
            format!("• Called {tool}{repair_suffix}\n  └ {{}}")
        };
    }

    lines.push(if tool_is_zh {
        format!("• 调用 {tool}{repair_suffix}")
    } else {
        format!("• Called {tool}{repair_suffix}")
    });
    lines.push(format!("  └ {}", summarize_tool_args(args)));
    lines.join("\n")
}

fn format_repair_badge(repair: &Value) -> Option<&'static str> {
    repair.is_object().then_some("(args repaired)")
}

fn extract_patch_input(args: &Value) -> Option<&str> {
    if let Value::String(value) = args {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let obj = args.as_object()?;
    for key in ["input", "patch", "content", "raw"] {
        if let Some(value) = obj.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn summarize_patch_input(patch: &str) -> String {
    let line_count = patch.lines().count();
    let (op_count, added_lines, removed_lines) = count_patch_metrics(patch);
    if op_count > 0 {
        let mut parts = vec![format!("files={op_count}")];
        if added_lines > 0 || removed_lines > 0 {
            parts.push(format!("+{added_lines}"));
            parts.push(format!("-{removed_lines}"));
        } else {
            parts.push(format!("lines={line_count}"));
        }
        parts.join(", ")
    } else if line_count > 0 {
        format!("lines={line_count}")
    } else {
        String::new()
    }
}

fn count_patch_metrics(patch: &str) -> (usize, usize, usize) {
    let mut files = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut in_file = false;

    for raw_line in patch.lines() {
        let line = raw_line.trim();
        if line.starts_with("*** Add File:")
            || line.starts_with("*** Update File:")
            || line.starts_with("*** Delete File:")
        {
            files = files.saturating_add(1);
            in_file = true;
            continue;
        }
        if line.starts_with("*** ") {
            in_file = false;
            continue;
        }
        if !in_file {
            continue;
        }
        if raw_line.starts_with('+') {
            added = added.saturating_add(1);
        } else if raw_line.starts_with('-') {
            removed = removed.saturating_add(1);
        }
    }

    (files, added, removed)
}

fn extract_patch_preview_lines(patch: &str, max_entries: usize, is_zh: bool) -> Vec<String> {
    let preview = build_patch_diff_preview(patch, max_entries, 6, is_zh);
    if preview.is_empty() {
        Vec::new()
    } else {
        format_patch_diff_preview_lines(&preview)
    }
}

fn summarize_tool_args(args: &Value) -> String {
    if let Some(object) = args.as_object() {
        for key in [
            "path",
            "file_path",
            "filePath",
            "query",
            "q",
            "url",
            "location",
            "ticker",
            "command",
            "content",
            "text",
            "prompt",
            "name",
        ] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                let cleaned = value.trim();
                if !cleaned.is_empty() {
                    return format!("{key}={cleaned}");
                }
            }
        }
    }
    compact_json(args)
}

fn render_question_panel_lines(payload: &Value, to_stderr: bool) -> bool {
    let emit = |line: String| {
        if to_stderr {
            let stderr = io::stderr();
            let mut lock = stderr.lock();
            let _ = writeln!(lock, "{line}");
        } else {
            println!("{line}");
        }
    };
    let raw_items: Vec<Value> = match payload.get("questions").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => items.clone(),
        // 旧版扁平单题写法（含历史线程里已落盘的面板）。
        _ => vec![payload.clone()],
    };
    let mut questions: Vec<(String, Vec<Value>, bool)> = Vec::new();
    for item in raw_items {
        let question = item
            .get("question")
            .or_else(|| item.get("prompt"))
            .or_else(|| item.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let options = item
            .get("options")
            .or_else(|| item.get("routes"))
            .or_else(|| item.get("choices"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let multiple = item
            .get("multiple")
            .or_else(|| item.get("allow_multiple"))
            .or_else(|| item.get("multi"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if question.is_empty() && options.is_empty() {
            continue;
        }
        questions.push((question, options, multiple));
    }
    if questions.is_empty() {
        return false;
    }

    let is_zh = questions.iter().any(|(question, options, _)| {
        looks_like_zh(question.as_str())
            || options.iter().any(|item| {
                item.get("label")
                    .or_else(|| item.get("title"))
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .map(looks_like_zh)
                    .unwrap_or(false)
            })
    });

    let question_total = questions.len();
    let mut number = 0usize;
    for (index, (question, options, multiple)) in questions.iter().enumerate() {
        let mut heading = if question.is_empty() {
            if is_zh {
                "请选择一条路线继续".to_string()
            } else {
                "Choose a route to continue".to_string()
            }
        } else if question_total > 1 {
            if is_zh {
                format!("问题 {}/{}：{}", index + 1, question_total, question)
            } else {
                format!("Question {}/{}: {}", index + 1, question_total, question)
            }
        } else {
            question.clone()
        };
        if *multiple {
            heading.push_str(if is_zh { "（可多选）" } else { " (multi-select)" });
        }
        emit(format!("[question_panel] {heading}"));
        for option in options {
            let (label, description, recommended) = match option {
                Value::String(value) => (value.trim().to_string(), String::new(), false),
                Value::Object(map) => {
                    let label = map
                        .get("label")
                        .or_else(|| map.get("title"))
                        .or_else(|| map.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let description = map
                        .get("description")
                        .or_else(|| map.get("detail"))
                        .or_else(|| map.get("desc"))
                        .or_else(|| map.get("summary"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let recommended = map
                        .get("recommended")
                        .or_else(|| map.get("preferred"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    (label, description, recommended)
                }
                _ => (String::new(), String::new(), false),
            };
            if label.is_empty() {
                continue;
            }
            number += 1;
            let recommended_tag = if recommended {
                if is_zh {
                    "（推荐）"
                } else {
                    " (recommended)"
                }
            } else {
                ""
            };
            if description.is_empty() {
                emit(format!("  {}. {}{}", number, label, recommended_tag));
            } else {
                let separator = if is_zh { "：" } else { ": " };
                emit(format!(
                    "  {}. {}{}{}{}",
                    number, label, recommended_tag, separator, description
                ));
            }
        }
    }

    if is_zh {
        emit("  输入序号选择，例如 1 或 1,3".to_string());
    } else {
        emit("  choose by typing route number(s), e.g. 1 or 1,3".to_string());
    }
    true
}

fn looks_like_zh(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || ('\u{20000}'..='\u{2a6df}').contains(&ch)
    })
}

fn parse_final(event: &StreamEvent) -> Option<FinalEvent> {
    if event.event != "final" {
        return None;
    }
    let payload = event_payload(&event.data);
    Some(FinalEvent {
        answer: payload
            .get("answer")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        usage: payload
            .get("usage")
            .cloned()
            .filter(|value| !value.is_null()),
        stop_reason: payload
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn is_apply_patch_tool_name(tool: &str) -> bool {
    let normalized = tool.trim().to_ascii_lowercase();
    normalized == "apply_patch" || tool.contains("应用补丁")
}

fn is_execute_command_tool_name(tool: &str) -> bool {
    let normalized = tool.trim().to_ascii_lowercase();
    normalized == "execute_command" || tool.contains("执行命令")
}

fn extract_tool_result_object(payload: &Value) -> &Value {
    payload.get("result").unwrap_or(payload)
}

fn extract_tool_result_data(result: &Value) -> &Value {
    result.get("data").unwrap_or(result)
}

fn number_value(value: Option<&Value>) -> i64 {
    value
        .and_then(|item| {
            item.as_i64()
                .or_else(|| item.as_u64().map(|num| num.min(i64::MAX as u64) as i64))
                .or_else(|| {
                    item.as_str()
                        .and_then(|text| text.trim().parse::<i64>().ok())
                })
        })
        .unwrap_or(0)
        .max(0)
}

fn extract_apply_patch_file_line(file: &Value) -> Option<String> {
    let obj = file.as_object()?;
    let action = obj
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let path = obj.get("path").and_then(Value::as_str).unwrap_or("").trim();
    let to_path = obj
        .get("to_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if path.is_empty() && to_path.is_empty() {
        return None;
    }

    let has_move = !path.is_empty() && !to_path.is_empty() && to_path != path;
    let marker = match action.as_str() {
        "add" => "A",
        "delete" => "D",
        "update" if has_move => "R",
        "update" => "M",
        "move" => "R",
        _ => "M",
    };
    let text = if !path.is_empty() && !to_path.is_empty() && to_path != path {
        format!("{path} → {to_path}")
    } else if !path.is_empty() {
        path.to_string()
    } else {
        to_path.to_string()
    };
    Some(format!("  {marker} {text}"))
}

fn push_tree_line(lines: &mut Vec<String>, content: String) {
    let prefix = if lines.len() <= 1 { "  └ " } else { "    " };
    lines.push(format!("{prefix}{content}"));
}

fn append_patch_preview_lines(lines: &mut Vec<String>, preview_lines: Vec<String>) {
    for line in preview_lines {
        let trimmed = line.trim_start();
        let is_header = trimmed.starts_with("diff ") || trimmed.starts_with('…');
        let prefix = if is_header { "    " } else { "      " };
        lines.push(format!("{prefix}{trimmed}"));
    }
}

fn append_text_preview(
    lines: &mut Vec<String>,
    label: &str,
    text: &str,
    max_lines: usize,
    max_chars: usize,
) -> bool {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n').trim();
    if trimmed.is_empty() {
        return false;
    }

    let (preview, chars_truncated) = truncate_by_chars(trimmed, max_chars);
    let parts = preview.lines().collect::<Vec<_>>();
    if parts.is_empty() {
        return false;
    }

    lines.push(format!("  {label}: {}", parts[0]));
    for line in parts.iter().skip(1).take(max_lines.saturating_sub(1)) {
        lines.push(format!("    {line}"));
    }

    let hidden_lines = parts.len().saturating_sub(max_lines);
    if hidden_lines > 0 || chars_truncated {
        let mut suffix = String::new();
        if hidden_lines > 0 {
            suffix.push_str(&format!("{hidden_lines} more lines"));
        }
        if chars_truncated {
            if !suffix.is_empty() {
                suffix.push_str(", ");
            }
            suffix.push_str("truncated");
        }
        lines.push(format!("    ... ({suffix})"));
    }

    true
}

fn truncate_by_chars(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }

    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }

    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    (output, true)
}

fn format_apply_patch_result_lines(tool: &str, payload: &Value, is_zh: bool) -> Vec<String> {
    let result = extract_tool_result_object(payload);
    let data = extract_tool_result_data(result);
    let ok = result.get("ok").and_then(Value::as_bool);
    let tool_is_zh = is_zh;
    let changed_files =
        number_value(data.get("changed_files")).max(number_value(result.get("changed_files")));
    let added = number_value(data.get("added"));
    let updated = number_value(data.get("updated"));
    let deleted = number_value(data.get("deleted"));
    let moved = number_value(data.get("moved"));
    let hunks =
        number_value(data.get("hunks_applied")).max(number_value(result.get("hunks_applied")));
    let mut lines = Vec::new();
    let files = data.get("files").and_then(Value::as_array);
    if ok == Some(false) {
        lines.push(if tool_is_zh {
            "✘ 补丁应用失败".to_string()
        } else {
            "✘ Failed to apply patch".to_string()
        });
    } else {
        let noun = if changed_files == 1 {
            if tool_is_zh {
                "1 个文件".to_string()
            } else {
                "1 file".to_string()
            }
        } else if tool_is_zh {
            format!("{changed_files} 个文件")
        } else {
            format!("{changed_files} files")
        };
        let mut metrics = Vec::new();
        if added > 0 {
            metrics.push(format!("+{added}"));
        }
        if updated > 0 {
            metrics.push(format!("~{updated}"));
        }
        if deleted > 0 {
            metrics.push(format!("-{deleted}"));
        }
        if moved > 0 {
            metrics.push(format!("↦{moved}"));
        }
        if hunks > 0 {
            metrics.push(format!("{hunks} hunks"));
        }
        let metric_suffix = if metrics.is_empty() {
            String::new()
        } else {
            format!(" ({})", metrics.join(", "))
        };
        lines.push(if tool_is_zh {
            format!("• 已修改 {noun}{metric_suffix}")
        } else {
            format!("• Edited {noun}{metric_suffix}")
        });
    }
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_tree_line(
            &mut lines,
            if tool_is_zh {
                format!("错误: {error}")
            } else {
                format!("Error: {error}")
            },
        );
    }
    if let Some(code) = data
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_tree_line(&mut lines, format!("code: {code}"));
    }
    if let Some(hint) = data
        .get("hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_tree_line(
            &mut lines,
            if tool_is_zh {
                format!("提示: {hint}")
            } else {
                format!("Hint: {hint}")
            },
        );
    }

    if let Some(files) = files {
        let mut appended = 0usize;
        for file in files.iter().take(MAX_PATCH_RESULT_FILES) {
            if let Some(line) = extract_apply_patch_file_line(file) {
                push_tree_line(&mut lines, line.trim().to_string());
                appended = appended.saturating_add(1);
            }
        }
        if files.len() > appended {
            push_tree_line(
                &mut lines,
                format!("... ({} more)", files.len().saturating_sub(appended)),
            );
        }
    }

    if lines.len() == 1 {
        push_tree_line(
            &mut lines,
            wunder_server::tool_result_display::tool_result_display(tool, payload, false),
        );
    }
    lines
}

fn format_execute_command_result_lines(tool: &str, result: &Value, is_zh: bool) -> Vec<String> {
    let data = result.get("data").unwrap_or(result);
    let Some(first) = data
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let command = first
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let returncode = number_value(first.get("returncode")).max(number_value(
        result.get("meta").and_then(|meta| meta.get("exit_code")),
    ));
    let duration_ms = number_value(result.get("meta").and_then(|meta| meta.get("duration_ms")));
    let tool_is_zh = is_zh;
    let header = if returncode != 0 {
        if tool_is_zh {
            format!("✘ {tool} 失败")
        } else {
            format!("✘ {tool} failed")
        }
    } else if tool_is_zh {
        format!("• 已完成 {tool}")
    } else {
        format!("• Completed {tool}")
    };
    let mut metrics = Vec::new();
    metrics.push(format!("exit={returncode}"));
    if duration_ms > 0 {
        metrics.push(format!("{duration_ms}ms"));
    }
    let mut lines = vec![if metrics.is_empty() {
        header
    } else {
        format!("{header} ({})", metrics.join(", "))
    }];
    if !command.is_empty() {
        push_tree_line(&mut lines, format!("cmd: {command}"));
    }

    let stdout = first
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = first
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut has_output = false;
    if returncode == 0 {
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 6, 900);
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 4, 300);
    } else {
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 6, 900);
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 4, 300);
    }

    if !has_output {
        push_tree_line(&mut lines, "output: <empty>".to_string());
    }
    lines
}

fn command_session_status_line(view: &CommandSessionView) -> String {
    let mut metrics = Vec::new();
    if let Some(exit_code) = view.exit_code {
        metrics.push(format!("exit={exit_code}"));
    }
    if let Some(duration_ms) = view.duration_ms.filter(|value| *value > 0) {
        metrics.push(format!("{duration_ms}ms"));
    }
    if view.timed_out {
        metrics.push("timeout".to_string());
    }
    let suffix = if metrics.is_empty() {
        String::new()
    } else {
        format!(" ({})", metrics.join(", "))
    };
    let command = view.command.trim();
    let command_suffix = if command.is_empty() {
        String::new()
    } else {
        format!(" `{command}`")
    };
    match view.status {
        CommandSessionDisplayStatus::FailedToStart => {
            format!("- Command failed to start{command_suffix}{suffix}")
        }
        CommandSessionDisplayStatus::Exited if view.success() => {
            format!("- Command finished{command_suffix}{suffix}")
        }
        CommandSessionDisplayStatus::Exited => {
            format!("- Command failed{command_suffix}{suffix}")
        }
        CommandSessionDisplayStatus::Running | CommandSessionDisplayStatus::Pending => {
            String::new()
        }
    }
}

fn command_session_result_lines(view: &CommandSessionView) -> Vec<String> {
    let mut lines = Vec::new();
    let status = command_session_status_line(view);
    if !status.is_empty() {
        lines.push(status);
    }
    if let Some(cwd) = view
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("  cwd: {cwd}"));
    }
    if let Some(error) = view
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("  error: {error}"));
    }
    lines.extend(command_session_output_lines(view));
    if lines.len() == 1 && !view.has_output() {
        lines.push("  output: <empty>".to_string());
    }
    lines
}

fn command_session_output_lines(view: &CommandSessionView) -> Vec<String> {
    let mut lines = Vec::new();
    let first_stream = if !view.pty.trim().is_empty() {
        Some(("pty", view.pty.as_str(), 10, 1_500))
    } else if !view.success() && !view.stderr.trim().is_empty() {
        Some(("stderr", view.stderr.as_str(), 8, 1_200))
    } else if !view.stdout.trim().is_empty() {
        Some(("stdout", view.stdout.as_str(), 8, 1_200))
    } else {
        None
    };
    if let Some((label, text, max_lines, max_chars)) = first_stream {
        lines.push(format!("  {label}:"));
        for line in compact_output_lines(text, max_lines, max_chars) {
            lines.push(format!("    {line}"));
        }
    }

    let secondary = if first_stream.is_some_and(|(label, _, _, _)| label == "stderr") {
        (!view.stdout.trim().is_empty()).then_some(("stdout", view.stdout.as_str(), 4, 500))
    } else {
        (!view.stderr.trim().is_empty()).then_some(("stderr", view.stderr.as_str(), 4, 500))
    };
    if let Some((label, text, max_lines, max_chars)) = secondary {
        lines.push(format!("  {label}:"));
        for line in compact_output_lines(text, max_lines, max_chars) {
            lines.push(format!("    {line}"));
        }
    }
    lines
}

fn compact_output_lines(text: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n');
    if trimmed.trim().is_empty() {
        return Vec::new();
    }
    let (preview, chars_truncated) = compact_text_preview(trimmed, max_chars);
    let lines = preview.lines().map(ToString::to_string).collect::<Vec<_>>();
    collapse_middle_lines(lines, max_lines, chars_truncated)
}

fn collapse_middle_lines(
    lines: Vec<String>,
    max_lines: usize,
    chars_truncated: bool,
) -> Vec<String> {
    if max_lines == 0 {
        return vec![ellipsis_line(lines.len(), chars_truncated)];
    }
    if lines.len() <= max_lines && !chars_truncated {
        return lines;
    }
    if max_lines == 1 {
        return vec![ellipsis_line(lines.len(), chars_truncated)];
    }
    let keep_head = max_lines / 2;
    let keep_tail = max_lines.saturating_sub(keep_head + 1);
    let omitted = lines.len().saturating_sub(keep_head + keep_tail);
    let mut collapsed = Vec::new();
    collapsed.extend(lines.iter().take(keep_head).cloned());
    collapsed.push(ellipsis_line(omitted, chars_truncated));
    if keep_tail > 0 {
        let start = lines.len().saturating_sub(keep_tail);
        collapsed.extend(lines.into_iter().skip(start));
    }
    collapsed
}

fn ellipsis_line(omitted_lines: usize, chars_truncated: bool) -> String {
    let mut parts = Vec::new();
    if omitted_lines > 0 {
        parts.push(format!("{omitted_lines} lines"));
    }
    if chars_truncated {
        parts.push("truncated".to_string());
    }
    if parts.is_empty() {
        "...".to_string()
    } else {
        format!("... omitted {} ...", parts.join(", "))
    }
}

fn take_chars(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }
    let mut output = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if count >= max_chars {
            return (output, true);
        }
        output.push(ch);
        count = count.saturating_add(1);
    }
    (output, false)
}

fn append_tail(target: &mut String, text: &str, max_chars: usize) {
    target.push_str(text);
    let count = target.chars().count();
    if count <= max_chars {
        return;
    }
    let start = count.saturating_sub(max_chars);
    *target = target.chars().skip(start).collect();
}

fn format_generic_tool_result_lines(tool: &str, payload: &Value, is_zh: bool) -> Vec<String> {
    let result = extract_tool_result_object(payload);
    let ok = result.get("ok").and_then(Value::as_bool);
    let repair = result
        .get("meta")
        .and_then(|value| value.get("repair"))
        .or_else(|| payload.get("repair"));
    if is_execute_command_tool_name(tool) {
        let mut lines = format_execute_command_result_lines(tool, result, is_zh);
        if !lines.is_empty() {
            if let Some(badge) = repair.and_then(format_repair_badge) {
                if let Some(header) = lines.first_mut() {
                    header.push(' ');
                    header.push_str(badge);
                }
            }
            if let Some(repair) = repair {
                if let Some(summary) = format_repair_summary(repair) {
                    push_tree_line(&mut lines, format!("note: {summary}"));
                }
            }
            return lines;
        }
    }

    let tool_is_zh = is_zh;
    let mut header = if ok == Some(false) {
        if tool_is_zh {
            format!("✘ {tool} 失败")
        } else {
            format!("✘ {tool} failed")
        }
    } else if tool_is_zh {
        format!("• 已完成 {tool}")
    } else {
        format!("• Completed {tool}")
    };
    if let Some(badge) = repair.and_then(format_repair_badge) {
        header.push(' ');
        header.push_str(badge);
    }

    let mut lines = vec![header];
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_tree_line(
            &mut lines,
            if tool_is_zh {
                format!("错误: {error}")
            } else {
                format!("Error: {error}")
            },
        );
    }
    if let Some(repair) = repair {
        if let Some(summary) = format_repair_summary(repair) {
            push_tree_line(&mut lines, format!("note: {summary}"));
        }
    }
    if let Some(display) = summarize_tool_result(payload, tool_is_zh) {
        if let Some(summary) = display.summary.filter(|value| !value.is_empty()) {
            push_tree_line(&mut lines, summary);
        }
        for detail in display.details {
            let text = if let Some(label) = detail.label.filter(|value| !value.is_empty()) {
                format!("{label} {}", detail.text)
            } else {
                detail.text
            };
            push_tree_line(&mut lines, text);
        }
    } else if lines.len() == 1 {
        push_tree_line(
            &mut lines,
            wunder_server::tool_result_display::tool_result_display(tool, payload, false),
        );
    }
    lines
}

fn tool_only_completion_fallback(is_zh: bool) -> String {
    if is_zh {
        "已完成本轮任务，结果见上方工具输出。".to_string()
    } else {
        "Done. Review the tool results above.".to_string()
    }
}

fn format_repair_summary(repair: &Value) -> Option<String> {
    let strategy = repair.get("strategy").and_then(Value::as_str).unwrap_or("");
    let count = repair.get("count").and_then(Value::as_u64).unwrap_or(0);
    match strategy {
        "sanitize_before_request" if count > 0 => Some(format!(
            "sanitized {count} malformed tool-call argument payload(s)"
        )),
        "lossy_json_string_repair" => {
            Some("recovered malformed JSON arguments before execution".to_string())
        }
        "raw_arguments_wrapped" => {
            Some("wrapped non-JSON arguments before sending them upstream".to_string())
        }
        "non_object_arguments_wrapped" => {
            Some("wrapped non-object arguments into JSON before sending them upstream".to_string())
        }
        _ => None,
    }
}

fn compact_json(value: &Value) -> String {
    compact_json_within(value, MAX_INLINE_JSON_CHARS)
}

/// Inline JSON preview shared by every rendering surface. The budget is measured
/// in bytes, so the cut has to back off to a char boundary - a tool payload of
/// multibyte text would otherwise end the slice inside a code point and panic.
pub(crate) fn compact_json_within(value: &Value, max_bytes: usize) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > max_bytes {
        let mut safe_boundary = max_bytes;
        while safe_boundary > 0 && !text.is_char_boundary(safe_boundary) {
            safe_boundary = safe_boundary.saturating_sub(1);
        }
        text.truncate(safe_boundary);
        text.push_str("...");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inline_json_preview_cuts_on_a_char_boundary() {
        // 180 bytes would land inside a 3-byte code point here; the byte budget
        // is deliberately hostile to multibyte payloads.
        let value = serde_json::json!({ "note": "你".repeat(400) });
        let output = compact_json(&value);
        assert!(output.ends_with("..."));
        assert!(output.chars().count() < 200);
    }

    #[test]
    fn colour_is_opt_in_and_only_touches_the_progress_stream() {
        let plain = StreamRenderer::new(false, "en-US");
        assert_eq!(plain.dim("line"), "line", "colour is off by default");

        let colored = StreamRenderer::new(false, "en-US").with_color(true);
        assert!(colored.dim("line").contains("\u{1b}[2m"));
        assert!(colored.green("ok").contains("\u{1b}[32;1m"));
        assert!(colored.red("bad").contains("\u{1b}[31;1m"));
    }

    #[test]
    fn apply_patch_result_lines_include_change_markers() {
        let payload = serde_json::json!({
            "tool": "应用补丁",
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 2,
                    "hunks_applied": 3,
                    "files": [
                        { "action": "add", "path": "src/new.rs" },
                        { "action": "delete", "path": "src/old.rs" }
                    ]
                }
            }
        });
        let lines = format_apply_patch_result_lines("应用补丁", &payload, true);
        assert!(lines[0].contains("已修改 2 个文件"));
        assert!(lines.iter().any(|line| line.contains("A src/new.rs")));
        assert!(lines.iter().any(|line| line.contains("D src/old.rs")));
    }

    #[test]
    fn apply_patch_result_lines_include_error_code_and_hint() {
        let payload = serde_json::json!({
            "tool": "apply_patch",
            "result": {
                "ok": false,
                "error": "Patch apply failed",
                "data": {
                    "error_code": "PATCH_CONTEXT_NOT_FOUND",
                    "hint": "Read file and retry"
                }
            }
        });
        let lines = format_apply_patch_result_lines("apply_patch", &payload, false);
        assert!(lines
            .iter()
            .any(|line| line.contains("PATCH_CONTEXT_NOT_FOUND")));
        assert!(lines
            .iter()
            .any(|line| line.contains("Read file and retry")));
    }

    #[test]
    fn tool_call_line_marks_repaired_arguments() {
        let line = format_tool_call_line(
            "执行命令",
            &serde_json::json!({ "content": "python3 demo.py" }),
            Some(&serde_json::json!({ "strategy": "lossy_json_string_repair" })),
            true,
        );
        assert!(line.contains("调用"));
        assert!(line.contains("python3 demo.py"));
        assert!(line.contains("args repaired"));
    }

    #[test]
    fn apply_patch_tool_call_line_uses_real_diff_preview() {
        let line = format_tool_call_line(
            "apply_patch",
            &serde_json::json!({
                "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
            }),
            None,
            false,
        );
        assert!(line.contains("files=1, +1, -1"));
        assert!(line.contains("diff src/main.rs"));
        assert!(line.contains("@@"));
        assert!(line.contains("- old"));
        assert!(line.contains("+ new"));
        assert!(line.contains("    diff src/main.rs"));
        assert!(line.contains("      @@"));
    }

    #[test]
    fn generic_tool_result_lines_include_repair_note() {
        let lines = format_generic_tool_result_lines(
            "执行命令",
            &serde_json::json!({
                "tool": "执行命令",
                "ok": false,
                "error": "命令执行失败。",
                "meta": {
                    "repair": {
                        "strategy": "lossy_json_string_repair"
                    }
                }
            }),
            true,
        );
        assert!(lines.iter().any(|line| line.contains("args repaired")));
        assert!(lines
            .iter()
            .any(|line| line.contains("recovered malformed JSON arguments")));
    }

    #[test]
    fn generic_tool_result_lines_render_list_preview_instead_of_raw_json() {
        let lines = format_generic_tool_result_lines(
            "list_files",
            &serde_json::json!({
                "tool": "list_files",
                "items": ["src/", "src/main.rs", "Cargo.toml"]
            }),
            false,
        );
        assert!(lines.iter().any(|line| line.contains("3 items")));
        assert!(lines.iter().any(|line| line.contains("src/main.rs")));
        assert!(!lines.iter().any(|line| line.contains("\"items\"")));
    }

    #[test]
    fn unknown_results_keep_user_text_without_protocol_json() {
        let output = format_generic_tool_result_lines(
            "custom_tool",
            &serde_json::json!({
                "ok": true, "data": {"message": "Useful result", "query_handle": "hidden", "cursor": 999}
            }),
            false,
        )
        .join("\n");
        assert!(output.contains("Useful result"));
        assert!(!output.contains("hidden"));
        assert!(!output.contains("999"));
    }

    #[test]
    fn command_live_chunk_uses_indented_terminal_block() {
        let mut state = CommandLivePrintState::default();
        let first = format_command_live_chunk(&mut state, "one\ntwo");
        assert_eq!(first, "  output:\n    one\n    two");
        assert!(command_live_line_open(&state));

        let second = format_command_live_chunk(&mut state, "\nthree\n");
        assert_eq!(second, "\n    three\n");
        assert!(!command_live_line_open(&state));
    }

    #[test]
    fn tool_only_completion_fallback_is_human_readable() {
        assert_eq!(
            tool_only_completion_fallback(false),
            "Done. Review the tool results above."
        );
        assert_eq!(
            tool_only_completion_fallback(true),
            "已完成本轮任务，结果见上方工具输出。"
        );
    }
}
