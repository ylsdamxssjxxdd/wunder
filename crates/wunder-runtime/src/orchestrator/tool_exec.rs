use super::context::{normalize_model_context_message, MODEL_CONTEXT_INTERNAL_META_TYPE};
use super::*;

pub(super) const TOOL_TIMEOUT_ERROR: &str = "tool_timeout";

use super::tool_result_payload::{
    append_truncation_reason, collect_truncation_reasons_from_value,
    compact_large_tool_result_data, dedupe_truncation_reasons, estimate_tool_result_chars,
    extract_exit_code, merge_tool_result_meta, should_skip_event_payload_truncation,
    supports_tool_result_continuation, truncate_tool_result_data, truncate_tool_result_string,
    TRUNCATION_CONTINUATION_HINT,
};
impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
        attachments: Option<&[AttachmentPayload]>,
        meta: Option<&Value>,
        reasoning: Option<&str>,
        tool_calls: Option<&Value>,
        tool_call_id: Option<&str>,
        model_content: Option<&Value>,
        round_info: RoundInfo,
    ) {
        if let Err(err) = self.try_append_chat(
            user_id,
            session_id,
            role,
            content,
            attachments,
            meta,
            reasoning,
            tool_calls,
            tool_call_id,
            model_content,
            round_info,
        ) {
            warn!("append chat failed for session {session_id} role {role}: {err}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
        attachments: Option<&[AttachmentPayload]>,
        meta: Option<&Value>,
        reasoning: Option<&str>,
        tool_calls: Option<&Value>,
        tool_call_id: Option<&str>,
        model_content: Option<&Value>,
        round_info: RoundInfo,
    ) -> Result<()> {
        let timestamp = Local::now().to_rfc3339();
        let content_value = content
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        let content_value = match content_value {
            Value::String(_) | Value::Array(_) | Value::Object(_) => content_value,
            Value::Null => Value::String(String::new()),
            other => Value::String(other.to_string()),
        };
        let mut payload = json!({
            "role": role,
            "content": content_value,
            "session_id": session_id,
            "timestamp": timestamp,
        });
        let has_round_info = round_info.user_round.is_some() || round_info.model_round.is_some();
        if let Value::Object(ref mut map) = payload {
            round_info.insert_into(map);
            if has_round_info {
                map.insert(
                    "round_info_source".to_string(),
                    Value::String("orchestrator".to_string()),
                );
            }
        }
        if let Some(model_content) = model_content {
            if !model_content.is_null() {
                payload["model_content"] = model_content.clone();
            }
        }
        if let Some(reasoning) = reasoning {
            let cleaned = reasoning.trim();
            if !cleaned.is_empty() {
                payload["reasoning_content"] = Value::String(cleaned.to_string());
            }
        }
        if let Some(meta) = meta {
            if !meta.is_null() {
                payload["meta"] = meta.clone();
            }
        }
        if let Some(tool_calls) = tool_calls {
            if !tool_calls.is_null() {
                payload["tool_calls"] = tool_calls.clone();
            }
        }
        if let Some(tool_call_id) = tool_call_id {
            let cleaned = tool_call_id.trim();
            if !cleaned.is_empty() {
                payload["tool_call_id"] = Value::String(cleaned.to_string());
            }
        }
        if let Some(attachments) = attachments {
            if let Some(value) = build_history_attachments(attachments) {
                payload["attachments"] = value;
            }
        }
        let turn_id = match round_info.thread_turn_id {
            Some(id) => Some(id.to_string()),
            None => match round_info.user_round {
                Some(round) => self
                    .storage
                    .find_thread_turn_id(user_id, session_id, round)?,
                None => None,
            },
        };
        if let Some(turn_id) = turn_id {
            // Only an admitted request owns the stable input slot. Commands,
            // summaries and internal observations append independent messages.
            let input_slot = role == "user"
                && round_info.model_round.is_none()
                && round_info.thread_turn_id.is_some()
                && meta
                    .and_then(|value| value.get("type"))
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "subagent_hidden_user");
            // Every visible assistant write that belongs to a model round is
            // the same durable output item.  Stats are allowed to arrive in a
            // later write (and tool/question terminal paths do not all carry
            // the stats object), so identity must not depend on metadata.
            // Hidden model-context messages stay append-only and never replace
            // the user-visible stream item.
            let output_slot = role == "assistant"
                && round_info.model_round.is_some()
                && meta
                    .and_then(|value| value.get("hidden"))
                    .and_then(Value::as_bool)
                    != Some(true);
            let item_id = if input_slot {
                format!("{turn_id}:user")
            } else if output_slot {
                // A model round has exactly one visible durable output item.
                // The stream registers it as `{turn}:text-{round}` at request
                // admission; final history and message statistics must update
                // that same item instead of appending a random-id duplicate.
                format!(
                    "{turn_id}:text-{}",
                    round_info
                        .model_round
                        .expect("output slot requires a model round")
                )
            } else {
                Uuid::new_v4().to_string()
            };
            payload["turn_id"] = Value::String(turn_id);
            payload["item_id"] = Value::String(item_id);
        }
        let payload =
            crate::services::chat_payload_sanitizer::sanitize_persisted_chat_payload(&payload);
        self.workspace.append_chat(user_id, &payload)
    }

    pub(super) fn append_internal_model_context_chat(
        &self,
        user_id: &str,
        session_id: &str,
        message: &Value,
        source: &str,
        round_info: RoundInfo,
    ) {
        if let Err(err) = self.try_append_internal_model_context_chat(
            user_id, session_id, message, source, round_info,
        ) {
            warn!("append internal model context chat failed for session {session_id}: {err}");
        }
    }

    pub(super) fn try_append_internal_model_context_chat(
        &self,
        user_id: &str,
        session_id: &str,
        message: &Value,
        source: &str,
        round_info: RoundInfo,
    ) -> Result<()> {
        let Some(payload) = normalize_model_context_message(message.clone()) else {
            return Ok(());
        };
        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = payload.get("content").cloned().unwrap_or(Value::Null);
        let mut meta = json!({
            "type": MODEL_CONTEXT_INTERNAL_META_TYPE,
            "hidden": true,
            "internal_user": role == "user",
            "source": source,
        });
        if let Value::Object(ref mut map) = meta {
            if let Some(tool_call_id) = payload.get("tool_call_id").and_then(Value::as_str) {
                map.insert(
                    "tool_call_id".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
            }
        }
        self.try_append_chat(
            user_id,
            session_id,
            role,
            Some(&content),
            None,
            Some(&meta),
            payload.get("reasoning_content").and_then(Value::as_str),
            payload.get("tool_calls"),
            payload.get("tool_call_id").and_then(Value::as_str),
            None,
            round_info,
        )
    }

    pub(super) fn mark_internal_model_context_message(
        &self,
        message: &mut Value,
        source: &str,
        round_info: RoundInfo,
    ) {
        let Some(obj) = message.as_object_mut() else {
            return;
        };
        let mut meta = json!({
            "type": MODEL_CONTEXT_INTERNAL_META_TYPE,
            "hidden": true,
            "internal_user": obj.get("role").and_then(Value::as_str) == Some("user"),
            "source": source,
        });
        if let Value::Object(ref mut map) = meta {
            if let Some(tool_call_id) = obj.get("tool_call_id").and_then(Value::as_str) {
                map.insert(
                    "tool_call_id".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
            }
            round_info.insert_into(map);
        }
        obj.insert("meta".to_string(), meta);
    }

    pub(super) fn build_tool_observation(
        &self,
        tool_name: &str,
        result: &ToolResultPayload,
    ) -> String {
        let payload = result.to_compact_payload(tool_name);
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    }

    pub(super) fn append_tool_log(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
        round_info: RoundInfo,
        tool_call_id: Option<&str>,
        request_context_tokens: Option<i64>,
        request_usage: Option<&TokenUsage>,
    ) {
        let timestamp = Local::now().to_rfc3339();
        let safe_args = if args.is_object() {
            args.clone()
        } else {
            json!({ "raw": args })
        };
        // Tool logs always persist in the compact profile; the storage layer
        // bounds args/data and never stores the full payload column.
        let mut payload = json!({
            "tool": tool_name,
            "session_id": session_id,
            "ok": result.ok,
            "error": result.error,
            "args": safe_args,
            "data": result.data,
            "timestamp": timestamp,
            "event_type": "tool_result",
        });
        if let Some(tokens) = request_context_tokens {
            payload["request_context_tokens"] = json!(tokens);
        }
        if let Some(usage) = request_usage {
            payload["request_usage"] = serde_json::to_value(usage).unwrap_or(Value::Null);
        }
        if let Some(tool_call_id) = tool_call_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            payload["tool_call_id"] = Value::String(tool_call_id.to_string());
        }
        if let Some(meta) = &result.meta {
            payload["meta"] = meta.clone();
        }
        if result.sandbox {
            payload["sandbox"] = Value::Bool(true);
        }
        round_info.insert_into(payload.as_object_mut().expect("tool payload object"));
        if let Some(turn_id) = round_info.thread_turn_id {
            let tool_id = payload
                .get("tool_call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|id| format!("{turn_id}:tool-{id}"))
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            payload["item_id"] = Value::String(tool_id);
        }
        if let Err(err) = self.workspace.append_tool_log(user_id, &payload) {
            warn!("append tool log failed for session {session_id} tool {tool_name}: {err}");
        }
    }

    pub(super) fn finalize_tool_result(
        &self,
        tool_name: &str,
        mut result: ToolResultPayload,
        started_at: Instant,
    ) -> ToolResultPayload {
        let skip_truncation = should_skip_event_payload_truncation(tool_name);
        let duration_ms = started_at.elapsed().as_millis() as i64;
        let continuation_supported =
            supports_tool_result_continuation(&result.data, result.meta.as_ref());
        let mut truncated = false;
        let mut truncation_reasons: Vec<String> = Vec::new();
        if !skip_truncation {
            truncated = truncate_tool_result_data(
                &mut result.data,
                TOOL_RESULT_HEAD_CHARS,
                TOOL_RESULT_TAIL_CHARS,
                TOOL_RESULT_TRUNCATION_MARKER,
            );
            if result.error.len() > TOOL_RESULT_MAX_CHARS {
                result.error = truncate_tool_result_string(
                    &result.error,
                    TOOL_RESULT_HEAD_CHARS,
                    TOOL_RESULT_TAIL_CHARS,
                    TOOL_RESULT_TRUNCATION_MARKER,
                );
                truncated = true;
                append_truncation_reason(&mut truncation_reasons, "string_chars");
            }
            if truncated {
                truncation_reasons.extend(collect_truncation_reasons_from_value(
                    &result.data,
                    TOOL_RESULT_TRUNCATION_MARKER,
                ));
                dedupe_truncation_reasons(&mut truncation_reasons);
            }
        }
        let mut output_chars = estimate_tool_result_chars(&result.data);
        if !skip_truncation && output_chars > TOOL_RESULT_MAX_CHARS {
            append_truncation_reason(&mut truncation_reasons, "char_budget");
            result.data = compact_large_tool_result_data(
                &result.data,
                output_chars,
                TOOL_RESULT_HEAD_CHARS,
                TOOL_RESULT_TAIL_CHARS,
                TOOL_RESULT_TRUNCATION_MARKER,
                continuation_supported,
                &truncation_reasons,
            );
            truncated = true;
            output_chars = estimate_tool_result_chars(&result.data);
        }
        let exit_code = extract_exit_code(&result.data);
        let mut meta = merge_tool_result_meta(result.meta.take());
        meta.insert("duration_ms".to_string(), json!(duration_ms));
        meta.insert("truncated".to_string(), json!(truncated));
        meta.insert("output_chars".to_string(), json!(output_chars));
        if truncated && !truncation_reasons.is_empty() {
            meta.insert(
                "truncation_reasons".to_string(),
                Value::Array(
                    truncation_reasons
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
        }
        if truncated && continuation_supported {
            meta.insert("continuation_required".to_string(), Value::Bool(true));
            meta.insert(
                "continuation_hint".to_string(),
                Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
            );
        }
        if let Some(exit_code) = exit_code {
            meta.insert("exit_code".to_string(), json!(exit_code));
        }
        if meta.is_empty() {
            result.meta = None;
        } else {
            result.meta = Some(Value::Object(meta));
        }
        result
    }

    pub(super) fn append_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) {
        let entries = self.build_artifact_entries(tool_name, args, result);
        if entries.is_empty() {
            return;
        }
        let timestamp = Local::now().to_rfc3339();
        for mut entry in entries {
            if let Value::Object(ref mut map) = entry {
                map.entry("tool".to_string())
                    .or_insert_with(|| Value::String(tool_name.to_string()));
                map.entry("ok".to_string())
                    .or_insert_with(|| Value::Bool(result.ok));
                if !result.error.trim().is_empty() {
                    map.entry("error".to_string())
                        .or_insert_with(|| Value::String(result.error.clone()));
                }
                map.insert(
                    "session_id".to_string(),
                    Value::String(session_id.to_string()),
                );
                map.insert("timestamp".to_string(), Value::String(timestamp.clone()));
            }
            if let Err(err) = self.workspace.append_artifact_log(user_id, &entry) {
                warn!(
                    "append artifact log failed for session {session_id} tool {tool_name}: {err}"
                );
            }
        }
    }

    pub(super) fn build_artifact_entries(
        &self,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) -> Vec<Value> {
        let mut entries = Vec::new();
        let file_actions = HashMap::from([
            ("读取文件", "read"),
            ("写入文件", "write"),
            ("编辑", "edit"),
        ]);
        if let Some(action) = file_actions.get(tool_name) {
            let paths = extract_file_paths(args);
            for path in paths {
                let mut meta = serde_json::Map::new();
                if let Value::Object(data) = &result.data {
                    if *action == "write" {
                        if let Some(value) = data.get("bytes") {
                            meta.insert("bytes".to_string(), value.clone());
                        }
                    } else if *action == "patch" {
                        if let Some(value) = data.get("changed_files") {
                            meta.insert("changed_files".to_string(), value.clone());
                        }
                        if let Some(value) = data.get("hunks_applied") {
                            meta.insert("hunks_applied".to_string(), value.clone());
                        }
                    }
                }
                entries.push(json!({
                    "kind": "file",
                    "action": action,
                    "name": path,
                    "meta": Value::Object(meta),
                }));
            }
            return entries;
        }

        if tool_name == "执行命令" {
            let commands = extract_command_lines(args);
            let mut returncode_map = HashMap::new();
            let mut fallback_rc: Option<Value> = None;
            if let Value::Object(data) = &result.data {
                if let Some(Value::Array(items)) = data.get("results") {
                    for item in items {
                        let Some(obj) = item.as_object() else {
                            continue;
                        };
                        let command = obj
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if command.is_empty() {
                            continue;
                        }
                        returncode_map.insert(
                            command,
                            obj.get("returncode").cloned().unwrap_or(Value::Null),
                        );
                    }
                }
                if data.contains_key("returncode") {
                    fallback_rc = data.get("returncode").cloned();
                }
            }
            for command in commands {
                let returncode = returncode_map
                    .get(&command)
                    .cloned()
                    .or_else(|| fallback_rc.clone());
                let ok = match returncode.as_ref().and_then(Value::as_i64) {
                    Some(code) => code == 0,
                    None => result.ok,
                };
                entries.push(json!({
                    "kind": "command",
                    "action": "execute",
                    "name": command,
                    "ok": ok,
                    "meta": { "returncode": returncode.unwrap_or(Value::Null) },
                }));
            }
            return entries;
        }

        if tool_name == "ptc" {
            let mut script_path = String::new();
            if let Value::Object(data) = &result.data {
                script_path = data
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                script_path = args
                    .get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                return entries;
            }
            let returncode = match &result.data {
                Value::Object(data) => data.get("returncode").cloned(),
                _ => None,
            };
            let ok = match returncode.as_ref().and_then(Value::as_i64) {
                Some(code) => code == 0,
                None => result.ok,
            };
            entries.push(json!({
                "kind": "script",
                "action": "run",
                "name": script_path,
                "ok": ok,
                "meta": { "returncode": returncode.unwrap_or(Value::Null) }
            }));
            return entries;
        }

        entries
    }

    pub(super) fn append_skill_usage_logs(
        &self,
        user_id: &str,
        session_id: &str,
        args: &Value,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        round_info: RoundInfo,
    ) {
        let paths = extract_file_paths(args);
        if paths.is_empty() {
            return;
        }
        let mut specs = skills.list_specs();
        if let Some(bindings) = user_tool_bindings {
            if !bindings.skill_specs.is_empty() {
                specs.extend(bindings.skill_specs.iter().cloned());
            }
        }
        if specs.is_empty() {
            return;
        }

        let mut seen_names = HashSet::new();
        let mut path_map: HashMap<PathBuf, String> = HashMap::new();
        for spec in specs {
            let name = spec.name.trim();
            if name.is_empty() {
                continue;
            }
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            let Some(spec_path) = resolve_absolute_path(&spec.path) else {
                continue;
            };
            let key = normalize_compare_path(&spec_path);
            path_map.insert(key, name.to_string());
        }
        if path_map.is_empty() {
            return;
        }

        let mut matched = HashSet::new();
        for raw in paths {
            let Some(candidate) = resolve_absolute_path(&raw) else {
                continue;
            };
            let key = normalize_compare_path(&candidate);
            if let Some(name) = path_map.get(&key) {
                matched.insert(name.clone());
            }
        }
        if matched.is_empty() {
            return;
        }
        let result = ToolResultPayload::from_value(json!({ "source": "skill_read" }));
        for name in matched {
            self.append_tool_log(
                user_id, session_id, &name, args, &result, round_info, None, None, None,
            );
        }
    }

    pub(super) fn resolve_final_answer(&self, content: &str) -> String {
        strip_tool_calls(content).trim().to_string()
    }

    pub(super) fn reconcile_final_answer_workspace_images(
        &self,
        workspace_id: &str,
        session_id: &str,
        answer: &str,
    ) -> String {
        let cleaned_workspace_id = workspace_id.trim();
        if cleaned_workspace_id.is_empty() || answer.trim().is_empty() {
            return answer.to_string();
        }
        let Some(image_regex) = markdown_image_regex() else {
            return answer.to_string();
        };
        if !image_regex.is_match(answer) {
            return answer.to_string();
        }

        let artifact_candidates = collect_existing_artifact_image_paths(
            &self.workspace,
            cleaned_workspace_id,
            session_id,
        );
        let mut dir_candidates: HashMap<String, Vec<String>> = HashMap::new();
        let mut used_replacements: HashSet<String> = HashSet::new();
        let mut changed = false;

        let rewritten = image_regex.replace_all(answer, |caps: &regex::Captures<'_>| {
            let full = caps.get(0).map(|item| item.as_str()).unwrap_or("");
            let alt = caps.get(1).map(|item| item.as_str()).unwrap_or("");
            let raw_target = caps.get(2).map(|item| item.as_str()).unwrap_or("").trim();
            if raw_target.is_empty() {
                return full.to_string();
            }
            let (path_token, title_suffix) = split_markdown_target(raw_target);
            if path_token.is_empty() {
                return full.to_string();
            }
            let (path_token_clean, wrapper) = unwrap_markdown_path_token(&path_token);
            let (path_without_suffix, _) = split_url_suffix(path_token_clean.as_str());
            let Some(normalized_relative) = normalize_workspace_markdown_relative_path(
                &path_without_suffix,
                cleaned_workspace_id,
            ) else {
                return full.to_string();
            };
            if workspace_relative_path_exists(
                &self.workspace,
                cleaned_workspace_id,
                normalized_relative.as_str(),
            ) {
                return full.to_string();
            }

            let replacement = find_missing_image_replacement(
                &self.workspace,
                cleaned_workspace_id,
                normalized_relative.as_str(),
                &artifact_candidates,
                &mut dir_candidates,
                &mut used_replacements,
            );
            let Some(replacement) = replacement else {
                return full.to_string();
            };
            let replaced_target = format_markdown_path_token(
                path_token_clean.as_str(),
                replacement.as_str(),
                cleaned_workspace_id,
                wrapper,
            );
            let rebuilt_target = if title_suffix.is_empty() {
                replaced_target
            } else {
                format!("{replaced_target} {title_suffix}")
            };
            changed = true;
            format!("![{alt}]({rebuilt_target})")
        });

        if changed {
            rewritten.into_owned()
        } else {
            answer.to_string()
        }
    }

    pub(super) async fn execute_tool_with_timeout(
        &self,
        tool_context: &ToolContext<'_>,
        name: &str,
        args: &Value,
        timeout: Option<Duration>,
    ) -> Result<Value, anyhow::Error> {
        if let Some(timeout) = timeout {
            match tokio::time::timeout(
                timeout,
                crate::tools::execute_tool(tool_context, name, args),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow!(TOOL_TIMEOUT_ERROR)),
            }
        } else {
            crate::tools::execute_tool(tool_context, name, args).await
        }
    }

    pub(super) fn resolve_tool_timeout(
        &self,
        config: &Config,
        tool_name: &str,
        args: &Value,
    ) -> Option<Duration> {
        let canonical_tool_name = crate::tools::resolve_tool_name(tool_name);
        let _ = canonical_tool_name;
        let mut timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
        if canonical_tool_name == "执行命令" {
            let run_in_background = match args.get("run_in_background") {
                Some(Value::Bool(flag)) => *flag,
                Some(Value::String(text)) => matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                ),
                _ => false,
            };
            if !run_in_background {
                // Blocking mode: the tool waits for the command up to its own
                // timeout_s (default 120s), so the outer budget must outlast it.
                let explicit = args.get("timeout_s").is_some();
                return if explicit && timeout_s <= 0.0 {
                    None
                } else {
                    let effective = if timeout_s > 0.0 { timeout_s } else { 120.0 };
                    Some(Duration::from_secs_f64(
                        (effective + 30.0).max(MIN_TOOL_TIMEOUT_S),
                    ))
                };
            }
            if timeout_s <= 0.0 {
                timeout_s = DEFAULT_TOOL_TIMEOUT_S;
            }
        } else if canonical_tool_name == "命令会话" {
            // A poll may wait up to 60s server-side before responding.
            let yield_s = args
                .get("yield_time_ms")
                .and_then(Value::as_u64)
                .unwrap_or(500) as f64
                / 1000.0;
            timeout_s = timeout_s.max(yield_s.min(60.0) + 30.0);
        } else if tool_name.contains('@') {
            if timeout_s <= 0.0 {
                let fallback = DEFAULT_TOOL_TIMEOUT_S;
                let configured = config.mcp.timeout_s as f64;
                timeout_s = if configured > 0.0 {
                    configured.max(fallback)
                } else {
                    fallback
                };
            }
        } else if timeout_s <= 0.0 {
            let fallback = DEFAULT_TOOL_TIMEOUT_S;
            let sandbox_timeout = if sandbox::sandbox_enabled(config) {
                sandbox::sandbox_timeout_seconds() as f64
            } else {
                0.0
            };
            timeout_s = if sandbox_timeout > 0.0 {
                sandbox_timeout.max(fallback)
            } else {
                fallback
            };
        }
        if timeout_s <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(timeout_s.max(MIN_TOOL_TIMEOUT_S)))
        }
    }
}

