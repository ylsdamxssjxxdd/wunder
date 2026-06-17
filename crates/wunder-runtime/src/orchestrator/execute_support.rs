use super::tool_calls::ToolCall;
use super::*;
use crate::core::approval::{ApprovalRequestKind, ApprovalResponse};
use crate::core::llm_speed::TurnDecodeSpeedAccumulator;
use crate::services::tools::tool_error::{with_error_meta, ToolErrorMeta};

pub(super) struct PlannedToolCall {
    pub(super) call: ToolCall,
    pub(super) name: String,
    pub(super) function_name: String,
}

pub(super) struct ToolPlanningResult {
    pub(super) planned: Vec<PlannedToolCall>,
    pub(super) rejected: Vec<RejectedToolCall>,
}

pub(super) struct RejectedToolCall {
    pub(super) name: String,
    pub(super) resolved_name: String,
    pub(super) reason: &'static str,
    pub(super) arguments_preview: String,
}

pub(super) struct ToolExecutionOutcome {
    pub(super) call: ToolCall,
    pub(super) name: String,
    pub(super) result: ToolResultPayload,
}

#[derive(Default, Clone, Copy)]
pub(super) struct ToolBudgetUsage {
    pub(super) total: u32,
    pub(super) db_query: u32,
    pub(super) memory_recall: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ToolBudgetLimits {
    pub(super) total: u32,
    pub(super) db_query: u32,
    pub(super) memory_recall: u32,
}

#[derive(Clone)]
pub(super) struct CachedToolResult {
    pub(super) ok: bool,
    pub(super) data: Value,
    pub(super) error: String,
    pub(super) sandbox: bool,
    pub(super) meta: Option<Value>,
}

impl CachedToolResult {
    pub(super) fn from_payload(result: &ToolResultPayload) -> Self {
        Self {
            ok: result.ok,
            data: result.data.clone(),
            error: result.error.clone(),
            sandbox: result.sandbox,
            meta: result.meta.clone(),
        }
    }

    pub(super) fn to_payload(&self) -> ToolResultPayload {
        ToolResultPayload {
            ok: self.ok,
            data: self.data.clone(),
            error: self.error.clone(),
            sandbox: self.sandbox,
            timestamp: Utc::now(),
            meta: self.meta.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct CachedRecallResult {
    pub(super) revision: u64,
    pub(super) result: CachedToolResult,
}

#[derive(Clone, Copy)]
pub(super) enum ToolBudgetBlockKind {
    Total,
    DbQuery,
    MemoryRecall,
}

impl ToolBudgetBlockKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            ToolBudgetBlockKind::Total => "total",
            ToolBudgetBlockKind::DbQuery => "db_query",
            ToolBudgetBlockKind::MemoryRecall => "memory_recall",
        }
    }
}

#[derive(Clone)]
pub(super) struct ToolBudgetBlock {
    pub(super) kind: ToolBudgetBlockKind,
    pub(super) limit: u32,
    pub(super) attempted: u32,
    pub(super) tool: String,
}

pub(super) enum TerminalTool {
    A2ui,
    Final,
}

#[derive(Clone)]
pub(super) struct AssistantHistorySnapshot {
    pub(super) tool_calls: Option<Value>,
    pub(super) persisted_tool_calls: Option<Value>,
}

const DEFAULT_NON_ADMIN_MAX_ROUNDS: u32 = 1000;
const MIN_NON_ADMIN_MAX_ROUNDS: u32 = 2;
const MIN_NON_ADMIN_MAX_ROUNDS_WITH_TOOLS: u32 = MIN_NON_ADMIN_MAX_ROUNDS;
pub(super) const MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS: u32 = 8;
const DEFAULT_REPEATED_TOOL_FAILURE_THRESHOLD: u32 = 5;
pub(super) const DEFAULT_TOOL_CALL_BUDGET_PER_TURN: u32 = 10_000;
const DEFAULT_DB_QUERY_TOOL_BUDGET_PER_TURN: u32 = 2_000;
const EXTENDED_DB_QUERY_TOOL_BUDGET_PER_TURN: u32 = 10_000;
pub(super) const DEFAULT_MEMORY_RECALL_BUDGET_PER_TURN: u32 = 2_000;
const TOOL_FAILURE_SIGNATURE_MAX_CHARS: usize = 240;
pub(super) const INVALID_TOOL_CALL_REROUTE_MAX_PER_TURN: u32 = 2;
const INVALID_TOOL_CALL_ARGUMENT_PREVIEW_CHARS: usize = 320;
pub(super) const EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN: u32 = 3;
const WORKSPACE_UPDATE_MAX_CHANGED_PATHS: usize = 24;
const CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY: &str = "_channel_display_question";
const WORKSPACE_PATH_HINT_KEYS: [&str; 26] = [
    "path",
    "paths",
    "changed_paths",
    "changedPaths",
    "public_path",
    "publicPath",
    "workspace_relative_path",
    "workspaceRelativePath",
    "target_path",
    "targetPath",
    "source_path",
    "sourcePath",
    "destination",
    "destination_path",
    "destinationPath",
    "output_path",
    "outputPath",
    "saved_path",
    "savedPath",
    "file_path",
    "filePath",
    "relative_path",
    "relativePath",
    "file",
    "files",
    "to_path",
];
const WORKSPACE_EVENT_NESTED_OBJECT_KEYS: [&str; 5] =
    ["data", "meta", "result", "output", "payload"];
const CANCELLED_GENERATION_CONTEXT_MARKER: &str =
    "Previous run cancelled by user before an assistant response was produced.";

pub(super) fn should_enable_local_full_event_logs(server_mode: &str) -> bool {
    matches!(
        server_mode.trim().to_ascii_lowercase().as_str(),
        "desktop" | "cli"
    )
}

pub(super) fn build_planned_tool_calls(
    calls: Vec<ToolCall>,
    allowed_tool_names: &HashSet<String>,
) -> ToolPlanningResult {
    let mut planned = Vec::new();
    let mut rejected = Vec::new();
    for mut call in calls {
        let name = call.name.trim();
        if name.is_empty() {
            rejected.push(RejectedToolCall {
                name: String::new(),
                resolved_name: String::new(),
                reason: "empty_tool_name",
                arguments_preview: tool_call_arguments_preview(&call.arguments),
            });
            continue;
        }
        let function_name = call
            .function_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(name)
            .to_string();
        let resolved = resolve_tool_name(name);
        if resolved.trim().is_empty() {
            rejected.push(RejectedToolCall {
                name: name.to_string(),
                resolved_name: resolved,
                reason: "empty_resolved_tool_name",
                arguments_preview: tool_call_arguments_preview(&call.arguments),
            });
            continue;
        }
        if !allowed_tool_names.contains(&resolved) && !allowed_tool_names.contains(name) {
            rejected.push(RejectedToolCall {
                name: name.to_string(),
                resolved_name: resolved,
                reason: "tool_not_allowed_or_unknown",
                arguments_preview: tool_call_arguments_preview(&call.arguments),
            });
            continue;
        }
        call.name = resolved.clone();
        planned.push(PlannedToolCall {
            call,
            name: resolved,
            function_name,
        });
    }
    ToolPlanningResult { planned, rejected }
}

fn tool_call_arguments_preview(arguments: &Value) -> String {
    let text = serde_json::to_string(arguments).unwrap_or_else(|_| String::new());
    trim_text_to_chars(&text, INVALID_TOOL_CALL_ARGUMENT_PREVIEW_CHARS, "...")
}

pub(super) fn build_invalid_tool_call_model_notice(
    rejected: &[RejectedToolCall],
    allowed_tool_names: &HashSet<String>,
) -> Value {
    let mut allowed = allowed_tool_names.iter().cloned().collect::<Vec<_>>();
    allowed.sort();
    allowed.truncate(64);
    json!({
        "type": "invalid_tool_call_notice",
        "ok": false,
        "reason": "model_emitted_unexecutable_tool_calls",
        "rejected_tool_calls": rejected_tool_calls_event_payload(rejected),
        "allowed_tool_names_sample": allowed,
        "instruction": "The previous assistant message contained tool calls that this runtime cannot execute. Do not repeat the same invalid tool name or malformed argument wrapper. Emit one valid allowed tool call with valid JSON arguments, or call final_response with a concise final answer if no tool is needed.",
    })
}

pub(super) fn rejected_tool_calls_event_payload(rejected: &[RejectedToolCall]) -> Value {
    Value::Array(
        rejected
            .iter()
            .take(8)
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "resolved_name": entry.resolved_name,
                    "reason": entry.reason,
                    "arguments_preview": entry.arguments_preview,
                })
            })
            .collect(),
    )
}

