use super::{
    apply_tool_overrides, error_response, fetch_agent_record, finalize_tool_names,
    normalize_tool_overrides, resolve_agent_tool_defaults, resolve_agent_workspace_id,
    resolve_chat_model_name, resolve_session_tool_overrides,
};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::llm::{
    build_llm_client, is_llm_model, resolve_tool_call_mode, ChatMessage, ToolCallMode,
};
use crate::state::AppState;
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/chat/system-prompt", post(system_prompt))
        .route(
            "/wunder/chat/sessions/{session_id}/system-prompt",
            post(session_system_prompt),
        )
}

#[derive(Debug, Deserialize)]
struct SystemPromptRequest {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_overrides: Option<Vec<String>>,
    #[serde(default)]
    question: Option<String>,
}

async fn system_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SystemPromptRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_context = build_user_tool_context(&state, &resolved.user.user_id).await;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, payload.agent_id.as_deref(), false).await?;
    let mut allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let overrides = payload
        .tool_overrides
        .map(normalize_tool_overrides)
        .unwrap_or_else(|| resolve_agent_tool_defaults(agent_record.as_ref()));
    let agent_defaults = resolve_agent_tool_defaults(agent_record.as_ref());
    allowed = apply_tool_overrides(allowed, &overrides, &agent_defaults);
    let tool_names = finalize_tool_names(allowed.clone());
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);
    let workspace_id = resolve_agent_workspace_id(
        &state,
        &resolved.user.user_id,
        payload.agent_id.as_deref(),
        agent_record.as_ref(),
    );
    let prompt = state
        .kernel
        .orchestrator
        .build_system_prompt(
            &user_context.config,
            &tool_names,
            &user_context.skills,
            Some(&user_context.bindings),
            &resolved.user.user_id,
            payload.agent_id.as_deref(),
            UserStore::is_admin(&resolved.user),
            &workspace_id,
            None,
            agent_prompt.as_deref(),
            preview_skill,
        )
        .await;
    let tooling_preview = build_prompt_tooling_preview_payload(
        &state,
        &resolved.user.user_id,
        &user_context,
        &allowed,
        agent_record.as_ref(),
        &prompt,
        payload.question.as_deref(),
    );
    Ok(Json(json!({
        "data": build_system_prompt_preview_payload(prompt, "pending", Some(tooling_preview)),
    })))
}

async fn session_system_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SystemPromptRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let workspace_id = resolve_agent_workspace_id(
        &state,
        &resolved.user.user_id,
        record.agent_id.as_deref().or(payload.agent_id.as_deref()),
        None,
    );
    let workspace_root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .unwrap_or_else(|_| state.workspace.root().to_path_buf());
    let expected_public_workdir = state.workspace.display_path(&workspace_id, &workspace_root);
    let expected_local_workdir = workspace_root.to_string_lossy().replace('\\', "/");
    let request_overrides = payload
        .tool_overrides
        .as_ref()
        .map(|values| normalize_tool_overrides(values.clone()));
    let stored_prompt = state
        .workspace
        .load_session_system_prompt_async(&resolved.user.user_id, &session_id, None)
        .await
        .unwrap_or(None);
    let frozen_tool_overrides = state
        .workspace
        .load_session_frozen_tool_overrides_async(&resolved.user.user_id, &session_id)
        .await;
    let user_context = build_user_tool_context(&state, &resolved.user.user_id).await;
    let agent_record = fetch_agent_record(
        &state,
        &resolved.user,
        record.agent_id.as_deref().or(payload.agent_id.as_deref()),
        true,
    )
    .await?;
    let mut allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let overrides = request_overrides.clone().unwrap_or_else(|| {
        resolve_session_tool_overrides(
            &record,
            frozen_tool_overrides.as_deref(),
            agent_record.as_ref(),
        )
    });
    let agent_defaults = resolve_agent_tool_defaults(agent_record.as_ref());
    allowed = apply_tool_overrides(allowed, &overrides, &agent_defaults);
    let tool_names = finalize_tool_names(allowed.clone());
    if request_overrides.is_none() {
        if let Some(prompt) = stored_prompt {
            if prompt_has_workdir(&prompt, &expected_public_workdir, &expected_local_workdir) {
                let tooling_preview = build_prompt_tooling_preview_payload(
                    &state,
                    &resolved.user.user_id,
                    &user_context,
                    &allowed,
                    agent_record.as_ref(),
                    &prompt,
                    payload.question.as_deref(),
                );
                return Ok(Json(json!({
                    "data": build_system_prompt_preview_payload(
                        prompt,
                        "frozen",
                        Some(tooling_preview.clone()),
                    ),
                })));
            }
        }
    }
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);
    let workspace_id = resolve_agent_workspace_id(
        &state,
        &resolved.user.user_id,
        record.agent_id.as_deref().or(payload.agent_id.as_deref()),
        agent_record.as_ref(),
    );
    let prompt = state
        .kernel
        .orchestrator
        .build_system_prompt(
            &user_context.config,
            &tool_names,
            &user_context.skills,
            Some(&user_context.bindings),
            &resolved.user.user_id,
            record.agent_id.as_deref().or(payload.agent_id.as_deref()),
            UserStore::is_admin(&resolved.user),
            &workspace_id,
            None,
            agent_prompt.as_deref(),
            preview_skill,
        )
        .await;
    let tooling_preview = build_prompt_tooling_preview_payload(
        &state,
        &resolved.user.user_id,
        &user_context,
        &allowed,
        agent_record.as_ref(),
        &prompt,
        payload.question.as_deref(),
    );
    Ok(Json(json!({
        "data": build_system_prompt_preview_payload(prompt, "pending", Some(tooling_preview)),
    })))
}