fn build_history_attachments(attachments: &[AttachmentPayload]) -> Option<Value> {
    if attachments.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    for attachment in attachments {
        let name = attachment
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let content_type = attachment
            .content_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let public_path = attachment
            .public_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let content_value = attachment
            .content
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| !value.starts_with("data:"))
            .map(ToString::to_string);
        if name.is_none()
            && content_type.is_none()
            && public_path.is_none()
            && content_value.is_none()
        {
            continue;
        }
        let mut entry = Map::new();
        if let Some(name) = name {
            entry.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(content_type) = content_type {
            entry.insert(
                "content_type".to_string(),
                Value::String(content_type.to_string()),
            );
        }
        if let Some(public_path) = public_path {
            entry.insert(
                "public_path".to_string(),
                Value::String(public_path.to_string()),
            );
        }
        if let Some(content_value) = content_value {
            entry.insert("content".to_string(), Value::String(content_value));
        }
        items.push(Value::Object(entry));
    }
    if items.is_empty() {
        None
    } else {
        Some(Value::Array(items))
    }
}

fn extract_file_paths(args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    let Some(obj) = args.as_object() else {
        return paths;
    };
    if let Some(Value::Array(files)) = obj.get("files") {
        for item in files {
            let Some(file_obj) = item.as_object() else {
                continue;
            };
            let path = file_obj
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                paths.push(path);
            }
        }
    }
    if let Some(path) = obj.get("path").and_then(Value::as_str) {
        let cleaned = path.trim();
        if !cleaned.is_empty() {
            paths.push(cleaned.to_string());
        }
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        ordered.push(path);
    }
    ordered
}

