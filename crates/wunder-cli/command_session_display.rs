use serde_json::Value;
use std::collections::{HashMap, VecDeque};

const BUFFER_HEAD_CHARS: usize = 2_000;
const BUFFER_TAIL_CHARS: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSessionDisplayStatus {
    Pending,
    Running,
    FailedToStart,
    Exited,
}

impl Default for CommandSessionDisplayStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Default)]
struct BoundedTextBuffer {
    head: String,
    head_chars: usize,
    tail: VecDeque<char>,
    total_chars: usize,
    omitted_chars: usize,
}

impl BoundedTextBuffer {
    fn append(&mut self, text: &str) {
        for ch in text.chars() {
            self.total_chars = self.total_chars.saturating_add(1);
            if self.head_chars < BUFFER_HEAD_CHARS {
                self.head.push(ch);
                self.head_chars = self.head_chars.saturating_add(1);
            } else {
                self.tail.push_back(ch);
                if self.tail.len() > BUFFER_TAIL_CHARS {
                    self.tail.pop_front();
                    self.omitted_chars = self.omitted_chars.saturating_add(1);
                }
            }
        }
    }

    fn set_text(&mut self, text: &str) {
        *self = Self::default();
        self.append(text);
    }

    fn is_empty(&self) -> bool {
        self.total_chars == 0
    }

    fn preview(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        if self.omitted_chars == 0 {
            let mut text = self.head.clone();
            text.extend(self.tail.iter());
            return text;
        }
        let tail = self.tail.iter().collect::<String>();
        format!(
            "{}\n... omitted {} chars ...\n{}",
            self.head, self.omitted_chars, tail
        )
    }
}

#[derive(Debug, Clone, Default)]
struct CommandSessionRecord {
    primary_id: String,
    command_session_id: Option<String>,
    tool_call_id: Option<String>,
    command: String,
    cwd: Option<String>,
    status: CommandSessionDisplayStatus,
    exit_code: Option<i64>,
    timed_out: bool,
    error: Option<String>,
    duration_ms: Option<i64>,
    stdout: BoundedTextBuffer,
    stderr: BoundedTextBuffer,
    pty: BoundedTextBuffer,
}

impl CommandSessionRecord {
    fn view(&self) -> CommandSessionView {
        CommandSessionView {
            primary_id: self.primary_id.clone(),
            command_session_id: self.command_session_id.clone(),
            tool_call_id: self.tool_call_id.clone(),
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            status: self.status,
            exit_code: self.exit_code,
            timed_out: self.timed_out,
            error: self.error.clone(),
            duration_ms: self.duration_ms,
            stdout: self.stdout.preview(),
            stderr: self.stderr.preview(),
            pty: self.pty.preview(),
        }
    }

    fn has_output(&self) -> bool {
        !self.stdout.is_empty() || !self.stderr.is_empty() || !self.pty.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandSessionView {
    pub(crate) primary_id: String,
    pub(crate) command_session_id: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) command: String,
    pub(crate) cwd: Option<String>,
    pub(crate) status: CommandSessionDisplayStatus,
    pub(crate) exit_code: Option<i64>,
    pub(crate) timed_out: bool,
    pub(crate) error: Option<String>,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) pty: String,
}