pub(super) fn append_terminal_tool_context_result(
    orchestrator: &Orchestrator,
    user_id: &str,
    session_id: &str,
    call: &ToolCall,
    tool_name: &str,
) {
    let payload = json!({
        "tool": tool_name,
        "ok": true,
        "data": {
            "terminal": true,
        },
    });
    let serialized = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let use_native_tool_result = call.id.as_deref().is_some_and(|id| {
        let cleaned = id.trim();
        !cleaned.is_empty() && !cleaned.starts_with("call_terminal_")
    });
    if use_native_tool_result {
        let tool_call_id = call.id.as_deref().unwrap().trim();
        orchestrator.append_model_context_entry(
            user_id,
            session_id,
            &json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": serialized,
            }),
        );
    } else {
        orchestrator.append_model_context_entry(
            user_id,
            session_id,
            &json!({
                "role": "user",
                "content": format!("{OBSERVATION_PREFIX}{serialized}"),
            }),
        );
    }
}

pub(super) fn build_assistant_history_snapshot(
    tool_calls_payload: Option<&Value>,
    allowed_tool_names: &HashSet<String>,
) -> AssistantHistorySnapshot {
    let Some(payload) = tool_calls_payload.cloned() else {
        return AssistantHistorySnapshot {
            tool_calls: None,
            persisted_tool_calls: None,
        };
    };
    let persisted = extract_replayable_tool_call_payloads(&payload, allowed_tool_names);
    let persisted_tool_calls = (!persisted.is_empty()).then_some(Value::Array(persisted));
    AssistantHistorySnapshot {
        tool_calls: persisted_tool_calls.clone(),
        persisted_tool_calls,
    }
}

pub(super) fn append_cancelled_generation_context_marker(
    orchestrator: &Orchestrator,
    user_id: &str,
    session_id: &str,
    messages: &[Value],
) {
    if !should_append_cancelled_generation_context_marker(messages) {
        return;
    }
    let marker_message = json!({
        "role": "assistant",
        "content": CANCELLED_GENERATION_CONTEXT_MARKER,
    });
    orchestrator.append_model_context_entry(user_id, session_id, &marker_message);
    orchestrator.append_internal_model_context_chat(
        user_id,
        session_id,
        &marker_message,
        "cancelled_generation_marker",
    );
}

fn should_append_cancelled_generation_context_marker(messages: &[Value]) -> bool {
    matches!(
        messages
            .last()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str),
        Some("user" | "tool")
    )
}

pub(super) fn build_model_context_tool_calls_snapshot(
    tool_calls_payload: Option<&Value>,
    allowed_tool_names: &HashSet<String>,
) -> Option<Value> {
    let payload = tool_calls_payload?;
    let persisted = extract_model_context_tool_call_payloads(payload, allowed_tool_names);
    (!persisted.is_empty()).then_some(Value::Array(persisted))
}