#[derive(Clone, Copy)]
enum MarkdownPathWrapper {
    None,
    Angle,
    DoubleQuote,
    SingleQuote,
}

fn markdown_image_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            // Captures Markdown image links: ![alt](target)
            compile_regex(r"!\[([^\]]*)\]\(([^)]+)\)", "markdown_image_link")
        })
        .as_ref()
}

fn split_markdown_target(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let mut in_angle = false;
    for (index, ch) in trimmed.char_indices() {
        if ch == '<' {
            in_angle = true;
            continue;
        }
        if ch == '>' {
            in_angle = false;
            continue;
        }
        if ch.is_whitespace() && !in_angle {
            let path = trimmed[..index].trim().to_string();
            let suffix = trimmed[index..].trim().to_string();
            return (path, suffix);
        }
    }
    (trimmed.to_string(), String::new())
}

fn unwrap_markdown_path_token(token: &str) -> (String, MarkdownPathWrapper) {
    let trimmed = token.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('<') && trimmed.ends_with('>') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::Angle,
        );
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::DoubleQuote,
        );
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::SingleQuote,
        );
    }
    (trimmed.to_string(), MarkdownPathWrapper::None)
}

fn split_url_suffix(raw: &str) -> (String, String) {
    if let Some(index) = raw.find(['?', '#']) {
        return (raw[..index].to_string(), raw[index..].to_string());
    }
    (raw.to_string(), String::new())
}

