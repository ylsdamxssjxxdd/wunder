//! The `wunder-cli exec --json` contract.
//!
//! Scripts get one JSON object per line on stdout with the same skeleton the
//! reference implementation uses: `thread.started`, `turn.started`,
//! `item.started` / `item.updated` / `item.completed`, `turn.completed`,
//! `turn.failed` and `error`. Items carry a stable id and a typed body, so a
//! consumer never has to parse the agent's own event vocabulary or guess which
//! tool call a result belongs to.
//!
//! Everything here is bounded: item text is capped and only the tail of a long
//! output is kept, because a JSONL consumer reads lines, not a transcript.

use serde_json::{json, Value};

use crate::command_session_display::{CommandSessionDisplayState, CommandSessionDisplayStatus};
use crate::tool_presentation::{present, target_argument, ToolAccess};
use wunder_server::schemas::StreamEvent;

/// Per-item text budget. Long command output keeps its tail; the count of what
/// was dropped is reported instead of the bytes themselves.
const MAX_ITEM_TEXT_CHARS: usize = 8_000;
const MAX_ITEM_OUTPUT_CHARS: usize = 8_000;
const TRUNCATION_NOTE: &str = "\n…(output truncated)";

pub(crate) struct ExecEventWriter {
    thread_id: String,
    message_seq: u64,
    open_message: Option<OpenText>,
    commands: CommandSessionDisplayState,
    /// Open items keyed by the stable tool call id, for both plain tools and
    /// commands: a command is announced first as a tool call, so without one
    /// shared key the same command would appear twice.
    open_items: Vec<(String, ToolItem)>,
    /// Command session primary id → item id.
    command_items: Vec<(String, String)>,
    /// Calls whose item already completed; a late result for them is ignored
    /// instead of opening a second card.
    closed_calls: std::collections::VecDeque<String>,
    usage: Option<Value>,
    saw_error: bool,
}

struct OpenText {
    id: String,
    text: String,
    truncated: bool,
}

struct ToolItem {
    id: String,
    item_type: &'static str,
    title: String,
    output: String,
}

impl ExecEventWriter {
    pub(crate) fn new(thread_id: &str) -> Self {
        Self {
            thread_id: thread_id.to_string(),
            message_seq: 0,
            open_message: None,
            commands: CommandSessionDisplayState::default(),
            open_items: Vec::new(),
            command_items: Vec::new(),
            closed_calls: std::collections::VecDeque::new(),
            usage: None,
            saw_error: false,
        }
    }

    /// Opens the run: one thread, then its first turn.
    pub(crate) fn begin(&self) {
        emit(json!({"type": "thread.started", "thread_id": self.thread_id}));
        emit(json!({"type": "turn.started"}));
    }

    pub(crate) fn saw_error(&self) -> bool {
        self.saw_error
    }