fn extract_model_context_tool_call_payloads(
    payload: &Value,
    allowed_tool_names: &HashSet<String>,
) -> Vec<Value> {
    let payload_items = match payload {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![payload.clone()],
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .map(|parsed| extract_model_context_tool_call_payloads(&parsed, allowed_tool_names))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    payload_items
        .into_iter()
        .filter(|item| should_keep_model_context_tool_call_payload(item, allowed_tool_names))
        .collect()
}

fn should_keep_model_context_tool_call_payload(
    payload: &Value,
    allowed_tool_names: &HashSet<String>,
) -> bool {
    let Some(name) = replay_tool_call_name(payload) else {
        return false;
    };
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let resolved = resolve_tool_name(name);
    !resolved.trim().is_empty()
        && (allowed_tool_names.contains(&resolved) || allowed_tool_names.contains(name))
}

fn extract_replayable_tool_call_payloads(
    payload: &Value,
    allowed_tool_names: &HashSet<String>,
) -> Vec<Value> {
    let payload_items = match payload {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![payload.clone()],
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .map(|parsed| extract_replayable_tool_call_payloads(&parsed, allowed_tool_names))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    payload_items
        .into_iter()
        .filter(|item| should_replay_tool_call_payload(item, allowed_tool_names))
        .collect()
}

fn should_replay_tool_call_payload(payload: &Value, allowed_tool_names: &HashSet<String>) -> bool {
    let Some(name) = replay_tool_call_name(payload) else {
        return false;
    };
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let resolved = resolve_tool_name(name);
    !resolved.trim().is_empty()
        && !is_terminal_tool_name(resolved.as_str())
        && (allowed_tool_names.contains(&resolved) || allowed_tool_names.contains(name))
}

fn replay_tool_call_name(payload: &Value) -> Option<&str> {
    let map = payload.as_object()?;
    map.get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| map.get("name").and_then(Value::as_str))
}

fn is_terminal_tool_name(tool_name: &str) -> bool {
    let canonical = resolve_tool_name(tool_name.trim());
    canonical == resolve_tool_name("final_response") || canonical == "a2ui"
}

fn tool_call_mode_key(mode: ToolCallMode) -> &'static str {
    match mode {
        ToolCallMode::ToolCall => "tool_call",
        ToolCallMode::FunctionCall => "function_call",
        ToolCallMode::FreeformCall => "freeform_call",
    }
}

impl Orchestrator {
    pub(super) fn resolve_frozen_session_tool_call_mode(
        &self,
        user_id: &str,
        session_id: &str,
        llm_config: &LlmModelConfig,
    ) -> ToolCallMode {
        if let Some(stored) = self
            .workspace
            .load_session_frozen_tool_call_mode(user_id, session_id)
        {
            return crate::llm::normalize_tool_call_mode(Some(stored.as_str()));
        }
        let mode = self
            .workspace
            .load_session_system_prompt(user_id, session_id, None)
            .ok()
            .flatten()
            .and_then(|prompt| infer_tool_call_mode_from_frozen_system_prompt(&prompt))
            .unwrap_or_else(|| crate::llm::resolve_tool_call_mode(llm_config));
        self.workspace.save_session_frozen_tool_call_mode(
            user_id,
            session_id,
            tool_call_mode_key(mode),
        );
        mode
    }
}

fn infer_tool_call_mode_from_frozen_system_prompt(prompt: &str) -> Option<ToolCallMode> {
    let cleaned = prompt.trim();
    if cleaned.is_empty() {
        return None;
    }
    let lowered = cleaned.to_ascii_lowercase();
    let has_prompt_tool_protocol = cleaned.contains("[工具协议]")
        || lowered.contains("[tools protocol]")
        || (lowered.contains("<tools>") && lowered.contains("<tool_call>"));
    if !has_prompt_tool_protocol {
        return Some(ToolCallMode::FunctionCall);
    }
    if lowered.contains("freeform") || cleaned.contains("apply_patch 专用规则") {
        return Some(ToolCallMode::FreeformCall);
    }
    Some(ToolCallMode::ToolCall)
}

pub(super) fn uses_native_tool_api(
    tool_call_mode: ToolCallMode,
    llm_config: &LlmModelConfig,
) -> bool {
    match tool_call_mode {
        ToolCallMode::FunctionCall => true,
        ToolCallMode::FreeformCall => matches!(
            crate::llm::resolve_openai_api_mode(llm_config),
            crate::llm::OpenAiApiMode::Responses
        ),
        ToolCallMode::ToolCall => false,
    }
}

pub(super) fn resolve_tool_parallelism(total: usize) -> usize {
    let desired = DEFAULT_TOOL_PARALLELISM.max(1);
    total.max(1).min(desired)
}

pub(super) fn resolve_non_admin_max_rounds(
    llm_config: &LlmModelConfig,
    skip_tool_calls: bool,
) -> i64 {
    let configured = llm_config
        .max_rounds
        .unwrap_or(DEFAULT_NON_ADMIN_MAX_ROUNDS);
    let minimum = if skip_tool_calls {
        MIN_NON_ADMIN_MAX_ROUNDS
    } else {
        MIN_NON_ADMIN_MAX_ROUNDS_WITH_TOOLS
    };
    i64::from(configured.max(minimum))
}

pub(super) fn resolve_db_query_tool_budget(question: &str) -> u32 {
    if should_allow_extended_db_query_budget(question) {
        EXTENDED_DB_QUERY_TOOL_BUDGET_PER_TURN
    } else {
        DEFAULT_DB_QUERY_TOOL_BUDGET_PER_TURN
    }
}