fn normalize_workspace_markdown_relative_path(raw: &str, workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
    {
        return None;
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        return None;
    }

    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        value = stripped.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspaces/") {
        value = stripped.to_string();
    }
    if !value.is_empty() {
        if value == workspace_id {
            return Some(String::new());
        }
        let workspace_prefix = format!("{workspace_id}/");
        if let Some(stripped) = value.strip_prefix(workspace_prefix.as_str()) {
            value = stripped.to_string();
        } else if value.contains("__c__") || value.contains("__a__") || value.contains("__agent__")
        {
            return None;
        }
    }

    if let Some(stripped) = value.strip_prefix("/workspace/") {
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
    let mut normalized_parts = Vec::new();
    for part in value.split('/') {
        let cleaned = part.trim();
        if cleaned.is_empty() || cleaned == "." {
            continue;
        }
        if cleaned == ".." {
            return None;
        }
        normalized_parts.push(cleaned);
    }
    if normalized_parts.is_empty() {
        return Some(String::new());
    }
    Some(normalized_parts.join("/"))
}

fn workspace_relative_path_exists(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    relative_path: &str,
) -> bool {
    if relative_path.trim().is_empty() {
        return false;
    }
    match workspace.resolve_path(workspace_id, relative_path) {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}

fn is_image_relative_path(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
    )
}