    /// Feed one engine event. Returns the final answer once the turn settles.
    pub(crate) fn handle(&mut self, event: &StreamEvent) -> Option<Value> {
        let payload = payload_of(&event.data);
        // The runtime collapses every `*_delta` frame into `thread_item_delta`
        // and keeps the semantic stream type in `source_event`; resolve it back
        // so live command/tool output keeps streaming.
        let kind = payload
            .get("source_event")
            .and_then(Value::as_str)
            .unwrap_or_else(|| event.event.as_str());
        match kind {
            "llm_output_delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.append_message(delta);
                    }
                }
                None
            }
            "llm_output" => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        self.set_message(content);
                    }
                }
                None
            }
            "tool_call" => {
                self.start_tool(payload);
                None
            }
            "tool_output_delta" => {
                self.append_tool_output(payload);
                None
            }
            "tool_result" => {
                self.complete_tool(payload);
                None
            }
            "command_session_start" => {
                self.command_start(payload);
                None
            }
            "command_session_delta" => {
                self.command_delta(payload);
                None
            }
            "command_session_status" | "command_session_exit" | "command_session_summary" => {
                self.command_status(payload);
                None
            }
            "approval_request" => {
                self.emit_approval(payload, "requested");
                None
            }
            "approval_resolved" | "approval_result" => {
                let status = payload
                    .get("status")
                    .or_else(|| payload.get("decision"))
                    .and_then(Value::as_str)
                    .unwrap_or("resolved");
                self.emit_approval(payload, status);
                None
            }
            "error" => {
                self.saw_error = true;
                let message = crate::error_display::format_error_message(payload)
                    .unwrap_or_else(|| compact(payload));
                emit(json!({"type": "error", "message": message}));
                None
            }
            "final" => self.finish_turn(payload),
            _ => None,
        }
    }

    /// Close any open item and report how the turn ended.
    pub(crate) fn finish(&mut self, failure: Option<&str>) {
        self.close_message();
        self.close_open_commands();
        match failure {
            None => {
                let mut body = json!({"type": "turn.completed"});
                if let Some(usage) = self.usage.take() {
                    body["usage"] = usage;
                }
                emit(body);
            }
            Some(message) => {
                emit(json!({"type": "turn.failed", "message": message}));
            }
        }
    }

    fn finish_turn(&mut self, payload: &Value) -> Option<Value> {
        self.close_message();
        self.close_open_commands();
        let usage = payload
            .get("usage")
            .cloned()
            .filter(|value| !value.is_null());
        if usage.is_some() {
            self.usage = usage.clone();
        }
        Some(json!({
            "answer": payload.get("answer").and_then(Value::as_str).unwrap_or_default(),
            "stop_reason": payload.get("stop_reason").cloned().unwrap_or(Value::Null),
            "usage": usage.unwrap_or(Value::Null),
        }))
    }

    fn append_message(&mut self, delta: &str) {
        if self.open_message.is_none() {
            self.message_seq = self.message_seq.saturating_add(1);
            let id = format!("msg_{}", self.message_seq);
            let item = OpenText {
                id: id.clone(),
                text: String::new(),
                truncated: false,
            };
            self.open_message = Some(item);
            emit(json!({
                "type": "item.started",
                "item": {"id": id, "item_type": "agent_message", "status": "in_progress"},
            }));
        }
        let Some(item) = self.open_message.as_mut() else {
            return;
        };
        push_bounded(
            &mut item.text,
            delta,
            MAX_ITEM_TEXT_CHARS,
            &mut item.truncated,
        );
        let body = message_item(item);
        emit(json!({"type": "item.updated", "item": body}));
    }

    fn set_message(&mut self, content: &str) {
        if self.open_message.is_none() {
            self.append_message(content);
            return;
        }
        if let Some(item) = self.open_message.as_mut() {
            item.text.clear();
            item.truncated = false;
            push_bounded(
                &mut item.text,
                content,
                MAX_ITEM_TEXT_CHARS,
                &mut item.truncated,
            );
            let body = message_item(item);
            emit(json!({"type": "item.updated", "item": body}));
        }
    }

    fn close_message(&mut self) {
        let Some(item) = self.open_message.take() else {
            return;
        };
        let mut body = message_item(&item);
        body["status"] = json!("completed");
        emit(json!({"type": "item.completed", "item": body}));
    }

    fn start_tool(&mut self, payload: &Value) {
        let tool = payload
            .get("tool")
            .or_else(|| payload.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let item_type = item_type_for(tool);
        let args = payload
            .get("args")
            .or_else(|| payload.get("arguments"))
            .cloned()
            .unwrap_or(Value::Null);
        let call_id = payload
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("tool_{}", self.open_items.len() + 1));
        let title = target_argument(&args)
            .map(|(_, value)| value)
            .unwrap_or_else(|| tool.to_string());
        if self.has_open_item(call_id.as_str()) || self.call_is_closed(call_id.as_str()) {
            return;
        }
        let id = format!("item_{}", self.open_items.len() + 1);
        emit(json!({
            "type": "item.started",
            "item": {
                "id": id,
                "item_type": item_type,
                "tool": tool,
                "title": title,
                "status": "in_progress",
            },
        }));
        self.open_items.push((
            call_id,
            ToolItem {
                id,
                item_type,
                title,
                output: String::new(),
            },
        ));
    }

    fn append_tool_output(&mut self, payload: &Value) {
        let call_id = payload
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let delta = payload
            .get("delta")
            .or_else(|| payload.get("output"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if delta.is_empty() {
            return;
        }
        let Some((_, item)) = self
            .open_items
            .iter_mut()
            .find(|(id, _)| id.as_str() == call_id)
        else {
            return;
        };
        let mut truncated = false;
        push_bounded(
            &mut item.output,
            delta,
            MAX_ITEM_OUTPUT_CHARS,
            &mut truncated,
        );
        let (id, item_type, title, output) = (
            item.id.clone(),
            item.item_type,
            item.title.clone(),
            item.output.clone(),
        );
        emit(json!({
            "type": "item.updated",
            "item": {
                "id": id,
                "item_type": item_type,
                "title": title,
                "output": output,
                "truncated": truncated,
                "status": "in_progress",
            },
        }));
    }

    fn complete_tool(&mut self, payload: &Value) {
        let call_id = payload
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        // The command path may already have closed this call; a late result for
        // it must not open a second item.
        if self.call_is_closed(call_id.as_str()) {
            return;
        }
        let Some(index) = self
            .open_items
            .iter()
            .position(|(id, _)| id.as_str() == call_id)
        else {
            // A result without a matching call still becomes one completed item:
            // dropping it would hide work the agent actually did.
            let tool = payload
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let id = format!("item_{}", self.open_items.len() + 1);
            self.open_items.push((
                call_id.clone(),
                ToolItem {
                    id,
                    item_type: item_type_for(&tool),
                    title: tool,
                    output: String::new(),
                },
            ));
            let index = self.open_items.len() - 1;
            self.complete_tool_at(index, payload);
            self.mark_call_closed(call_id.as_str());
            return;
        };
        self.complete_tool_at(index, payload);
        self.mark_call_closed(call_id.as_str());
    }

    fn complete_tool_at(&mut self, index: usize, payload: &Value) {
        let (_, item) = &mut self.open_items[index];
        let ok = payload
            .get("ok")
            .and_then(Value::as_bool)
            .or_else(|| {
                payload
                    .get("result")
                    .and_then(|result| result.get("ok"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(true);
        let error = payload
            .get("error")
            .or_else(|| payload.get("result").and_then(|result| result.get("error")))
            .and_then(Value::as_str)
            .map(str::to_string);
        let summary = crate::tool_display::summarize_tool_result(payload, false)
            .and_then(|summary| summary.summary);
        let mut output = item.output.clone();
        if output.trim().is_empty() {
            if let Some(summary) = summary.as_deref() {
                let mut truncated = false;
                push_bounded(&mut output, summary, MAX_ITEM_OUTPUT_CHARS, &mut truncated);
            }
        }
        let body = json!({
            "id": item.id,
            "item_type": item.item_type,
            "title": item.title,
            "status": if ok { "completed" } else { "failed" },
            "ok": ok,
            "output": output,
            "error": error,
        });
        let key = self.open_items[index].0.clone();
        self.open_items.retain(|(existing, _)| existing != &key);
        self.command_items
            .retain(|(_, item_id)| *item_id != body["id"]);
        emit(json!({"type": "item.completed", "item": body}));
    }

    fn has_open_item(&self, call_id: &str) -> bool {
        self.open_items
            .iter()
            .any(|(existing, _)| existing == call_id)
    }

    fn call_is_closed(&self, call_id: &str) -> bool {
        self.closed_calls.iter().any(|existing| existing == call_id)
    }

    fn mark_call_closed(&mut self, call_id: &str) {
        const MAX_CLOSED_CALLS: usize = 128;
        if call_id.is_empty() || self.call_is_closed(call_id) {
            return;
        }
        if self.closed_calls.len() >= MAX_CLOSED_CALLS {
            self.closed_calls.pop_front();
        }
        self.closed_calls.push_back(call_id.to_string());
    }

    fn command_item_id_for_call(&self, call_id: &str) -> Option<String> {
        self.open_items
            .iter()
            .find(|(existing, _)| existing == call_id)
            .map(|(_, item)| item.id.clone())
    }

    fn command_start(&mut self, payload: &Value) {
        let Some(update) = self.commands.register_start(payload) else {
            return;
        };
        let view = update.view;
        if self
            .command_items
            .iter()
            .any(|(primary, _)| primary == &view.primary_id)
        {
            return;
        }
        // The command was already announced as a tool call: promote that item
        // instead of opening a second card for the same process.
        if let Some(call_id) = view.tool_call_id.as_deref() {
            if let Some(id) = self.command_item_id_for_call(call_id) {
                emit(json!({
                    "type": "item.updated",
                    "item": {
                        "id": id,
                        "item_type": "command_execution",
                        "command": view.command,
                        "cwd": view.cwd,
                        "status": "in_progress",
                    },
                }));
                self.command_items.push((view.primary_id, id));
                return;
            }
        }
        let id = format!("cmd_{}", self.command_items.len() + 1);
        emit(json!({
            "type": "item.started",
            "item": {
                "id": id,
                "item_type": "command_execution",
                "command": view.command,
                "cwd": view.cwd,
                "status": "in_progress",
            },
        }));
        if let Some(call_id) = view.tool_call_id.clone() {
            // Track the promoted item under the call id as well, so a later
            // tool_result closes this same item.
            self.open_items.push((
                call_id,
                ToolItem {
                    id: id.clone(),
                    item_type: "command_execution",
                    title: view.command.clone(),
                    output: String::new(),
                },
            ));
        }
        self.command_items.push((view.primary_id, id));
    }

    fn command_delta(&mut self, payload: &Value) {
        let Some(update) = self.commands.register_delta(payload) else {
            return;
        };
        let view = update.view;
        let Some(id) = self.command_item_id(&view.primary_id) else {
            return;
        };
        let output = combined_output(&view);
        emit(json!({
            "type": "item.updated",
            "item": {
                "id": id,
                "item_type": "command_execution",
                "command": view.command,
                "output": output,
                "status": "in_progress",
            },
        }));
    }

    fn command_status(&mut self, payload: &Value) {
        let Some(update) = self.commands.register_status(payload) else {
            return;
        };
        let view = update.view;
        if !view.is_terminal() {
            return;
        }
        let Some(id) = self.command_item_id(&view.primary_id) else {
            return;
        };
        let body = json!({
            "id": id,
            "item_type": "command_execution",
            "command": view.command,
            "cwd": view.cwd,
            "status": match view.status {
                CommandSessionDisplayStatus::Exited => "completed",
                _ => "failed",
            },
            "exit_code": view.exit_code,
            "timed_out": view.timed_out,
            "duration_ms": view.duration_ms,
            "output": combined_output(&view),
            "error": view.error,
        });
        self.command_items
            .retain(|(primary, _)| primary != &view.primary_id);
        if let Some(call_id) = view.tool_call_id.as_deref() {
            self.open_items.retain(|(existing, _)| existing != call_id);
            self.mark_call_closed(call_id);
        }
        emit(json!({"type": "item.completed", "item": body}));
    }

    /// A command whose session never reported a terminal status (interrupted
    /// turn) is closed as failed rather than left dangling.
    fn close_open_commands(&mut self) {
        let open = std::mem::take(&mut self.command_items);
        for (primary_id, id) in open {
            let Some(view) = self.commands.view_for(&primary_id) else {
                continue;
            };
            emit(json!({
                "type": "item.completed",
                "item": {
                    "id": id,
                    "item_type": "command_execution",
                    "command": view.command,
                    "status": "failed",
                    "exit_code": view.exit_code,
                    "output": combined_output(&view),
                    "error": view.error.or_else(|| Some("the turn ended before the command reported a status".to_string())),
                },
            }));
        }
    }

    fn emit_approval(&self, payload: &Value, status: &str) {
        let tool = payload
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let summary = payload
            .get("summary")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        emit(json!({
            "type": "item.completed",
            "item": {
                "id": format!("approval_{tool}"),
                "item_type": "approval",
                "tool": tool,
                "status": status,
                "summary": summary,
            },
        }));
    }

    fn command_item_id(&self, primary_id: &str) -> Option<String> {
        self.command_items
            .iter()
            .find(|(primary, _)| primary == primary_id)
            .map(|(_, id)| id.clone())
    }
}

fn message_item(item: &OpenText) -> Value {
    json!({
        "id": item.id,
        "item_type": "agent_message",
        "text": item.text,
        "truncated": item.truncated,
        "status": "in_progress",
    })
}

fn combined_output(view: &crate::command_session_display::CommandSessionView) -> String {
    let mut combined = String::new();
    let mut truncated = false;
    for part in [
        view.stdout.as_str(),
        view.stderr.as_str(),
        view.pty.as_str(),
    ] {
        if part.trim().is_empty() {
            continue;
        }
        push_bounded(&mut combined, part, MAX_ITEM_OUTPUT_CHARS, &mut truncated);
    }
    combined
}

/// Map a tool name onto the item vocabulary a script can switch on, reusing the
/// same classifier the transcript cards use so the two never disagree.
fn item_type_for(tool_name: &str) -> &'static str {
    let normalized = tool_name.trim().to_ascii_lowercase();
    if normalized.contains("apply_patch") || normalized.contains("patch") {
        return "file_change";
    }
    if normalized.contains("search")
        || normalized.contains("fetch")
        || normalized.contains("browse")
    {
        return "web_search";
    }
    match present(tool_name).access {
        ToolAccess::Execute => "command_execution",
        ToolAccess::Mutate => "file_change",
        ToolAccess::Delegate => "mcp_tool_call",
        ToolAccess::Explore | ToolAccess::Other => "tool",
    }
}

fn payload_of(data: &Value) -> &Value {
    data.get("data").unwrap_or(data)
}

fn push_bounded(target: &mut String, text: &str, limit: usize, truncated: &mut bool) {
    if target.chars().count() >= limit {
        *truncated = true;
        return;
    }
    target.push_str(text);
    if target.chars().count() > limit {
        let kept: String = target.chars().take(limit).collect();
        *target = kept;
        target.push_str(TRUNCATION_NOTE);
        *truncated = true;
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn emit(value: Value) {
    if let Ok(line) = serde_json::to_string(&value) {
        println!("{line}");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

/// One non-streamed answer expressed in the same contract, so `--json` output
/// looks the same whether or not streaming was requested.
pub(crate) fn emit_single_turn(
    thread_id: &str,
    answer: &str,
    usage: Option<Value>,
    stop_reason: Option<&str>,
) {
    emit(json!({"type": "thread.started", "thread_id": thread_id}));
    emit(json!({"type": "turn.started"}));
    emit(json!({
        "type": "item.completed",
        "item": {
            "id": "msg_1",
            "item_type": "agent_message",
            "text": answer,
            "status": "completed",
        },
    }));
    let mut completed = json!({"type": "turn.completed"});
    if let Some(usage) = usage.filter(|value| !value.is_null()) {
        completed["usage"] = usage;
    }
    if let Some(stop_reason) = stop_reason {
        completed["stop_reason"] = json!(stop_reason);
    }
    emit(completed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str, data: Value) -> StreamEvent {
        StreamEvent {
            event: name.to_string(),
            data,
            id: None,
            timestamp: None,
        }
    }

    #[test]
    fn item_types_follow_the_shared_tool_classifier() {
        assert_eq!(item_type_for("execute_command"), "command_execution");
        assert_eq!(item_type_for("apply_patch"), "file_change");
        assert_eq!(item_type_for("write_file"), "file_change");
        assert_eq!(item_type_for("web_search"), "web_search");
        assert_eq!(item_type_for("mcp__demo__lookup"), "mcp_tool_call");
        assert_eq!(item_type_for("read_file"), "tool");
        assert_eq!(item_type_for("something_unknown"), "tool");
    }

    #[test]
    fn a_delta_opens_one_message_item_and_the_final_closes_it() {
        let mut writer = ExecEventWriter::new("thread-1");
        writer.handle(&event(
            "llm_output_delta",
            json!({"data": {"delta": "hello "}}),
        ));
        writer.handle(&event(
            "llm_output_delta",
            json!({"data": {"delta": "world"}}),
        ));
        let item = writer.open_message.as_ref().expect("one open message item");
        assert_eq!(item.text, "hello world");
        assert_eq!(item.id, "msg_1", "deltas accumulate into a single item");

        let finished = writer.handle(&event(
            "final",
            json!({"data": {"answer": "hello world", "stop_reason": "model_response"}}),
        ));
        assert!(finished.is_some());
        assert!(writer.open_message.is_none(), "the item was closed");
    }

    #[test]
    fn a_result_without_a_call_still_becomes_one_completed_item() {
        let mut writer = ExecEventWriter::new("thread-1");
        writer.handle(&event(
            "tool_result",
            json!({"data": {"tool": "read_file", "tool_call_id": "call-9", "ok": true}}),
        ));
        assert!(
            writer.open_items.is_empty(),
            "a completed result leaves no open item"
        );
        assert!(
            writer.call_is_closed("call-9"),
            "the closed call is remembered so a repeat result cannot reopen it"
        );
    }

    #[test]
    fn one_command_announced_as_a_tool_call_keeps_a_single_item() {
        let mut writer = ExecEventWriter::new("thread-1");
        writer.handle(&event(
            "tool_call",
            json!({"data": {"tool": "execute_command", "tool_call_id": "call-7", "args": {"content": "echo hi"}}}),
        ));
        assert_eq!(writer.open_items.len(), 1);
        let item_id = writer.open_items[0].1.id.clone();

        writer.handle(&event(
            "command_session_start",
            json!({"data": {
                "command_session_id": "cmd-7",
                "tool_call_id": "call-7",
                "command": "echo hi",
            }}),
        ));
        assert_eq!(
            writer.open_items.len(),
            1,
            "the command must not open a second item"
        );
        assert_eq!(
            writer.command_items[0].1, item_id,
            "the command reuses the announced item id"
        );

        writer.handle(&event(
            "command_session_exit",
            json!({"data": {"command_session_id": "cmd-7", "exit_code": 0, "status": "exited"}}),
        ));
        assert!(writer.open_items.is_empty());
        assert!(
            writer.call_is_closed("call-7"),
            "a later tool_result for the same call is ignored"
        );

        // The trailing tool_result must not produce a second completed item.
        writer.handle(&event(
            "tool_result",
            json!({"data": {"tool": "execute_command", "tool_call_id": "call-7", "ok": true}}),
        ));
        assert!(writer.open_items.is_empty());
    }

    #[test]
    fn long_output_keeps_a_bounded_tail_and_says_it_was_truncated() {
        let mut target = String::new();
        let mut truncated = false;
        push_bounded(&mut target, &"x".repeat(100), 10, &mut truncated);
        assert!(truncated);
        assert!(target.starts_with(&"x".repeat(10)));
        assert!(target.ends_with(TRUNCATION_NOTE.trim()));
        assert!(target.chars().count() < 100);
    }

    #[test]
    fn command_items_carry_the_exit_code_and_close_on_the_terminal_status() {
        let mut writer = ExecEventWriter::new("thread-1");
        writer.handle(&event(
            "command_session_start",
            json!({"data": {
                "command_session_id": "cmd-1",
                "tool_call_id": "call-1",
                "command": "cargo test",
            }}),
        ));
        assert_eq!(writer.command_items.len(), 1);
        writer.handle(&event(
            "command_session_delta",
            json!({"data": {"command_session_id": "cmd-1", "delta": "running tests\n"}}),
        ));
        writer.handle(&event(
            "command_session_exit",
            json!({"data": {"command_session_id": "cmd-1", "exit_code": 0, "status": "exited"}}),
        ));
        assert!(
            writer.command_items.is_empty(),
            "a terminal status closes the command item"
        );
    }
}