fn should_allow_extended_db_query_budget(question: &str) -> bool {
    let text = question.trim().to_lowercase();
    if text.is_empty() {
        return false;
    }
    let keywords = [
        "全量",
        "全部",
        "所有记录",
        "完整数据",
        "全表",
        "所有行",
        "导出全部",
        "all rows",
        "all records",
        "full dataset",
        "entire dataset",
        "full export",
        "paginate all",
    ];
    keywords.iter().any(|keyword| text.contains(keyword))
}

pub(super) fn is_db_query_tool_name(tool_name: &str) -> bool {
    let cleaned = tool_name.trim().to_lowercase();
    cleaned == "db_query"
        || cleaned.starts_with("db_query_")
        || cleaned.ends_with("@db_query")
        || cleaned.contains("@db_query_")
}

pub(super) fn is_memory_manager_tool_name(tool_name: &str, memory_manager_tool_name: &str) -> bool {
    let cleaned = tool_name.trim();
    cleaned == memory_manager_tool_name || cleaned.eq_ignore_ascii_case("memory_manager")
}

pub(super) fn extract_memory_manager_action(args: &Value) -> Option<String> {
    let normalized = crate::core::tool_args::recover_tool_args_value(args);
    normalized
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

pub(super) fn extract_memory_manager_query(args: &Value) -> Option<String> {
    let normalized = crate::core::tool_args::recover_tool_args_value(args);
    normalized
        .get("query")
        .or_else(|| normalized.get("content"))
        .or_else(|| normalized.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn is_memory_recall_action(action: &str) -> bool {
    matches!(
        action.trim().to_lowercase().as_str(),
        "recall" | "search" | "query" | "retrieve"
    )
}

pub(super) fn is_memory_write_action(action: &str) -> bool {
    matches!(
        action.trim().to_lowercase().as_str(),
        "add" | "create" | "append" | "update" | "upsert" | "delete" | "remove" | "clear" | "reset"
    )
}

pub(super) fn normalize_memory_recall_query(query: Option<&str>) -> Option<String> {
    let query = query.unwrap_or("").trim();
    if query.is_empty() {
        return None;
    }
    Some(query.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub(super) fn is_memory_recall_tool_call(
    tool_name: &str,
    args: &Value,
    memory_manager_tool_name: &str,
) -> bool {
    if !is_memory_manager_tool_name(tool_name, memory_manager_tool_name) {
        return false;
    }
    extract_memory_manager_action(args)
        .as_deref()
        .is_some_and(is_memory_recall_action)
}

pub(super) fn resolve_cached_memory_recall_result(
    planned: &PlannedToolCall,
    memory_manager_tool_name: &str,
    recall_cache: &HashMap<String, CachedRecallResult>,
    revision: u64,
) -> Option<CachedToolResult> {
    if !is_memory_recall_tool_call(
        &planned.name,
        &planned.call.arguments,
        memory_manager_tool_name,
    ) {
        return None;
    }
    let query_key = normalize_memory_recall_query(
        extract_memory_manager_query(&planned.call.arguments).as_deref(),
    )?;
    let cached = recall_cache.get(&query_key)?;
    if cached.revision != revision {
        return None;
    }
    Some(cached.result.clone())
}

pub(super) fn should_recover_from_context_overflow(err: &OrchestratorError) -> bool {
    err.code() == "CONTEXT_WINDOW_EXCEEDED"
        || super::llm::is_context_window_error_text(err.message())
}

pub(super) fn merge_context_window_limit_hint(
    current: Option<i64>,
    next: Option<i64>,
) -> Option<i64> {
    let current = current.filter(|value| *value > 0);
    let next = next.filter(|value| *value > 0);
    match (current, next) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(super) fn apply_context_window_limit_hint(
    llm_config: &LlmModelConfig,
    limit_hint: Option<i64>,
) -> LlmModelConfig {
    let Some(limit_hint) = limit_hint.filter(|value| *value > 0) else {
        return llm_config.clone();
    };
    let Ok(limit_hint_u32) = u32::try_from(limit_hint) else {
        return llm_config.clone();
    };
    let mut config = llm_config.clone();
    config.max_context = Some(
        config
            .max_context
            .map_or(limit_hint_u32, |current| current.min(limit_hint_u32)),
    );
    config
}

pub(super) fn derive_recovery_context_window_limit_hint(
    projected_request_tokens: i64,
    attempt: u32,
) -> i64 {
    let mut hint = projected_request_tokens.max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    let rounds = attempt.clamp(1, 8);
    for _ in 0..rounds {
        hint = (hint / 2).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    }
    hint
}

pub(super) fn extract_channel_display_question_override(
    config_overrides: Option<&Value>,
) -> Option<String> {
    let config_overrides = config_overrides?;
    config_overrides
        .get(CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn resolve_user_content_for_persist(
    messages: &[Value],
    fallback_user_message: &Value,
) -> Option<Value> {
    if let Some(index) = Orchestrator::locate_current_user_index(messages) {
        if let Some(content) = messages
            .get(index)
            .and_then(|message| message.get("content"))
        {
            return Some(content.clone());
        }
    }
    fallback_user_message.get("content").cloned()
}

pub(super) fn build_max_rounds_user_guidance(max_rounds: Option<i64>) -> String {
    let mut params = HashMap::new();
    params.insert(
        "max_rounds".to_string(),
        max_rounds.unwrap_or_default().max(0).to_string(),
    );
    i18n::t_with_params("error.max_rounds_user_guidance", &params)
}

pub(super) fn resolve_tool_failure_guard_threshold(config: &Config) -> u32 {
    let threshold = u32::try_from(config.server.tool_failure_guard_threshold)
        .unwrap_or(DEFAULT_REPEATED_TOOL_FAILURE_THRESHOLD);
    threshold.max(1)
}

pub(super) fn build_empty_final_answer_model_notice(
    attempt: u32,
    max_attempts: u32,
    had_content: bool,
    had_reasoning: bool,
    had_tool_payload: bool,
    allow_tool_calls: bool,
) -> Value {
    let instruction = if allow_tool_calls {
        "Continue the task now. Do not end this turn with an empty assistant message. Either emit one valid allowed tool call with valid JSON arguments, or call final_response with a concise final answer."
    } else {
        "Continue the task now. Do not end this turn with an empty assistant message. Respond directly with a concise final answer."
    };
    json!({
        "type": "empty_final_answer_notice",
        "ok": false,
        "reason": "model_returned_no_final_content",
        "attempt": attempt,
        "max_attempts": max_attempts,
        "had_content": had_content,
        "had_reasoning": had_reasoning,
        "had_tool_payload": had_tool_payload,
        "instruction": instruction,
    })
}

pub(super) fn build_empty_final_answer_retry_exhausted_error(max_attempts: u32) -> String {
    format!("LLM unavailable after {max_attempts} automatic recovery attempts.")
}

pub(super) fn build_tool_failure_signature(tool_name: &str, result: &ToolResultPayload) -> String {
    let detail = if !result.error.trim().is_empty() {
        result.error.trim().to_string()
    } else {
        serde_json::to_string(&result.data).unwrap_or_default()
    };
    let normalized = detail
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let clipped = normalized
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    format!("{tool_name}|{clipped}")
}

pub(super) fn build_tool_failure_guard_answer(
    tool_name: &str,
    result: &ToolResultPayload,
    repeat_count: u32,
    threshold: u32,
) -> String {
    let mut params = HashMap::new();
    params.insert("tool_name".to_string(), tool_name.to_string());
    params.insert("repeat_count".to_string(), repeat_count.to_string());
    params.insert("threshold".to_string(), threshold.to_string());
    let detail = result.error.trim();
    if detail.is_empty() {
        return i18n::t_with_params("error.tool_failure_guard_user_guidance", &params);
    }
    let clipped = detail
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    params.insert("detail".to_string(), clipped);
    i18n::t_with_params("error.tool_failure_guard_user_guidance_with_error", &params)
}

pub(super) fn should_request_tool_failure_reroute(
    reason: &str,
    reroute_notice_count: u32,
    fingerprint: &str,
    rerouted_fingerprints: &HashSet<String>,
) -> bool {
    if rerouted_fingerprints.contains(fingerprint.trim()) {
        return false;
    }
    match reason {
        // Non-retryable deterministic failures still get reroute guidance.
        // Cap globally but avoid repeating the same fingerprint notice.
        "same_non_retryable_failure" => reroute_notice_count < 2,
        "tool_failure_reroute_required" | "same_retryable_failure_exhausted" => {
            reroute_notice_count < 2
        }
        _ => false,
    }
}

pub(super) fn encode_observation_prefixed_json(payload: &Value) -> String {
    let serialized = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    format!("{OBSERVATION_PREFIX}{serialized}")
}

pub(super) fn build_tool_failure_reroute_model_notice(
    tool_name: &str,
    stop: &super::retry_governor::RetryStopDecision,
    repeat_count: u32,
    threshold: u32,
    detail: &str,
) -> Value {
    let next_step = build_tool_failure_next_step_hint(tool_name, &stop.error_code, detail);
    let detail_head = detail
        .trim()
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    json!({
        "type": "tool_failure_reroute_notice",
        "ok": false,
        "tool": tool_name,
        "reason": stop.reason,
        "error_code": stop.error_code,
        "retryable": stop.retryable,
        "repeat_count": repeat_count,
        "threshold": threshold,
        "same_tool_failures": stop.same_tool_failures,
        "fingerprint": stop.fingerprint,
        "error": if detail_head.is_empty() {
            Value::Null
        } else {
            Value::String(detail_head)
        },
        "next_step_hint": if next_step.trim().is_empty() {
            Value::Null
        } else {
            Value::String(next_step)
        },
        "instruction": if tool_name == resolve_tool_name("apply_patch") {
            Value::String("Do not emit another broad apply_patch attempt. Read the latest file or exact excerpt, keep the next patch to a small batch, and after @@ write each line explicitly as space-context / -old / +new instead of pasting raw target lines. If unsure, use dry_run first. Never leave both old and new versions as plain context lines. If that still fails, switch to write_file for a full rewrite.".to_string())
        } else {
            Value::String("Do not repeat the same failing call pattern. Re-plan using current observations and switch execution strategy.".to_string())
        },
    })
}

pub(super) fn build_tool_failure_next_step_hint(
    tool_name: &str,
    error_code: &str,
    detail: &str,
) -> String {
    let code = error_code.trim().to_ascii_uppercase();
    let lower_detail = detail.trim().to_ascii_lowercase();
    let execute_command = resolve_tool_name("execute_command");
    let apply_patch = resolve_tool_name("apply_patch");
    if code.starts_with("PRECHECK_SHELL_")
        || code == "COMMAND_NOT_FOUND"
        || (tool_name == execute_command && lower_detail.contains("syntax error"))
    {
        return "建议下一步：改用 `write_file` 写入脚本文件后再执行，避免 heredoc/printf 拼接多行脚本。".to_string();
    }
    if code.starts_with("PRECHECK_PYTHON_") || code == "PYTHON_SYNTAX_ERROR" {
        return "建议下一步：先修复 Python 缩进/括号语法，再执行；优先一次写入完整脚本并直接运行文件。".to_string();
    }
    if code.starts_with("PRECHECK_SQL_")
        || code == "SQL_SYNTAX_ERROR"
        || code == "SQL_FUNCTION_NOT_FOUND"
        || code == "SQL_UNKNOWN_COLUMN"
    {
        return "建议下一步：改用 ASCII SQL 标点并简化查询（先 `SELECT ... LIMIT` 验证字段，再做聚合/导出）。".to_string();
    }
    if code == "TOOL_ARGS_INVALID" || code == "TOOL_ARGS_MISSING_FIELD" {
        return "建议下一步：严格按工具要求重构参数；优先直接复制错误结果里的 example，再替换成当前目标和值。".to_string();
    }
    if code == "TOOL_TIMEOUT" {
        return "建议下一步：缩小查询范围或改用可分页/导出路径，避免单次超时。".to_string();
    }
    if tool_name == apply_patch {
        if code == "PATCH_FORMAT_INVALID"
            && (lower_detail.contains("end patch") || detail.contains("缺少 *** End Patch"))
        {
            return "建议下一步：这次补丁没有闭合。重新生成完整 apply_patch 输入，第一行必须是 `*** Begin Patch`，最后一行必须是 `*** End Patch`；删除末尾多余的悬空 `@@`，不要在 `*** End Patch` 后追加任何内容。".to_string();
        }
        if matches!(
            code.as_str(),
            "PATCH_FORMAT_INVALID"
                | "PATCH_CONTEXT_NOT_FOUND"
                | "PATCH_CONTEXT_AMBIGUOUS"
                | "PATCH_NO_EFFECT"
        ) {
            return "建议下一步：先重新读取目标文件或相关片段，只保留当前文件中的原始上下文；@@ 之后不要直接粘贴目标文件内容，必须把每一行明确写成“空格上下文 / -旧行 / +新行”，其中空白上下文行也要写成单个空格行；不要把旧行和新行都写成普通上下文行。下一次补丁请控制在少量文件、少量区域内，每处前后保留 2-3 行空格开头的上下文；如果你对匹配是否稳定没有把握，先加 `dry_run` 预演；不要复制 >>> 路径、行号或 --- 分隔线。若仍然接近整文件修改，请直接改用 `write_file`。".to_string();
        }
        return "建议下一步：不要继续盲目重复 apply_patch。先 `read_file` 读取最新文件或精确片段，再把修改拆成少量文件、少量区域逐次提交；如果不确定补丁是否能匹配，先用 `dry_run`；如果改动跨很多区域、很多文件，或接近整文件改写，请改用 `write_file`。".to_string();
    }
    "建议下一步：停止重复当前调用，调整工具参数或更换工具路径后继续。".to_string()
}

pub(super) fn build_tool_timeout_result(
    tool_name: &str,
    timeout: Option<Duration>,
) -> ToolResultPayload {
    let timeout_s = timeout.map(|value| value.as_secs_f64());
    let timeout_ms = timeout.map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64);
    let failure_summary = if let Some(seconds) = timeout_s {
        format!("Tool `{tool_name}` timed out after {seconds:.1}s.")
    } else {
        format!("Tool `{tool_name}` timed out before it returned a result.")
    };
    let next_step_hint =
        build_tool_failure_next_step_hint(tool_name, "TOOL_TIMEOUT", &failure_summary);
    let message = i18n::t_with_params(
        "error.tool_execution_failed",
        &HashMap::from([("name".to_string(), format!("{tool_name} timeout"))]),
    );
    let data = with_error_meta(
        json!({
            "tool": tool_name,
            "phase": "execution",
            "failure_summary": failure_summary,
            "error_detail_head": failure_summary,
            "next_step_hint": next_step_hint,
            "timeout_s": timeout_s,
            "timeout_ms": timeout_ms,
        }),
        ToolErrorMeta::new(
            "TOOL_TIMEOUT",
            Some("Retry with a narrower scope or a resumable flow.".to_string()),
            true,
            timeout_ms,
        ),
    );
    ToolResultPayload::error(message, data)
}

pub(super) fn build_tool_budget_guard_model_notice(
    block: &ToolBudgetBlock,
    limits: &ToolBudgetLimits,
    usage: &ToolBudgetUsage,
) -> String {
    let scope = match block.kind {
        ToolBudgetBlockKind::Total => "total tool calls",
        ToolBudgetBlockKind::DbQuery => "db_query calls",
        ToolBudgetBlockKind::MemoryRecall => "memory recall calls",
    };
    let limit = match block.kind {
        ToolBudgetBlockKind::Total => limits.total,
        ToolBudgetBlockKind::DbQuery => limits.db_query,
        ToolBudgetBlockKind::MemoryRecall => limits.memory_recall,
    };
    let next_step = match block.kind {
        ToolBudgetBlockKind::Total => {
            "Stop repeating identical tool calls in this turn. Re-plan and continue from existing observations."
        }
        ToolBudgetBlockKind::DbQuery => {
            "Do not continue blind pagination. Prefer aggregation, narrower filters, or resumable/export flow."
        }
        ToolBudgetBlockKind::MemoryRecall => {
            "Do not repeatedly recall the same memory query. Consolidate findings and continue reasoning."
        }
    };
    format!(
        "Runtime notice: soft guard reached for {scope}. Attempted {attempted} > limit {limit} (blocked tool: {tool}). Current usage: total={total}/{total_limit}, db_query={db}/{db_limit}, memory_recall={recall}/{recall_limit}. {next_step} Keep working and complete the task for the user.",
        attempted = block.attempted,
        tool = block.tool,
        total = usage.total,
        total_limit = limits.total,
        db = usage.db_query,
        db_limit = limits.db_query,
        recall = usage.memory_recall,
        recall_limit = limits.memory_recall,
    )
}

// Accumulate token usage across model rounds within a user round.
// Each llm_output reports its own usage; the round_usage total must be the sum.
pub(super) fn update_round_usage_authority(target: &mut TokenUsage, usage: &TokenUsage) {
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.total = target.total.saturating_add(usage.total);
}

pub(super) fn resolve_usage_context_occupancy_tokens(usage: &TokenUsage) -> Option<i64> {
    let total = usage.total.max(usage.input.saturating_add(usage.output));
    if total == 0 || total > i64::MAX as u64 {
        return None;
    }
    Some(total as i64)
}

pub(super) fn resolve_round_context_occupancy_tokens(
    confirmed_context_occupancy_tokens: Option<i64>,
    persisted_context_tokens: i64,
) -> i64 {
    confirmed_context_occupancy_tokens
        .unwrap_or(persisted_context_tokens)
        .max(0)
}

pub(super) fn build_round_usage_payload(
    round_usage: &TokenUsage,
    context_occupancy_tokens: i64,
    request_round: RoundInfo,
) -> Value {
    let mut usage_payload = json!({
        "input_tokens": round_usage.input,
        "output_tokens": round_usage.output,
        "total_tokens": round_usage.total,
        "context_occupancy_tokens": context_occupancy_tokens.max(0),
        "request_consumed_tokens": round_usage.total,
    });
    if let Value::Object(ref mut map) = usage_payload {
        request_round.insert_into(map);
    }
    usage_payload
}

pub(super) fn build_final_event_payload(
    answer: &str,
    response_usage: Option<&TokenUsage>,
    round_usage: &TokenUsage,
    context_occupancy_tokens: i64,
    stop_reason: &str,
    stop_meta: Option<&Value>,
    round_info: RoundInfo,
    turn_decode_speed: &TurnDecodeSpeedAccumulator,
) -> Value {
    let mut final_payload = json!({
        "answer": answer,
        "usage": response_usage.cloned().unwrap_or(TokenUsage {
            input: 0,
            output: 0,
            total: 0,
        }),
        "round_usage": round_usage,
        "context_occupancy_tokens": context_occupancy_tokens,
        "stop_reason": stop_reason
    });
    if let Value::Object(ref mut map) = final_payload {
        if let Some(meta) = stop_meta {
            map.insert("stop_meta".to_string(), meta.clone());
        }
        round_info.insert_into(map);
        turn_decode_speed.insert_into_map(map);
    }
    final_payload
}

pub(super) fn extract_workspace_changed_paths(
    meta: Option<&Value>,
    data: &Value,
    args: &Value,
    workspace_id: &str,
) -> Vec<String> {
    let mut output = Vec::new();
    if let Some(meta_obj) = meta.and_then(Value::as_object) {
        collect_workspace_paths_from_object(meta_obj, workspace_id, &mut output);
    }
    if let Some(data_obj) = data.as_object() {
        collect_workspace_paths_from_object(data_obj, workspace_id, &mut output);
    }
    if let Some(args_obj) = args.as_object() {
        collect_workspace_paths_from_object(args_obj, workspace_id, &mut output);
    }
    output
}

fn collect_workspace_paths_from_object(
    source: &Map<String, Value>,
    workspace_id: &str,
    output: &mut Vec<String>,
) {
    for key in WORKSPACE_PATH_HINT_KEYS {
        if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
            return;
        }
        if let Some(value) = source.get(key) {
            collect_workspace_paths_from_value(value, workspace_id, output);
        }
    }
    for key in WORKSPACE_EVENT_NESTED_OBJECT_KEYS {
        if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
            return;
        }
        if let Some(value) = source.get(key) {
            collect_workspace_paths_from_value(value, workspace_id, output);
        }
    }
}

fn collect_workspace_paths_from_value(value: &Value, workspace_id: &str, output: &mut Vec<String>) {
    if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
        return;
    }
    match value {
        Value::String(text) => push_workspace_changed_path(text, workspace_id, output),
        Value::Array(items) => {
            for item in items {
                if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
                    break;
                }
                collect_workspace_paths_from_value(item, workspace_id, output);
            }
        }
        Value::Object(map) => collect_workspace_paths_from_object(map, workspace_id, output),
        _ => {}
    }
}

fn push_workspace_changed_path(raw: &str, workspace_id: &str, output: &mut Vec<String>) {
    let Some(normalized) = normalize_workspace_changed_path(raw, workspace_id) else {
        return;
    };
    if output.iter().any(|existing| existing == &normalized) {
        return;
    }
    output.push(normalized);
}

fn normalize_workspace_changed_path(raw: &str, workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("file://") {
        value = stripped.to_string();
    }
    if let Some(index) = value.find(['?', '#']) {
        value.truncate(index);
    }
    if value == "/" || value == "." {
        return Some(String::new());
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        // Ignore absolute Windows drive paths because they are not stable client hints.
        return None;
    }
    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix("workspaces/") {
        value = stripped.to_string();
        let mut parts = value.splitn(2, '/');
        let owner = parts.next().unwrap_or_default().trim();
        let rest = parts.next().unwrap_or_default();
        if owner == workspace_id {
            value = rest.to_string();
        } else if !owner.is_empty() {
            return None;
        }
    } else if let Some(stripped) = value.strip_prefix("/workspace/") {
        value = stripped.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspace/") {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix('/') {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix(&format!("{workspace_id}/")) {
        value = stripped.to_string();
    }
    if value == workspace_id || value == "." || value == "/" {
        return Some(String::new());
    }
    Some(value.trim_matches('/').to_string())
}

pub(super) fn extract_container_id_from_workspace_id(workspace_id: &str) -> i32 {
    if let Some((_, suffix)) = workspace_id.rsplit_once("__c__") {
        if let Ok(parsed) = suffix.parse::<i32>() {
            return crate::storage::normalize_workspace_container_id(parsed);
        }
    }
    if workspace_id.contains("__a__") || workspace_id.contains("__agent__") {
        return crate::storage::DEFAULT_SANDBOX_CONTAINER_ID;
    }
    crate::storage::USER_PRIVATE_CONTAINER_ID
}

pub(super) fn approval_kind_for_tool(tool_name: &str) -> ApprovalRequestKind {
    let exec_tool = resolve_tool_name("execute_command");
    let ptc_tool = resolve_tool_name("ptc");
    let controller_tool = resolve_tool_name("desktop_controller");
    let monitor_tool = resolve_tool_name("desktop_monitor");
    if tool_name == exec_tool || tool_name == ptc_tool {
        ApprovalRequestKind::Exec
    } else if tool_name == controller_tool || tool_name == monitor_tool {
        ApprovalRequestKind::Control
    } else {
        ApprovalRequestKind::Patch
    }
}

pub(super) fn approval_summary_for_tool(
    tool_name: &str,
    args: &Value,
    kind: ApprovalRequestKind,
) -> String {
    match kind {
        ApprovalRequestKind::Exec => extract_command_text(args)
            .map(|cmd| format!("{tool_name}: {cmd}"))
            .unwrap_or_else(|| tool_name.to_string()),
        ApprovalRequestKind::Patch => args
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|path| format!("{tool_name}: {path}"))
            .unwrap_or_else(|| tool_name.to_string()),
        ApprovalRequestKind::Control => extract_control_summary(args)
            .map(|summary| format!("{tool_name}: {summary}"))
            .unwrap_or_else(|| tool_name.to_string()),
    }
}

pub(super) fn approval_resolution_status_and_scope(
    approval_response: ApprovalResponse,
) -> (&'static str, &'static str) {
    match approval_response {
        ApprovalResponse::ApproveSession => ("approved", "session"),
        ApprovalResponse::ApproveOnce => ("approved", "once"),
        ApprovalResponse::Deny => ("denied", "none"),
    }
}

pub(super) async fn emit_approval_resolved_event(
    emitter: &EventEmitter,
    round_info: RoundInfo,
    event: ApprovalResolvedEvent<'_>,
) {
    let mut payload = json!({
        "approval_id": event.approval_id,
        "status": event.status,
        "scope": event.scope,
        "kind": event.kind,
        "tool": event.tool_name,
        "summary": event.summary.unwrap_or_default(),
    });
    if let Value::Object(ref mut map) = payload {
        if let Some(resolved_by) = event
            .resolved_by
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                "resolved_by".to_string(),
                Value::String(resolved_by.to_string()),
            );
        }
        round_info.insert_into(map);
    }
    emitter.emit("approval_resolved", payload).await;
}