fn collect_existing_artifact_image_paths(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    session_id: &str,
) -> Vec<String> {
    let artifacts = workspace
        .load_artifact_logs(workspace_id, session_id, 200)
        .unwrap_or_default();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for item in artifacts {
        let Some(obj) = item.as_object() else {
            continue;
        };
        if obj.get("kind").and_then(Value::as_str) != Some("file") {
            continue;
        }
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(relative) = normalize_workspace_markdown_relative_path(name, workspace_id) else {
            continue;
        };
        if relative.is_empty() || !is_image_relative_path(relative.as_str()) {
            continue;
        }
        if !workspace_relative_path_exists(workspace, workspace_id, relative.as_str()) {
            continue;
        }
        if seen.insert(relative.clone()) {
            output.push(relative);
        }
    }
    output
}

fn load_directory_image_candidates(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    directory: &str,
) -> Vec<String> {
    let relative_dir = directory.trim_matches('/');
    let Ok((entries, _, _, _, _)) =
        workspace.list_workspace_entries(workspace_id, relative_dir, None, 0, 512, "name", "asc")
    else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for entry in entries {
        if entry.entry_type != "file" || !is_image_relative_path(entry.path.as_str()) {
            continue;
        }
        output.push(entry.path);
    }
    output
}

fn find_missing_image_replacement(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    missing_relative: &str,
    artifact_candidates: &[String],
    dir_candidates: &mut HashMap<String, Vec<String>>,
    used_replacements: &mut HashSet<String>,
) -> Option<String> {
    if missing_relative.trim().is_empty() || !is_image_relative_path(missing_relative) {
        return None;
    }
    let missing_path = Path::new(missing_relative);
    let missing_name = missing_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let missing_dir = missing_path
        .parent()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();

    let candidates = dir_candidates
        .entry(missing_dir.clone())
        .or_insert_with(|| {
            load_directory_image_candidates(workspace, workspace_id, missing_dir.as_str())
        });

    if !missing_name.is_empty() {
        for candidate in candidates.iter() {
            let candidate_name = Path::new(candidate)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if candidate_name == missing_name && used_replacements.insert(candidate.clone()) {
                return Some(candidate.clone());
            }
        }
    }
    for candidate in candidates.iter() {
        if used_replacements.insert(candidate.clone()) {
            return Some(candidate.clone());
        }
    }
    for candidate in artifact_candidates {
        let candidate_dir = Path::new(candidate)
            .parent()
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
            .trim_matches('/')
            .to_string();
        if !missing_dir.is_empty() && candidate_dir != missing_dir {
            continue;
        }
        if used_replacements.insert(candidate.clone()) {
            return Some(candidate.clone());
        }
    }
    None
}