fn prompt_has_workdir(prompt: &str, public_workdir: &str, local_workdir: &str) -> bool {
    let cleaned_prompt = prompt.trim();
    if cleaned_prompt.is_empty() {
        return false;
    }
    let public = public_workdir.trim();
    if !public.is_empty() && cleaned_prompt.contains(public) {
        return true;
    }
    let local = local_workdir.trim();
    !local.is_empty() && cleaned_prompt.contains(local)
}

fn sanitize_system_prompt_preview(prompt: String) -> String {
    let placeholder = crate::prompting::SYSTEM_PROMPT_MEMORY_PLACEHOLDER;
    if !prompt.contains(placeholder) {
        return prompt;
    }
    let cleaned = prompt.replace(placeholder, "");
    collapse_blank_lines(&cleaned)
}

fn collapse_blank_lines(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut run = 0usize;
    for ch in normalized.chars() {
        if ch == '\n' {
            run = run.saturating_add(1);
            if run <= 2 {
                out.push('\n');
            }
        } else {
            run = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn extract_system_prompt_memory_preview(prompt: &str) -> String {
    let prefix = crate::i18n::t("memory.block_prefix");
    let normalized_prefix = prefix.trim();
    if normalized_prefix.is_empty() {
        return String::new();
    }
    let cleaned = prompt.trim();
    let Some(index) = cleaned.find(normalized_prefix) else {
        return String::new();
    };
    collapse_blank_lines(&cleaned[index..])
}

fn count_system_prompt_memory_items(memory_preview: &str) -> usize {
    memory_preview
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("- ["))
        .count()
}

fn extract_system_prompt_memory_total_count(memory_preview: &str) -> Option<usize> {
    memory_preview
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('-') && !line.starts_with("- ["))
        .and_then(|line| {
            line.split(|ch: char| !ch.is_ascii_digit())
                .find(|segment| !segment.is_empty())
        })
        .and_then(|value| value.parse::<usize>().ok())
}

fn tool_call_mode_key(mode: ToolCallMode) -> &'static str {
    match mode {
        ToolCallMode::FunctionCall => "function_call",
        ToolCallMode::ToolCall => "tool_call",
        ToolCallMode::FreeformCall => "freeform_call",
    }
}

fn resolve_system_prompt_tool_call_mode(
    config: &crate::config::Config,
    agent_record: Option<&crate::storage::UserAgentRecord>,
) -> ToolCallMode {
    let Some(model_name) = resolve_chat_model_name(config, agent_record) else {
        return ToolCallMode::FunctionCall;
    };
    config
        .llm
        .models
        .get(&model_name)
        .filter(|model| is_llm_model(model))
        .map(resolve_tool_call_mode)
        .unwrap_or(ToolCallMode::FunctionCall)
}