pub(super) fn turn_terminal_status_for_error(err: &OrchestratorError) -> &'static str {
    match err.code() {
        "CANCELLED" => "cancelled",
        "USER_BUSY" | "USER_QUOTA_EXCEEDED" | "USER_TOKEN_INSUFFICIENT" | "INVALID_REQUEST" => {
            "rejected"
        }
        _ => "failed",
    }
}

pub(super) async fn emit_turn_terminal_event(
    emitter: &EventEmitter,
    round_info: RoundInfo,
    event: TurnTerminalEvent<'_>,
) {
    let mut payload = json!({
        "status": event.status,
        "retryable": event.error.map(OrchestratorError::retryable).unwrap_or(false),
        "waiting_for_user_input": event.waiting_for_user_input,
    });
    if let Value::Object(ref mut map) = payload {
        if let Some(stop_reason) = event
            .stop_reason
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                "stop_reason".to_string(),
                Value::String(stop_reason.to_string()),
            );
        }
        if let Some(round_usage) = event.round_usage {
            map.insert("round_usage".to_string(), json!(round_usage));
        }
        if let Some(error) = event.error {
            map.insert("error".to_string(), error.to_payload());
            map.insert(
                "recovery_action".to_string(),
                Value::String(error.recovery_action().to_string()),
            );
            if let Some(retry_after_ms) = error.retry_after_ms() {
                map.insert("retry_after_ms".to_string(), json!(retry_after_ms));
            }
        }
        if let Some(stop_meta) = event.stop_meta {
            map.insert("stop_meta".to_string(), stop_meta.clone());
        }
        round_info.insert_into(map);
    }
    emitter.emit("turn_terminal", payload).await;
}