impl CommandSessionView {
    pub(crate) fn success(&self) -> bool {
        self.status == CommandSessionDisplayStatus::Exited
            && self.exit_code.unwrap_or(0) == 0
            && !self.timed_out
            && self
                .error
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
    }

    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            CommandSessionDisplayStatus::Exited | CommandSessionDisplayStatus::FailedToStart
        )
    }

    pub(crate) fn has_output(&self) -> bool {
        !self.stdout.trim().is_empty()
            || !self.stderr.trim().is_empty()
            || !self.pty.trim().is_empty()
    }

    pub(crate) fn ref_ids(&self) -> Vec<&str> {
        let mut refs = vec![self.primary_id.as_str()];
        if let Some(command_session_id) = self.command_session_id.as_deref() {
            if !refs.contains(&command_session_id) {
                refs.push(command_session_id);
            }
        }
        if self.command_session_id.is_some() {
            return refs;
        }
        if let Some(tool_call_id) = self.tool_call_id.as_deref() {
            if !refs.contains(&tool_call_id) {
                refs.push(tool_call_id);
            }
        }
        refs
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandSessionUpdate {
    pub(crate) view: CommandSessionView,
    pub(crate) had_output_before: bool,
}

#[derive(Debug, Default)]
pub(crate) struct CommandSessionDisplayState {
    records: HashMap<String, CommandSessionRecord>,
    aliases: HashMap<String, String>,
}

impl CommandSessionDisplayState {
    pub(crate) fn register_tool_call(&mut self, payload: &Value) -> Option<CommandSessionUpdate> {
        let tool_call_id = string_field(payload, "tool_call_id")?;
        let args = payload.get("args").unwrap_or(&Value::Null);
        let command = extract_command_input(args).unwrap_or_default();
        let update = self.upsert_for_ref(tool_call_id.as_str());
        let record = self.records.get_mut(update.primary.as_str())?;
        record.tool_call_id.get_or_insert(tool_call_id.clone());
        if record.command.trim().is_empty() && !command.trim().is_empty() {
            record.command = command;
        }
        Some(CommandSessionUpdate {
            view: record.view(),
            had_output_before: update.had_output_before,
        })
    }

    pub(crate) fn register_start(&mut self, payload: &Value) -> Option<CommandSessionUpdate> {
        let command_session_id = string_field(payload, "command_session_id")?;
        let tool_call_id = string_field(payload, "tool_call_id");
        let update = self
            .claim_pending_tool_call_record(tool_call_id.as_deref(), command_session_id.as_str())
            .unwrap_or_else(|| self.upsert_for_ref(command_session_id.as_str()));
        let record = self.records.get_mut(update.primary.as_str())?;
        record.status = CommandSessionDisplayStatus::Running;
        record.command_session_id = Some(command_session_id.clone());
        if let Some(tool_call_id) = tool_call_id {
            record.tool_call_id = Some(tool_call_id.clone());
            self.aliases
                .entry(tool_call_id)
                .or_insert_with(|| update.primary.clone());
        }
        self.aliases
            .insert(command_session_id.clone(), update.primary.clone());
        merge_text_field(&mut record.command, payload, "command");
        merge_optional_text_field(&mut record.cwd, payload, "cwd");
        Some(CommandSessionUpdate {
            view: record.view(),
            had_output_before: update.had_output_before,
        })
    }

    pub(crate) fn register_delta(&mut self, payload: &Value) -> Option<CommandSessionUpdate> {
        let command_session_id = string_field(payload, "command_session_id");
        let tool_call_id = string_field(payload, "tool_call_id");
        let primary_ref = command_session_id.as_deref().or(tool_call_id.as_deref())?;
        let update = self.upsert_for_ref(primary_ref);
        let record = self.records.get_mut(update.primary.as_str())?;
        if let Some(command_session_id) = command_session_id {
            record.command_session_id = Some(command_session_id.clone());
            self.aliases
                .insert(command_session_id, update.primary.clone());
        }
        if let Some(tool_call_id) = tool_call_id {
            record.tool_call_id = Some(tool_call_id.clone());
            self.aliases
                .entry(tool_call_id)
                .or_insert_with(|| update.primary.clone());
        }
        if record.status == CommandSessionDisplayStatus::Pending {
            record.status = CommandSessionDisplayStatus::Running;
        }
        merge_text_field(&mut record.command, payload, "command");
        merge_optional_text_field(&mut record.cwd, payload, "cwd");
        let delta = payload
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match payload
            .get("stream")
            .and_then(Value::as_str)
            .unwrap_or("stdout")
            .to_ascii_lowercase()
            .as_str()
        {
            "stderr" => record.stderr.append(delta),
            "pty" => record.pty.append(delta),
            _ => record.stdout.append(delta),
        }
        Some(CommandSessionUpdate {
            view: record.view(),
            had_output_before: update.had_output_before,
        })
    }

    pub(crate) fn register_status(&mut self, payload: &Value) -> Option<CommandSessionUpdate> {
        let command_session_id = string_field(payload, "command_session_id");
        let tool_call_id = string_field(payload, "tool_call_id");
        let primary_ref = command_session_id.as_deref().or(tool_call_id.as_deref())?;
        let update = self.upsert_for_ref(primary_ref);
        let record = self.records.get_mut(update.primary.as_str())?;
        if let Some(command_session_id) = command_session_id {
            record.command_session_id = Some(command_session_id.clone());
            self.aliases
                .insert(command_session_id, update.primary.clone());
        }
        if let Some(tool_call_id) = tool_call_id {
            record.tool_call_id = Some(tool_call_id.clone());
            self.aliases
                .entry(tool_call_id)
                .or_insert_with(|| update.primary.clone());
        }
        merge_text_field(&mut record.command, payload, "command");
        merge_optional_text_field(&mut record.cwd, payload, "cwd");
        apply_status_fields(record, payload);
        Some(CommandSessionUpdate {
            view: record.view(),
            had_output_before: update.had_output_before,
        })
    }

    pub(crate) fn register_tool_result_all(
        &mut self,
        payload: &Value,
    ) -> Vec<CommandSessionUpdate> {
        let result = payload.get("result").unwrap_or(payload);
        let data = result.get("data").unwrap_or(result);
        let items = data
            .get("results")
            .and_then(Value::as_array)
            .map(|items| items.iter().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![data]);
        let tool_call_id = string_field(payload, "tool_call_id");
        let mut updates = Vec::new();
        for item in items {
            if let Some(update) =
                self.register_tool_result_item(payload, result, item, tool_call_id.as_deref())
            {
                updates.push(update);
            }
        }
        updates
    }

    fn register_tool_result_item(
        &mut self,
        payload: &Value,
        result: &Value,
        item: &Value,
        tool_call_id: Option<&str>,
    ) -> Option<CommandSessionUpdate> {
        let command_session_id = string_field(item, "command_session_id")
            .or_else(|| string_field(payload, "command_session_id"));
        let fallback_tool_call_id = string_field(payload, "tool_call_id");
        let primary_ref = command_session_id
            .as_deref()
            .or(tool_call_id)
            .or(fallback_tool_call_id.as_deref())?;
        let update = self.upsert_for_ref(primary_ref);
        if let Some(command_session_id) = command_session_id.as_ref() {
            self.aliases
                .insert(command_session_id.clone(), update.primary.clone());
        }
        if let Some(tool_call_id) = tool_call_id {
            self.aliases
                .entry(tool_call_id.to_string())
                .or_insert_with(|| update.primary.clone());
        }
        let record = self.records.get_mut(update.primary.as_str())?;
        if let Some(tool_call_id) = tool_call_id {
            record.tool_call_id = Some(tool_call_id.to_string());
        }
        if let Some(command_session_id) = command_session_id {
            record.command_session_id = Some(command_session_id.clone());
            self.aliases
                .insert(command_session_id, update.primary.clone());
        }
        merge_text_field(&mut record.command, item, "command");
        if record.command.trim().is_empty() {
            merge_text_field(&mut record.command, payload, "command");
        }
        if let Some(returncode) = number_field(item, "returncode")
            .or_else(|| nested_number_field(result, &["meta", "exit_code"]))
        {
            record.exit_code = Some(returncode);
        }
        if let Some(duration_ms) = nested_number_field(result, &["meta", "duration_ms"]) {
            record.duration_ms = Some(duration_ms);
        }
        record.status = if result.get("ok").and_then(Value::as_bool) == Some(false)
            && record.exit_code.is_none()
        {
            CommandSessionDisplayStatus::FailedToStart
        } else {
            CommandSessionDisplayStatus::Exited
        };
        if let Some(error) = result
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            record.error = Some(error.to_string());
        }
        if let Some(stdout) = item.get("stdout").and_then(Value::as_str) {
            if !stdout.is_empty() || record.stdout.is_empty() {
                record.stdout.set_text(stdout);
            }
        }
        if let Some(stderr) = item.get("stderr").and_then(Value::as_str) {
            if !stderr.is_empty() || record.stderr.is_empty() {
                record.stderr.set_text(stderr);
            }
        }

        Some(CommandSessionUpdate {
            view: record.view(),
            had_output_before: update.had_output_before,
        })
    }

    pub(crate) fn primary_for_view(&self, view: &CommandSessionView) -> String {
        self.resolve_alias(view.primary_id.as_str())
            .unwrap_or_else(|| view.primary_id.clone())
    }

    fn upsert_for_ref(&mut self, reference: &str) -> UpsertInfo {
        let primary = self
            .resolve_alias(reference)
            .unwrap_or_else(|| reference.to_string());
        let had_output_before = self
            .records
            .get(primary.as_str())
            .is_some_and(CommandSessionRecord::has_output);
        self.records
            .entry(primary.clone())
            .or_insert_with(|| CommandSessionRecord {
                primary_id: primary.clone(),
                ..CommandSessionRecord::default()
            });
        self.aliases.insert(reference.to_string(), primary.clone());
        UpsertInfo {
            primary,
            had_output_before,
        }
    }

    fn resolve_alias(&self, reference: &str) -> Option<String> {
        let cleaned = reference.trim();
        if cleaned.is_empty() {
            return None;
        }
        self.aliases.get(cleaned).cloned().or_else(|| {
            self.records
                .contains_key(cleaned)
                .then(|| cleaned.to_string())
        })
    }

    fn claim_pending_tool_call_record(
        &mut self,
        tool_call_id: Option<&str>,
        command_session_id: &str,
    ) -> Option<UpsertInfo> {
        let tool_call_id = tool_call_id?.trim();
        if tool_call_id.is_empty()
            || tool_call_id == command_session_id
            || self.records.contains_key(command_session_id)
        {
            return None;
        }
        let Some(mut record) = self.records.remove(tool_call_id) else {
            return None;
        };
        let can_claim = record.command_session_id.is_none()
            && !record.has_output()
            && record.status == CommandSessionDisplayStatus::Pending;
        if !can_claim {
            self.records.insert(tool_call_id.to_string(), record);
            return None;
        }
        let had_output_before = record.has_output();
        record.primary_id = command_session_id.to_string();
        self.records.insert(command_session_id.to_string(), record);
        for primary in self.aliases.values_mut() {
            if primary == tool_call_id {
                *primary = command_session_id.to_string();
            }
        }
        self.aliases.insert(
            command_session_id.to_string(),
            command_session_id.to_string(),
        );
        Some(UpsertInfo {
            primary: command_session_id.to_string(),
            had_output_before,
        })
    }
}