fn format_markdown_path_token(
    original_clean_path: &str,
    replacement_relative: &str,
    workspace_id: &str,
    wrapper: MarkdownPathWrapper,
) -> String {
    let replacement = if original_clean_path.starts_with("/workspaces/") {
        format!(
            "/workspaces/{workspace_id}/{}",
            replacement_relative.trim_matches('/')
        )
    } else if original_clean_path.starts_with('/') {
        format!("/{}", replacement_relative.trim_matches('/'))
    } else if original_clean_path.starts_with("./") {
        format!("./{}", replacement_relative.trim_matches('/'))
    } else {
        replacement_relative.trim_matches('/').to_string()
    };
    match wrapper {
        MarkdownPathWrapper::None => replacement,
        MarkdownPathWrapper::Angle => format!("<{replacement}>"),
        MarkdownPathWrapper::DoubleQuote => format!("\"{replacement}\""),
        MarkdownPathWrapper::SingleQuote => format!("'{replacement}'"),
    }
}

fn normalize_compare_path(path: &Path) -> PathBuf {
    let normalized = normalize_target_path(path);
    normalize_path_for_compare(&normalized)
}

fn resolve_absolute_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Some(path)
    } else {
        let cwd = std::env::current_dir().ok()?;
        Some(cwd.join(path))
    }
}

fn extract_command_lines(args: &Value) -> Vec<String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut commands = Vec::new();
    for line in content.lines() {
        let cleaned = line.trim();
        if !cleaned.is_empty() {
            commands.push(cleaned.to_string());
        }
    }
    commands
}

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_markdown_target_keeps_title_suffix() {
        let (path, suffix) = split_markdown_target("charts/a.png \"title\"");
        assert_eq!(path, "charts/a.png");
        assert_eq!(suffix, "\"title\"");
    }

    #[test]
    fn test_normalize_workspace_markdown_relative_path_handles_workspace_public_path() {
        let normalized = normalize_workspace_markdown_relative_path(
            "/workspaces/alice__c__2/charts/a.png",
            "alice__c__2",
        );
        assert_eq!(normalized.as_deref(), Some("charts/a.png"));
    }

    #[test]
    fn test_format_markdown_path_token_preserves_leading_slash() {
        let formatted = format_markdown_path_token(
            "/charts/wrong.png",
            "charts/right.png",
            "alice__c__2",
            MarkdownPathWrapper::None,
        );
        assert_eq!(formatted, "/charts/right.png");
    }
}