pub(super) struct ApprovalResolvedEvent<'a> {
    pub(super) approval_id: &'a str,
    pub(super) status: &'a str,
    pub(super) scope: &'a str,
    pub(super) kind: Option<ApprovalRequestKind>,
    pub(super) tool_name: &'a str,
    pub(super) summary: Option<&'a str>,
    pub(super) resolved_by: Option<&'a str>,
}

pub(super) struct TurnTerminalEvent<'a> {
    pub(super) status: &'a str,
    pub(super) stop_reason: Option<&'a str>,
    pub(super) round_usage: Option<&'a TokenUsage>,
    pub(super) error: Option<&'a OrchestratorError>,
    pub(super) waiting_for_user_input: bool,
    pub(super) stop_meta: Option<&'a Value>,
}

fn extract_control_summary(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    let action = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(action) = action {
        let desc = obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(desc) = desc {
            return Some(format!("action={action} {desc}"));
        }
        return Some(format!("action={action}"));
    }
    if let Some(wait_ms) = obj.get("wait_ms") {
        if let Some(value) = wait_ms
            .as_i64()
            .or_else(|| wait_ms.as_u64().map(|v| v as i64))
        {
            return Some(format!("wait_ms={value}"));
        }
    }
    None
}

fn extract_command_text(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in ["content", "command", "cmd"] {
        if let Some(Value::String(text)) = obj.get(key) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

pub(super) fn args_with_approved_flag(args: &Value) -> Value {
    if let Some(obj) = args.as_object() {
        let mut updated = obj.clone();
        updated.insert("approved".to_string(), Value::Bool(true));
        return Value::Object(updated);
    }
    json!({ "raw": args, "approved": true })
}

#[cfg(test)]
mod tests;