struct UpsertInfo {
    primary: String,
    had_output_before: bool,
}

pub(crate) fn extract_command_input(value: &Value) -> Option<String> {
    if let Value::String(text) = value {
        let cleaned = text.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    let object = value.as_object()?;
    for key in ["content", "command", "cmd", "text"] {
        if let Some(text) = object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(text.to_string());
        }
    }
    None
}

pub(crate) fn compact_text_preview(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return (text.to_string(), false);
    }
    if max_chars <= 16 {
        return (text.chars().take(max_chars).collect(), true);
    }
    let head_chars = max_chars / 2;
    let tail_chars = max_chars.saturating_sub(head_chars);
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let omitted = char_count.saturating_sub(head_chars + tail_chars);
    (
        format!("{head}\n... omitted {omitted} chars ...\n{tail}"),
        true,
    )
}

fn apply_status_fields(record: &mut CommandSessionRecord, payload: &Value) {
    if let Some(status) = payload.get("status").and_then(Value::as_str) {
        record.status = match status {
            "failed_to_start" => CommandSessionDisplayStatus::FailedToStart,
            "exited" => CommandSessionDisplayStatus::Exited,
            "running" => CommandSessionDisplayStatus::Running,
            _ => record.status,
        };
    }
    if let Some(exit_code) = number_field(payload, "exit_code") {
        record.exit_code = Some(exit_code);
    }
    if let Some(duration_ms) = number_field(payload, "duration_ms") {
        record.duration_ms = Some(duration_ms);
    }
    if let Some(timed_out) = payload.get("timed_out").and_then(Value::as_bool) {
        record.timed_out = timed_out;
    }
    if let Some(error) = payload
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        record.error = Some(error.to_string());
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
}