fn build_prompt_tooling_preview_payload(
    state: &Arc<AppState>,
    user_id: &str,
    user_context: &crate::user_access::UserToolContext,
    allowed_tool_names: &HashSet<String>,
    agent_record: Option<&crate::storage::UserAgentRecord>,
    system_prompt: &str,
    question: Option<&str>,
) -> Value {
    let tool_call_mode = resolve_system_prompt_tool_call_mode(&user_context.config, agent_record);
    let selected_tool_names = finalize_tool_names(allowed_tool_names.clone());
    let runtime_tool_display_map =
        crate::tools::build_runtime_tool_display_map(&user_context.config);
    let selected_tool_display_map = selected_tool_names
        .iter()
        .map(|name| {
            (
                name.clone(),
                runtime_tool_display_map
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
            )
        })
        .collect::<HashMap<String, String>>();
    let tooling = state.kernel.orchestrator.build_function_tooling(
        &user_context.config,
        &user_context.skills,
        allowed_tool_names,
        Some(&user_context.bindings),
        tool_call_mode,
        user_id,
        agent_record.map(|record| record.agent_id.as_str()),
        user_id,
    );
    let llm_tools = tooling
        .as_ref()
        .map(|resolved| resolved.tools.clone())
        .unwrap_or_default();
    let llm_tool_name_map = tooling
        .as_ref()
        .map(|resolved| resolved.display_map.clone())
        .unwrap_or_default();
    let model_request = build_system_prompt_model_request_preview(
        &user_context.config,
        agent_record,
        system_prompt,
        question,
        llm_tools.as_slice(),
    );
    json!({
        "tool_call_mode": tool_call_mode_key(tool_call_mode),
        "selected_tool_names": selected_tool_names,
        "selected_tool_display_map": selected_tool_display_map,
        "llm_tools": llm_tools,
        "llm_tool_name_map": llm_tool_name_map,
        "model_request": model_request,
    })
}

fn build_system_prompt_model_request_preview(
    config: &crate::config::Config,
    agent_record: Option<&crate::storage::UserAgentRecord>,
    system_prompt: &str,
    question: Option<&str>,
    llm_tools: &[Value],
) -> Value {
    let Some(model_name) = resolve_chat_model_name(config, agent_record) else {
        return json!({
            "messages": [{
                "role": "system",
                "content": system_prompt,
            }],
            "tools": llm_tools,
        });
    };
    let Some(llm_config) = config
        .llm
        .models
        .get(&model_name)
        .filter(|model| is_llm_model(model))
    else {
        return json!({
            "messages": [{
                "role": "system",
                "content": system_prompt,
            }],
            "tools": llm_tools,
        });
    };
    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: Value::String(system_prompt.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    if let Some(question) = question.map(str::trim).filter(|value| !value.is_empty()) {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Value::String(question.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }
    let tools = if llm_tools.is_empty() {
        None
    } else {
        Some(llm_tools)
    };
    build_llm_client(llm_config, reqwest::Client::new())
        .build_request_payload_with_tools(&messages, true, tools)
}

fn build_system_prompt_preview_payload(
    prompt: String,
    memory_mode_hint: &str,
    tooling_preview: Option<Value>,
) -> Value {
    let memory_preview = extract_system_prompt_memory_preview(&prompt);
    let memory_preview_count = count_system_prompt_memory_items(&memory_preview);
    let memory_preview_total_count =
        extract_system_prompt_memory_total_count(&memory_preview).unwrap_or(memory_preview_count);
    let memory_preview_mode = if memory_preview_count == 0 {
        "none"
    } else {
        memory_mode_hint
    };
    let mut payload = json!({
        "prompt": sanitize_system_prompt_preview(prompt),
        "memory_preview": memory_preview,
        "memory_preview_mode": memory_preview_mode,
        "memory_preview_count": memory_preview_count,
        "memory_preview_total_count": memory_preview_total_count,
    });
    if let Some(tooling_preview) = tooling_preview {
        if let Some(map) = payload.as_object_mut() {
            map.insert("tooling_preview".to_string(), tooling_preview);
        }
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::{
        count_system_prompt_memory_items, extract_system_prompt_memory_preview,
        extract_system_prompt_memory_total_count,
    };

    #[test]
    fn extract_system_prompt_memory_preview_reads_tail_block() {
        let prefix = crate::i18n::t("memory.block_prefix");
        let prompt = format!(
            "You are a helpful assistant.\n\n{}\n- Available long-term memories: 7. Injected now: 2 (limit 30).\n- The injected items below are memory indexes only (memory_id + title). Use memory_manager get with a memory_id when you need the full detail.\n- If the injected indexes below are not enough, continue with memory_manager list/search to find more memory ids, then use get for the full detail.\n- [2026-04-12 09:00] mem_a | Item one\n- [2026-04-12 09:00] mem_b | Item two",
            prefix
        );
        let memory = extract_system_prompt_memory_preview(&prompt);
        assert!(memory.contains(&prefix));
        assert_eq!(count_system_prompt_memory_items(&memory), 2);
        assert_eq!(extract_system_prompt_memory_total_count(&memory), Some(7));
    }
}