fn merge_text_field(target: &mut String, value: &Value, key: &str) {
    if let Some(text) = string_field(value, key) {
        *target = text;
    }
}

fn merge_optional_text_field(target: &mut Option<String>, value: &Value, key: &str) {
    if let Some(text) = string_field(value, key) {
        *target = Some(text);
    }
}

fn number_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(value_as_i64)
}

fn nested_number_field(value: &Value, path: &[&str]) -> Option<i64> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    value_as_i64(current)
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|item| item.min(i64::MAX as u64) as i64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn start_links_tool_call_and_command_session_refs() {
        let mut state = CommandSessionDisplayState::default();
        let call = state
            .register_tool_call(&json!({
                "tool_call_id": "call_1",
                "args": { "content": "echo hi" }
            }))
            .expect("tool call");
        assert_eq!(call.view.primary_id, "call_1");

        let start = state
            .register_start(&json!({
                "tool_call_id": "call_1",
                "command_session_id": "cmd_1",
                "command": "echo hi"
            }))
            .expect("start");
        assert_eq!(start.view.primary_id, "cmd_1");

        let delta = state
            .register_delta(&json!({
                "command_session_id": "cmd_1",
                "stream": "stdout",
                "delta": "ok\n"
            }))
            .expect("delta");
        assert_eq!(delta.view.primary_id, "cmd_1");
        assert_eq!(delta.view.stdout, "ok\n");
    }

    #[test]
    fn buffers_keep_head_and_tail() {
        let mut state = CommandSessionDisplayState::default();
        state
            .register_start(&json!({
                "command_session_id": "cmd_1",
                "command": "demo"
            }))
            .expect("start");
        state
            .register_delta(&json!({
                "command_session_id": "cmd_1",
                "stream": "stdout",
                "delta": "a".repeat(BUFFER_HEAD_CHARS + BUFFER_TAIL_CHARS + 24)
            }))
            .expect("delta");
        let view = state
            .register_status(&json!({
                "command_session_id": "cmd_1",
                "status": "exited",
                "exit_code": 0
            }))
            .expect("status")
            .view;
        assert!(view.stdout.starts_with('a'));
        assert!(view.stdout.contains("omitted 24 chars"));
        assert!(view.stdout.ends_with('a'));
    }

    #[test]
    fn tool_result_replaces_live_output_without_losing_refs() {
        let mut state = CommandSessionDisplayState::default();
        state
            .register_start(&json!({
                "tool_call_id": "call_1",
                "command_session_id": "cmd_1",
                "command": "demo"
            }))
            .expect("start");
        state
            .register_delta(&json!({
                "command_session_id": "cmd_1",
                "stream": "stdout",
                "delta": "live"
            }))
            .expect("delta");
        let mut updates = state.register_tool_result_all(&json!({
            "tool": "execute_command",
            "tool_call_id": "call_1",
            "result": {
                "ok": true,
                "data": {
                    "results": [{
                        "command": "demo",
                        "command_session_id": "cmd_1",
                        "returncode": 0,
                        "stdout": "final",
                        "stderr": ""
                    }]
                },
                "meta": { "duration_ms": 12 }
            }
        }));
        assert_eq!(updates.len(), 1);
        let update = updates.remove(0);
        assert!(update.had_output_before);
        assert_eq!(update.view.stdout, "final");
        assert_eq!(update.view.exit_code, Some(0));
        assert_eq!(update.view.duration_ms, Some(12));
        assert!(update.view.ref_ids().contains(&"cmd_1"));
    }

    #[test]
    fn shared_tool_call_id_keeps_distinct_command_sessions() {
        let mut state = CommandSessionDisplayState::default();
        state
            .register_tool_call(&json!({
                "tool_call_id": "call_1",
                "args": { "content": "first\nsecond" }
            }))
            .expect("tool call");
        let first = state
            .register_start(&json!({
                "tool_call_id": "call_1",
                "command_session_id": "cmd_1",
                "command": "first"
            }))
            .expect("first");
        let second = state
            .register_start(&json!({
                "tool_call_id": "call_1",
                "command_session_id": "cmd_2",
                "command": "second"
            }))
            .expect("second");

        assert_eq!(first.view.primary_id, "cmd_1");
        assert_eq!(second.view.primary_id, "cmd_2");

        let updates = state.register_tool_result_all(&json!({
            "tool": "execute_command",
            "tool_call_id": "call_1",
            "result": {
                "ok": true,
                "data": {
                    "results": [
                        {
                            "command": "first",
                            "command_session_id": "cmd_1",
                            "returncode": 0,
                            "stdout": "one",
                            "stderr": ""
                        },
                        {
                            "command": "second",
                            "command_session_id": "cmd_2",
                            "returncode": 0,
                            "stdout": "two",
                            "stderr": ""
                        }
                    ]
                }
            }
        }));

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].view.primary_id, "cmd_1");
        assert_eq!(updates[0].view.stdout, "one");
        assert_eq!(updates[1].view.primary_id, "cmd_2");
        assert_eq!(updates[1].view.stdout, "two");
    }
}
