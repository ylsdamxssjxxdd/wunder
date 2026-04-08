use super::*;

pub(super) const TOOL_TIMEOUT_ERROR: &str = "tool_timeout";

pub(super) struct ToolResultPayload {
    pub(super) ok: bool,
    pub(super) data: Value,
    pub(super) error: String,
    pub(super) sandbox: bool,
    #[allow(dead_code)]
    pub(super) timestamp: DateTime<Utc>,
    pub(super) meta: Option<Value>,
}

impl ToolResultPayload {
    pub(super) fn from_value(value: Value) -> Self {
        let timestamp = Utc::now();
        if let Value::Object(map) = &value {
            if map.get("ok").and_then(Value::as_bool).is_some() && map.contains_key("data") {
                let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
                let mut data = map.get("data").cloned().unwrap_or_else(|| json!({}));
                if let Some(error_meta) = map.get("error_meta").cloned() {
                    if let Some(obj) = data.as_object_mut() {
                        obj.entry("error_meta".to_string()).or_insert(error_meta);
                    }
                }
                let error = map
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let sandbox = map.get("sandbox").and_then(Value::as_bool).unwrap_or(false);
                let meta = map.get("meta").cloned().filter(|value| !value.is_null());
                return Self {
                    ok,
                    data,
                    error,
                    sandbox,
                    timestamp,
                    meta,
                };
            }
        }

        let data = if value.is_object() {
            value
        } else {
            json!({ "result": value })
        };
        Self {
            ok: true,
            data,
            error: String::new(),
            sandbox: false,
            timestamp,
            meta: None,
        }
    }

    pub(super) fn error(message: String, data: Value) -> Self {
        Self {
            ok: false,
            data: if data.is_object() {
                data
            } else {
                json!({ "detail": data })
            },
            error: message,
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        }
    }

    pub(super) fn insert_meta(&mut self, key: &str, value: Value) {
        let mut meta = merge_tool_result_meta(self.meta.take());
        meta.insert(key.to_string(), value);
        self.meta = Some(Value::Object(meta));
    }

    fn to_observation_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
        });
        if !self.error.trim().is_empty() {
            if let Value::Object(ref mut map) = payload {
                map.insert("error".to_string(), Value::String(self.error.clone()));
            }
        }
        if self.sandbox {
            if let Value::Object(ref mut map) = payload {
                map.insert("sandbox".to_string(), Value::Bool(true));
            }
        }
        if let Some(meta) = &self.meta {
            if let Value::Object(ref mut map) = payload {
                if let Some(duration_ms) = meta.get("duration_ms").and_then(Value::as_i64) {
                    if duration_ms > 0 {
                        map.insert("duration_ms".to_string(), json!(duration_ms));
                    }
                }
                if let Some(code) = meta.get("error_code").and_then(Value::as_str) {
                    let cleaned = code.trim();
                    if !cleaned.is_empty() {
                        map.insert("error_code".to_string(), Value::String(cleaned.to_string()));
                    }
                }
                if let Some(retryable) = meta.get("error_retryable").and_then(Value::as_bool) {
                    map.insert("retryable".to_string(), Value::Bool(retryable));
                }
            }
        }
        payload
    }

    fn to_compact_payload(&self, tool_name: &str) -> Value {
        let mut payload = self.to_observation_payload(tool_name);
        compact_observation_payload(&mut payload, tool_name);
        strip_compact_payload_noise(&mut payload, 0);
        payload
    }

    pub(super) fn to_event_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
        });
        if !self.error.trim().is_empty() {
            if let Value::Object(ref mut map) = payload {
                map.insert("error".to_string(), Value::String(self.error.clone()));
            }
        }
        if self.sandbox {
            if let Value::Object(ref mut map) = payload {
                map.insert("sandbox".to_string(), Value::Bool(true));
            }
        }
        if let Some(meta) = &self.meta {
            if let Value::Object(ref mut map) = payload {
                map.insert("meta".to_string(), meta.clone());
                if let Some(code) = meta.get("error_code").and_then(Value::as_str) {
                    let cleaned = code.trim();
                    if !cleaned.is_empty() {
                        map.insert("error_code".to_string(), Value::String(cleaned.to_string()));
                    }
                }
                if let Some(retryable) = meta.get("error_retryable").and_then(Value::as_bool) {
                    map.insert("retryable".to_string(), Value::Bool(retryable));
                }
            }
        }
        payload
    }
}

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
    ) {
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
        if let Err(err) = self.workspace.append_chat(user_id, &payload) {
            warn!("append chat failed for session {session_id} role {role}: {err}");
        }
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
        include_payload: bool,
    ) {
        let timestamp = Local::now().to_rfc3339();
        let safe_args = if args.is_object() {
            args.clone()
        } else {
            json!({ "raw": args })
        };
        let mut payload = json!({
            "tool": tool_name,
            "session_id": session_id,
            "ok": result.ok,
            "error": result.error,
            "args": safe_args,
            "data": result.data,
            "timestamp": timestamp,
        });
        if let Some(meta) = &result.meta {
            payload["meta"] = meta.clone();
        }
        if !include_payload {
            payload["__omit_payload"] = Value::Bool(true);
        }
        if result.sandbox {
            payload["sandbox"] = Value::Bool(true);
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
        let skip_truncation = should_skip_tool_truncation(tool_name);
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
            ("应用补丁", "patch"),
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
        include_payload: bool,
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
            self.append_tool_log(user_id, session_id, &name, args, &result, include_payload);
        }
    }

    pub(super) fn resolve_final_answer(&self, content: &str) -> String {
        if let Some(inner) = extract_tagged_block(content, "final_response") {
            let answer = normalize_final_response_payload(inner.as_str());
            if !answer.is_empty() {
                return answer;
            }
        }
        strip_tool_calls(content).trim().to_string()
    }

    pub(super) fn resolve_final_answer_from_tool(&self, args: &Value) -> String {
        resolve_final_answer_value(args)
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

    pub(super) fn resolve_a2ui_tool_payload(
        &self,
        args: &Value,
        user_id: &str,
        session_id: &str,
    ) -> (String, Option<Value>, String) {
        let (mut uid, content, mut raw_messages) = if let Some(obj) = args.as_object() {
            let uid = obj
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let content = obj
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let raw_messages = obj
                .get("a2ui")
                .cloned()
                .or_else(|| obj.get("messages").cloned())
                .unwrap_or(Value::Null);
            (uid, content, raw_messages)
        } else {
            (String::new(), String::new(), args.clone())
        };
        if uid.trim().is_empty() {
            uid = session_id.trim().to_string();
            if uid.is_empty() {
                uid = user_id.trim().to_string();
            }
        }
        if let Value::String(text) = raw_messages {
            raw_messages = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        }
        if raw_messages.is_object() {
            raw_messages = Value::Array(vec![raw_messages]);
        }
        let Value::Array(items) = raw_messages else {
            return (uid, None, content);
        };
        let mut normalized = Vec::new();
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let mut message = obj.clone();
            for key in [
                "beginRendering",
                "surfaceUpdate",
                "dataModelUpdate",
                "deleteSurface",
            ] {
                if let Some(payload) = message.get(key).and_then(Value::as_object) {
                    if !uid.is_empty() && !payload.contains_key("surfaceId") {
                        let mut payload = payload.clone();
                        payload.insert("surfaceId".to_string(), Value::String(uid.clone()));
                        message.insert(key.to_string(), Value::Object(payload));
                    }
                    break;
                }
            }
            normalized.push(Value::Object(message));
        }
        let messages_payload = if normalized.is_empty() {
            None
        } else {
            Some(Value::Array(normalized))
        };
        (uid, messages_payload, content)
    }

    pub(super) fn log_final_tool_call(
        &self,
        user_id: &str,
        session_id: &str,
        name: &str,
        args: &Value,
        include_payload: bool,
    ) {
        let content = self.resolve_final_answer_from_tool(args);
        let data = if content.trim().is_empty() {
            json!({})
        } else {
            json!({ "content": content })
        };
        let result = ToolResultPayload::from_value(data);
        self.append_tool_log(user_id, session_id, name, args, &result, include_payload);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn log_a2ui_tool_call(
        &self,
        user_id: &str,
        session_id: &str,
        name: &str,
        args: &Value,
        uid: &str,
        messages: &Option<Value>,
        content: &str,
        include_payload: bool,
    ) {
        let message_count = messages
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        let mut data = json!({
            "uid": uid,
            "message_count": message_count,
        });
        if !content.trim().is_empty() {
            if let Value::Object(ref mut map) = data {
                map.insert(
                    "content".to_string(),
                    Value::String(content.trim().to_string()),
                );
            }
        }
        let result = ToolResultPayload::from_value(data);
        self.append_tool_log(user_id, session_id, name, args, &result, include_payload);
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
        if canonical_tool_name == "agent_swarm" {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                action.as_str(),
                "send"
                    | "agent_send"
                    | "agents_send"
                    | "swarm_send"
                    | "batch_send"
                    | "swarm_batch_send"
                    | "agents_batch_send"
                    | "batch"
                    | "fanout"
                    | "dispatch"
                    | "wait"
                    | "join"
                    | "collect"
                    | "swarm_wait"
            ) {
                let explicit_timeout = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
                return if explicit_timeout <= 0.0 {
                    None
                } else {
                    Some(Duration::from_secs_f64(
                        explicit_timeout.max(MIN_TOOL_TIMEOUT_S),
                    ))
                };
            }
        }
        let mut timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
        if tool_name == "a2a等待" {
            let wait_s = parse_timeout_secs(args.get("wait_s")).unwrap_or(0.0);
            if wait_s > 0.0 {
                timeout_s = timeout_s.max(wait_s);
            }
            if timeout_s <= 0.0 {
                timeout_s = config.a2a.timeout_s as f64;
            }
        } else if crate::tools::is_sleep_tool_name(tool_name) {
            if let Some(seconds) = crate::tools::extract_sleep_seconds(args) {
                timeout_s = timeout_s.max(seconds + 10.0);
            }
            if timeout_s <= 0.0 {
                timeout_s = DEFAULT_TOOL_TIMEOUT_S;
            }
        } else if tool_name == "a2a观察" || tool_name.starts_with("a2a@") {
            if timeout_s <= 0.0 {
                timeout_s = config.a2a.timeout_s as f64;
            }
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
                config.sandbox.timeout_s as f64
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

fn extract_tagged_block(content: &str, tag: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(open.as_str())? + open.len();
    let end = lower[start..].find(close.as_str())? + start;
    trimmed
        .get(start..end)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn resolve_final_answer_value(value: &Value) -> String {
    if let Some(obj) = value.as_object() {
        let answer = obj
            .get("content")
            .or_else(|| obj.get("answer"))
            .unwrap_or(&Value::Null);
        return match answer {
            Value::String(text) => strip_tool_calls(text).trim().to_string(),
            Value::Null => String::new(),
            other => strip_tool_calls(
                &serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
            )
            .trim()
            .to_string(),
        };
    }
    if let Some(text) = value.as_str() {
        return strip_tool_calls(text).trim().to_string();
    }
    String::new()
}

fn normalize_final_response_payload(payload: &str) -> String {
    if let Some(content) = extract_tagged_block(payload, "content") {
        return strip_tool_calls(&content).trim().to_string();
    }
    if let Ok(value) = serde_json::from_str::<Value>(payload.trim()) {
        return resolve_final_answer_value(&value);
    }
    strip_tool_calls(payload).trim().to_string()
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

fn merge_tool_result_meta(meta: Option<Value>) -> Map<String, Value> {
    match meta {
        Some(Value::Object(map)) => map,
        Some(Value::Null) | None => Map::new(),
        Some(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    }
}

const OBSERVATION_MAX_CHARS: usize = 20_000;
const OBSERVATION_HEAD_CHARS: usize = 10_000;
const OBSERVATION_TAIL_CHARS: usize = 10_000;
const OBSERVATION_MAX_ARRAY_ITEMS: usize = 32;
const OBSERVATION_ARRAY_HEAD_ITEMS: usize = 20;
const OBSERVATION_ARRAY_TAIL_ITEMS: usize = 8;
const OBSERVATION_TABLE_SAMPLE_ROWS: usize = 4;
const OBSERVATION_SEARCH_HIT_LIMIT: usize = 10;
const OBSERVATION_SEARCH_CONTENT_HEAD_CHARS: usize = 180;
const OBSERVATION_READ_FILE_LIMIT: usize = 8;
const OBSERVATION_READ_CONTENT_HEAD_CHARS: usize = 3200;
const OBSERVATION_JSONL_ITEM_MAX_DEPTH: usize = 8;
const READ_OUTPUT_TRUNCATION_PREFIX: &str = "...(truncated read output, omitted ";
const READ_OUTPUT_TRUNCATION_SUFFIX: &str = " bytes)...";
const TRUNCATION_CONTINUATION_HINT: &str =
    "result_truncated_continue_with_pagination_or_narrower_query";
const CONTINUATION_SIGNAL_KEYS: [&str; 10] = [
    "query_handle",
    "next_cursor",
    "cursor",
    "next_page_token",
    "page_token",
    "continuation_token",
    "next_token",
    "next_url",
    "next_offset",
    "resume_token",
];
const CONTINUATION_NESTED_KEYS: [&str; 7] = [
    "meta",
    "data",
    "result",
    "output",
    "payload",
    "structured_content",
    "pagination",
];

fn value_has_continuation_signal(value: &Value, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    match value {
        Value::Object(map) => {
            if map.get("continuation_required").and_then(Value::as_bool) == Some(true)
                || map.get("has_more").and_then(Value::as_bool) == Some(true)
            {
                return true;
            }
            if CONTINUATION_SIGNAL_KEYS
                .iter()
                .any(|key| map.get(*key).is_some_and(is_non_empty_continuation_value))
            {
                return true;
            }
            CONTINUATION_NESTED_KEYS.iter().any(|key| {
                map.get(*key)
                    .is_some_and(|nested| value_has_continuation_signal(nested, depth + 1))
            })
        }
        Value::Array(items) => items
            .iter()
            .take(6)
            .any(|item| value_has_continuation_signal(item, depth + 1)),
        _ => false,
    }
}

fn is_non_empty_continuation_value(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Number(_) => true,
        Value::Bool(flag) => *flag,
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::Null => false,
    }
}

fn map_has_continuation_signal(map: &Map<String, Value>) -> bool {
    if map.get("continuation_required").and_then(Value::as_bool) == Some(true)
        || map.get("has_more").and_then(Value::as_bool) == Some(true)
    {
        return true;
    }
    CONTINUATION_SIGNAL_KEYS
        .iter()
        .any(|key| map.get(*key).is_some_and(is_non_empty_continuation_value))
}

fn looks_like_read_file_payload(data: &Value) -> bool {
    let Some(meta) = data.get("meta").and_then(Value::as_object) else {
        return false;
    };
    data.get("content").and_then(Value::as_str).is_some()
        && meta.get("files").and_then(Value::as_array).is_some()
        && meta.get("read").and_then(Value::as_object).is_some()
}

fn supports_tool_result_continuation(data: &Value, meta: Option<&Value>) -> bool {
    value_has_continuation_signal(data, 0)
        || meta.is_some_and(|value| value_has_continuation_signal(value, 0))
        || looks_like_read_file_payload(data)
}

fn should_skip_tool_truncation(tool_name: &str) -> bool {
    matches!(tool_name, "技能调用" | "skill_call" | "skill_get")
}

fn compact_observation_payload(payload: &mut Value, tool_name: &str) {
    if should_skip_tool_truncation(tool_name) {
        return;
    }
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    let Some(raw_data) = map.get("data").cloned() else {
        return;
    };
    let continuation_supported = supports_tool_result_continuation(&raw_data, None);
    let mut compacted_data = extract_observation_data(&raw_data);
    compact_tabular_observation_data(&mut compacted_data);
    compact_dense_arrays_to_jsonl(&mut compacted_data);
    let mut observation_truncated = truncate_observation_data(
        &mut compacted_data,
        OBSERVATION_HEAD_CHARS,
        OBSERVATION_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    let mut truncation_reasons = if observation_truncated {
        collect_truncation_reasons_from_value(&compacted_data, TOOL_RESULT_TRUNCATION_MARKER)
    } else {
        Vec::new()
    };
    let chars_before_compact = estimate_tool_result_chars(&compacted_data);
    if chars_before_compact > OBSERVATION_MAX_CHARS {
        append_truncation_reason(&mut truncation_reasons, "char_budget");
        compacted_data = compact_large_tool_result_data(
            &compacted_data,
            chars_before_compact,
            OBSERVATION_HEAD_CHARS,
            OBSERVATION_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
            continuation_supported,
            &truncation_reasons,
        );
        observation_truncated = true;
    }
    let observation_output_chars = estimate_tool_result_chars(&compacted_data);
    let data_continuation_required = compacted_data
        .get("continuation_required")
        .and_then(Value::as_bool)
        == Some(true);
    let data_continuation_hint = compacted_data
        .get("continuation_hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    map.insert("data".to_string(), compacted_data);
    if observation_truncated {
        map.insert("truncated".to_string(), Value::Bool(true));
        map.insert(
            "observation_output_chars".to_string(),
            json!(observation_output_chars),
        );
        if !truncation_reasons.is_empty() {
            map.insert(
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
        if continuation_supported {
            map.insert("continuation_required".to_string(), Value::Bool(true));
            map.insert(
                "continuation_hint".to_string(),
                Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
            );
        }
    }
    if data_continuation_required {
        map.insert("continuation_required".to_string(), Value::Bool(true));
        map.insert(
            "continuation_hint".to_string(),
            Value::String(
                data_continuation_hint.unwrap_or_else(|| TRUNCATION_CONTINUATION_HINT.to_string()),
            ),
        );
    }
    map.remove("meta");
}

fn compact_dense_arrays_to_jsonl(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if key.ends_with("_jsonl") || key.ends_with("_count") || key == "truncation_reasons"
                {
                    continue;
                }
                let lines = map.get(&key).and_then(Value::as_array).map(|items| {
                    items
                        .iter()
                        .map(|item| compact_jsonl_item_for_model(item, key.as_str(), 0))
                        .map(|item| value_to_jsonl_line(&item))
                        .collect::<Vec<_>>()
                });
                let Some(lines) = lines else {
                    continue;
                };
                map.insert(format!("{key}_count"), json!(lines.len()));
                map.insert(format!("{key}_jsonl"), Value::String(lines.join("\n")));
                map.remove(&key);
            }
            for nested in map.values_mut() {
                compact_dense_arrays_to_jsonl(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                compact_dense_arrays_to_jsonl(item);
            }
        }
        _ => {}
    }
}

fn compact_jsonl_item_for_model(value: &Value, parent_key: &str, depth: usize) -> Value {
    if depth >= OBSERVATION_JSONL_ITEM_MAX_DEPTH {
        return value.clone();
    }
    match value {
        Value::Object(map) => {
            if parent_key == "results" {
                if let Some(compacted) = compact_execute_command_result_item(map) {
                    return compacted;
                }
            }
            let mut compacted = Map::new();
            for (key, nested_value) in map {
                if should_drop_jsonl_observation_key(key) {
                    continue;
                }
                let nested = compact_jsonl_item_for_model(nested_value, key, depth + 1);
                if is_empty_observation_value(&nested) {
                    continue;
                }
                compacted.insert(key.clone(), nested);
            }
            Value::Object(compacted)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| compact_jsonl_item_for_model(item, parent_key, depth + 1))
                .filter(|item| !is_empty_observation_value(item))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn compact_execute_command_result_item(map: &Map<String, Value>) -> Option<Value> {
    if !map.contains_key("command") {
        return None;
    }
    let has_exec_result = map.contains_key("returncode")
        || map.get("stdout").and_then(Value::as_str).is_some()
        || map.get("stderr").and_then(Value::as_str).is_some();
    if !has_exec_result {
        return None;
    }
    let mut compacted = Map::new();
    if let Some(command) = map.get("command").cloned() {
        compacted.insert("command".to_string(), command);
    }
    if let Some(returncode) = map.get("returncode").cloned() {
        compacted.insert("returncode".to_string(), returncode);
    }
    if let Some(stdout) = map.get("stdout").and_then(Value::as_str) {
        if !stdout.trim().is_empty() {
            compacted.insert("stdout".to_string(), Value::String(stdout.to_string()));
        }
    }
    if let Some(stderr) = map.get("stderr").and_then(Value::as_str) {
        if !stderr.trim().is_empty() {
            compacted.insert("stderr".to_string(), Value::String(stderr.to_string()));
        }
    }
    if compacted.is_empty() {
        None
    } else {
        Some(Value::Object(compacted))
    }
}

fn should_drop_jsonl_observation_key(key: &str) -> bool {
    if key.ends_with("_session_id") || key.ends_with("_round") {
        return true;
    }
    if key.ends_with("_meta") && key != "error_meta" {
        return true;
    }
    matches!(
        key,
        "meta"
            | "tool_call_id"
            | "trace_id"
            | "timestamp"
            | "log_profile"
            | "transport_ok"
            | "business_ok"
            | "final_ok"
            | "command_index"
            | "output_meta"
            | "elapsed_ms"
            | "duration_ms"
            | "latency_ms"
            | "timing"
            | "timings"
            | "stats"
            | "metrics"
            | "performance"
            | "perf"
    )
}

fn is_empty_observation_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn value_to_jsonl_line(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(text) => text.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(num) => num.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

fn strip_compact_payload_noise(value: &mut Value, depth: usize) {
    if depth > 8 {
        return;
    }
    let Value::Object(map) = value else {
        return;
    };
    for key in [
        "meta",
        "tool_call_id",
        "trace_id",
        "model_round",
        "user_round",
        "timestamp",
        "log_profile",
        "business_ok",
        "final_ok",
        "transport_ok",
    ] {
        map.remove(key);
    }
    if let Some(data) = map.get_mut("data").and_then(Value::as_object_mut) {
        for key in [
            "meta",
            "budget",
            "scope",
            "scope_note",
            "resolved_path",
            "query_mode_inferred",
            "query_source",
            "query_used",
            "scanned_files",
            "file_pattern_items",
            "case_sensitive",
            "context_after",
            "context_before",
            "engine",
            "path",
        ] {
            data.remove(key);
        }
        for nested in data.values_mut() {
            strip_compact_payload_noise(nested, depth + 1);
        }
    }
    for nested in map.values_mut() {
        strip_compact_payload_noise(nested, depth + 1);
    }
}

fn extract_observation_data(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    if map.get("truncated").and_then(Value::as_bool) == Some(true) && map.contains_key("preview") {
        return compact_truncated_observation_wrapper(map);
    }
    if let Some(compacted_search) = compact_search_observation_data(map) {
        return compacted_search;
    }
    if let Some(compacted_read) = compact_read_file_observation_data(map) {
        return compacted_read;
    }
    if let Some(structured_content) = map.get("structured_content") {
        if !structured_content.is_null() {
            return structured_content.clone();
        }
    }
    if let Some(parsed) = parse_json_from_content_text_blocks(value) {
        return parsed;
    }
    if let Some(content) = map.get("content").filter(|item| !item.is_null()) {
        return content.clone();
    }
    value.clone()
}

fn compact_search_observation_data(map: &Map<String, Value>) -> Option<Value> {
    if !map.contains_key("hits") || !map.contains_key("query") {
        return None;
    }
    let mut compacted = Map::new();
    for key in [
        "query",
        "query_mode",
        "path",
        "strategy",
        "returned_match_count",
        "matched_file_count",
        "timeout_hit",
        "match_limit_hit",
        "file_limit_hit",
    ] {
        if let Some(value) = map.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }
    if let Some(summary) = map.get("summary").and_then(Value::as_object) {
        let mut summary_compacted = Map::new();
        for key in ["focus_points", "matched_terms", "top_files", "next_hint"] {
            if let Some(value) = summary.get(key) {
                summary_compacted.insert(key.to_string(), value.clone());
            }
        }
        if !summary_compacted.is_empty() {
            compacted.insert("summary".to_string(), Value::Object(summary_compacted));
        }
    }
    if let Some(hits) = map.get("hits").and_then(Value::as_array) {
        let mut compacted_hits = Vec::new();
        for hit in hits.iter().take(OBSERVATION_SEARCH_HIT_LIMIT) {
            let Some(hit_obj) = hit.as_object() else {
                continue;
            };
            let mut item = Map::new();
            for key in ["path", "line", "matched_terms"] {
                if let Some(value) = hit_obj.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            if let Some(content) = hit_obj.get("content").and_then(Value::as_str) {
                let (content_head, omitted_chars) =
                    truncate_text_head(content, OBSERVATION_SEARCH_CONTENT_HEAD_CHARS);
                if !content_head.trim().is_empty() {
                    item.insert("content_head".to_string(), Value::String(content_head));
                }
                if omitted_chars > 0 {
                    item.insert("content_omitted_chars".to_string(), json!(omitted_chars));
                }
            }
            if !item.is_empty() {
                compacted_hits.push(Value::Object(item));
            }
        }
        if hits.len() > OBSERVATION_SEARCH_HIT_LIMIT {
            compacted_hits.push(build_omitted_items_marker(
                hits.len().saturating_sub(OBSERVATION_SEARCH_HIT_LIMIT),
            ));
        }
        if !compacted_hits.is_empty() {
            compacted.insert("hits".to_string(), Value::Array(compacted_hits));
        }
    }
    if let Some(matches) = map.get("matches").and_then(Value::as_array) {
        let mut compacted_matches = matches
            .iter()
            .take(OBSERVATION_SEARCH_HIT_LIMIT)
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() > OBSERVATION_SEARCH_HIT_LIMIT {
            compacted_matches.push(build_omitted_items_marker(
                matches.len().saturating_sub(OBSERVATION_SEARCH_HIT_LIMIT),
            ));
        }
        if !compacted_matches.is_empty() {
            compacted.insert("matches".to_string(), Value::Array(compacted_matches));
        }
    }
    Some(Value::Object(compacted))
}

fn compact_read_file_observation_data(map: &Map<String, Value>) -> Option<Value> {
    let content = map.get("content").and_then(Value::as_str)?;
    let mut compacted = Map::new();
    let (clean_content, read_output_omitted_bytes) = strip_read_output_truncation_notice(content);
    let (content_head, content_omitted_chars) =
        truncate_text_head(&clean_content, OBSERVATION_READ_CONTENT_HEAD_CHARS);
    compacted.insert("content".to_string(), Value::String(content_head));
    if content_omitted_chars > 0 {
        compacted.insert(
            "content_omitted_chars".to_string(),
            json!(content_omitted_chars),
        );
        compacted.insert("continuation_required".to_string(), Value::Bool(true));
        compacted.insert(
            "continuation_hint".to_string(),
            Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
        );
    }
    if let Some(omitted_bytes) = read_output_omitted_bytes {
        compacted.insert(
            "read_output_omitted_bytes".to_string(),
            json!(omitted_bytes),
        );
        compacted.insert("continuation_required".to_string(), Value::Bool(true));
        compacted.insert(
            "continuation_hint".to_string(),
            Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
        );
    }
    let files = map
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("files"))
        .and_then(Value::as_array);
    if let Some(files) = files {
        let mut file_entries = Vec::new();
        for file in files.iter().take(OBSERVATION_READ_FILE_LIMIT) {
            let Some(file_obj) = file.as_object() else {
                continue;
            };
            let mut item = Map::new();
            for key in ["path", "read_lines", "total_lines", "complete"] {
                if let Some(value) = file_obj.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            if !item.is_empty() {
                file_entries.push(Value::Object(item));
            }
        }
        if files.len() > OBSERVATION_READ_FILE_LIMIT {
            file_entries.push(build_omitted_items_marker(
                files.len().saturating_sub(OBSERVATION_READ_FILE_LIMIT),
            ));
        }
        if !file_entries.is_empty() {
            compacted.insert("files".to_string(), Value::Array(file_entries));
        }
    }
    Some(Value::Object(compacted))
}

fn strip_read_output_truncation_notice(content: &str) -> (String, Option<u64>) {
    let Some(start) = content.rfind(READ_OUTPUT_TRUNCATION_PREFIX) else {
        return (content.to_string(), None);
    };
    let notice = &content[start..];
    if !notice.ends_with(READ_OUTPUT_TRUNCATION_SUFFIX) {
        return (content.to_string(), None);
    }
    let number_start = READ_OUTPUT_TRUNCATION_PREFIX.len();
    let number_end = notice
        .len()
        .saturating_sub(READ_OUTPUT_TRUNCATION_SUFFIX.len());
    let omitted = notice
        .get(number_start..number_end)
        .and_then(|value| value.trim().parse::<u64>().ok());
    let mut cleaned = content[..start].trim_end_matches('\n').to_string();
    if cleaned.is_empty() {
        cleaned = content.to_string();
    }
    (cleaned, omitted)
}

fn build_omitted_items_marker(omitted_items: usize) -> Value {
    json!({
        "__truncated": true,
        "omitted_items": omitted_items,
    })
}

fn compact_tabular_observation_data(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };
    let Some(rows) = map.get_mut("rows").and_then(Value::as_array_mut) else {
        return;
    };
    if rows.len() <= OBSERVATION_TABLE_SAMPLE_ROWS {
        return;
    }
    let original_len = rows.len();
    rows.truncate(OBSERVATION_TABLE_SAMPLE_ROWS);
    // Keep a tiny head sample only; large tabular payloads should stay in tools/files,
    // not be replayed through the model context page by page.
    map.insert(
        "rows_sampled".to_string(),
        json!(OBSERVATION_TABLE_SAMPLE_ROWS.min(original_len)),
    );
    map.insert(
        "rows_omitted".to_string(),
        json!(original_len.saturating_sub(OBSERVATION_TABLE_SAMPLE_ROWS)),
    );
}

fn compact_truncated_observation_wrapper(map: &Map<String, Value>) -> Value {
    let mut compacted = Map::new();
    for key in [
        "truncated",
        "original_chars",
        "truncation_reasons",
        "continuation_required",
        "continuation_hint",
        "exit_code",
    ] {
        if let Some(value) = map.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }
    if let Some(preview) = map.get("preview").and_then(Value::as_str) {
        // Keep wrapper previews close to the observation budget so the model
        // still sees the overall shape of a large tool result.
        let preview_budget_chars = OBSERVATION_MAX_CHARS.saturating_sub(
            estimate_tool_result_chars(&Value::Object(compacted.clone()))
                .saturating_add("preview".chars().count()),
        );
        compacted.insert(
            "preview".to_string(),
            Value::String(truncate_tool_result_string_to_budget(
                preview,
                preview_budget_chars,
                TOOL_RESULT_TRUNCATION_MARKER,
            )),
        );
    }
    Value::Object(compacted)
}

fn parse_json_from_content_text_blocks(value: &Value) -> Option<Value> {
    let content = value.get("content")?.as_array()?;
    if content.len() != 1 {
        return None;
    }
    let block = content.first()?.as_object()?;
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    let text = block.get("text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(text).ok()
}

fn truncate_tool_result_string(
    value: &str,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> String {
    let value_len = value.chars().count();
    if value_len <= head_chars + tail_chars {
        return value.to_string();
    }
    let head_chars = head_chars.min(value_len);
    let tail_chars = tail_chars.min(value_len.saturating_sub(head_chars));
    let mut output = String::new();
    output.extend(value.chars().take(head_chars));
    output.push_str(marker);
    if tail_chars > 0 {
        output.extend(value.chars().skip(value_len - tail_chars).take(tail_chars));
    }
    output
}

fn truncate_tool_result_string_to_budget(value: &str, budget_chars: usize, marker: &str) -> String {
    let value_len = value.chars().count();
    if value_len <= budget_chars {
        return value.to_string();
    }
    let marker_chars = marker.chars().count();
    if budget_chars <= marker_chars {
        return marker.chars().take(budget_chars).collect();
    }
    let visible_chars = budget_chars.saturating_sub(marker_chars);
    let head_chars = visible_chars / 2;
    let tail_chars = visible_chars.saturating_sub(head_chars);
    truncate_tool_result_string(value, head_chars, tail_chars, marker)
}

fn truncate_text_head(value: &str, max_chars: usize) -> (String, usize) {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return (value.to_string(), 0);
    }
    let head = chars.iter().take(max_chars).collect::<String>();
    (head, chars.len().saturating_sub(max_chars))
}

fn truncate_tool_result_data(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> bool {
    truncate_tool_result_data_with_array_limits(
        value,
        head_chars,
        tail_chars,
        marker,
        TOOL_RESULT_MAX_ARRAY_ITEMS,
        TOOL_RESULT_ARRAY_HEAD_ITEMS,
        TOOL_RESULT_ARRAY_TAIL_ITEMS,
    )
}

fn truncate_tool_result_data_with_array_limits(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
    max_array_items: usize,
    head_array_items: usize,
    tail_array_items: usize,
) -> bool {
    match value {
        Value::String(text) => {
            if text.chars().count() > head_chars + tail_chars {
                *text = truncate_tool_result_string(text, head_chars, tail_chars, marker);
                true
            } else {
                false
            }
        }
        Value::Array(items) => {
            let mut truncated = false;
            if items.len() > max_array_items {
                let original_len = items.len();
                let head_items = head_array_items.min(original_len);
                let tail_items = tail_array_items.min(original_len - head_items);
                let omitted = original_len.saturating_sub(head_items + tail_items);
                let mut compacted = Vec::with_capacity(head_items + tail_items + 1);
                compacted.extend(items.iter().take(head_items).cloned());
                compacted.push(build_omitted_items_marker(omitted));
                if tail_items > 0 {
                    compacted.extend(items.iter().skip(original_len - tail_items).cloned());
                }
                *items = compacted;
                truncated = true;
            }
            for item in items.iter_mut() {
                if truncate_tool_result_data_with_array_limits(
                    item,
                    head_chars,
                    tail_chars,
                    marker,
                    max_array_items,
                    head_array_items,
                    tail_array_items,
                ) {
                    truncated = true;
                }
            }
            truncated
        }
        Value::Object(map) => {
            let mut truncated = false;
            let (next_max_items, next_head_items, next_tail_items) =
                if map_has_continuation_signal(map) {
                    (
                        TOOL_RESULT_PAGINATED_MAX_ARRAY_ITEMS,
                        TOOL_RESULT_PAGINATED_ARRAY_HEAD_ITEMS,
                        TOOL_RESULT_PAGINATED_ARRAY_TAIL_ITEMS,
                    )
                } else {
                    (max_array_items, head_array_items, tail_array_items)
                };
            for value in map.values_mut() {
                if truncate_tool_result_data_with_array_limits(
                    value,
                    head_chars,
                    tail_chars,
                    marker,
                    next_max_items,
                    next_head_items,
                    next_tail_items,
                ) {
                    truncated = true;
                }
            }
            truncated
        }
        _ => false,
    }
}

fn truncate_observation_data(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> bool {
    match value {
        Value::String(text) => {
            if text.chars().count() > head_chars + tail_chars {
                *text = truncate_tool_result_string(text, head_chars, tail_chars, marker);
                true
            } else {
                false
            }
        }
        Value::Array(items) => {
            let mut truncated = false;
            if items.len() > OBSERVATION_MAX_ARRAY_ITEMS {
                let original_len = items.len();
                let head_items = OBSERVATION_ARRAY_HEAD_ITEMS.min(original_len);
                let tail_items = OBSERVATION_ARRAY_TAIL_ITEMS.min(original_len - head_items);
                let omitted = original_len.saturating_sub(head_items + tail_items);
                let mut compacted = Vec::with_capacity(head_items + tail_items + 1);
                compacted.extend(items.iter().take(head_items).cloned());
                compacted.push(build_omitted_items_marker(omitted));
                if tail_items > 0 {
                    compacted.extend(items.iter().skip(original_len - tail_items).cloned());
                }
                *items = compacted;
                truncated = true;
            }
            for item in items.iter_mut() {
                if truncate_observation_data(item, head_chars, tail_chars, marker) {
                    truncated = true;
                }
            }
            truncated
        }
        Value::Object(map) => {
            let mut truncated = false;
            for inner in map.values_mut() {
                if truncate_observation_data(inner, head_chars, tail_chars, marker) {
                    truncated = true;
                }
            }
            truncated
        }
        _ => false,
    }
}

fn compact_large_tool_result_data(
    value: &Value,
    original_chars: usize,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
    continuation_supported: bool,
    truncation_reasons: &[String],
) -> Value {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    let preview = truncate_tool_result_string(&serialized, head_chars, tail_chars, marker);
    let mut payload = json!({
        "truncated": true,
        "original_chars": original_chars,
        "preview": preview,
    });
    if !truncation_reasons.is_empty() {
        if let Value::Object(ref mut map) = payload {
            map.insert(
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
    }
    if continuation_supported {
        if let Value::Object(ref mut map) = payload {
            map.insert("continuation_required".to_string(), Value::Bool(true));
            map.insert(
                "continuation_hint".to_string(),
                Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
            );
        }
    }
    if let Some(exit_code) = extract_exit_code(value) {
        if let Value::Object(ref mut map) = payload {
            map.insert("exit_code".to_string(), json!(exit_code));
        }
    }
    payload
}

fn append_truncation_reason(reasons: &mut Vec<String>, reason: &str) {
    if reasons.iter().any(|item| item == reason) {
        return;
    }
    reasons.push(reason.to_string());
}

fn dedupe_truncation_reasons(reasons: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(reasons.len());
    for reason in reasons.iter() {
        if deduped.iter().any(|item: &String| item == reason) {
            continue;
        }
        deduped.push(reason.clone());
    }
    *reasons = deduped;
}

fn collect_truncation_reasons_from_value(value: &Value, marker: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    if value_contains_omitted_items_marker(value) {
        append_truncation_reason(&mut reasons, "array_items");
    }
    if value_contains_string_truncation_marker(value, marker) {
        append_truncation_reason(&mut reasons, "string_chars");
    }
    reasons
}

fn value_contains_omitted_items_marker(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("__truncated").and_then(Value::as_bool) == Some(true)
                && map
                    .get("omitted_items")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    > 0
            {
                return true;
            }
            map.values().any(value_contains_omitted_items_marker)
        }
        Value::Array(items) => items.iter().any(value_contains_omitted_items_marker),
        _ => false,
    }
}

fn value_contains_string_truncation_marker(value: &Value, marker: &str) -> bool {
    match value {
        Value::String(text) => text.contains(marker),
        Value::Array(items) => items
            .iter()
            .any(|item| value_contains_string_truncation_marker(item, marker)),
        Value::Object(map) => map
            .values()
            .any(|item| value_contains_string_truncation_marker(item, marker)),
        _ => false,
    }
}

fn estimate_tool_result_chars(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Number(num) => num.to_string().chars().count(),
        Value::Bool(flag) => {
            if *flag {
                4
            } else {
                5
            }
        }
        Value::Null => 4,
        Value::Array(items) => items.iter().map(estimate_tool_result_chars).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| key.chars().count() + estimate_tool_result_chars(value))
            .sum(),
    }
}

fn parse_exit_code(value: &Value) -> Option<i64> {
    match value {
        Value::Number(num) => num.as_i64().or_else(|| num.as_u64().map(|val| val as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn extract_exit_code(value: &Value) -> Option<i64> {
    let obj = value.as_object()?;
    for key in [
        "exit_code",
        "exitCode",
        "returncode",
        "return_code",
        "status_code",
    ] {
        if let Some(code) = obj.get(key).and_then(parse_exit_code) {
            return Some(code);
        }
    }
    if let Some(Value::Array(items)) = obj.get("results") {
        for item in items {
            let Some(result) = item.as_object() else {
                continue;
            };
            for key in [
                "exit_code",
                "exitCode",
                "returncode",
                "return_code",
                "status_code",
            ] {
                if let Some(code) = result.get(key).and_then(parse_exit_code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_tool_result_string() {
        let head_chars = 2;
        let tail_chars = 3;
        let input = "abcdefghijklmnopqrstuvwxyz";
        let value = truncate_tool_result_string(
            input,
            head_chars,
            tail_chars,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(value.starts_with("ab"));
        assert!(value.ends_with("xyz"));
        assert!(value.contains(TOOL_RESULT_TRUNCATION_MARKER));
        assert_eq!(
            value.chars().count(),
            head_chars + tail_chars + TOOL_RESULT_TRUNCATION_MARKER.chars().count()
        );
    }

    #[test]
    fn test_truncate_tool_result_data() {
        let head_chars = 1;
        let tail_chars = 2;
        let stdout = "0123456789";
        let mut value = json!({ "stdout": stdout });
        let truncated = truncate_tool_result_data(
            &mut value,
            head_chars,
            tail_chars,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(truncated);
        let stdout = value.get("stdout").and_then(Value::as_str).unwrap_or("");
        assert!(stdout.starts_with("0"));
        assert!(stdout.ends_with("89"));
        assert!(stdout.contains(TOOL_RESULT_TRUNCATION_MARKER));
    }

    #[test]
    fn test_truncate_tool_result_data_limits_large_arrays() {
        let mut rows = Vec::new();
        for idx in 0..200 {
            rows.push(json!({ "id": idx }));
        }
        let mut value = json!({ "rows": rows });
        let truncated = truncate_tool_result_data(
            &mut value,
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(truncated);
        let rows = value
            .get("rows")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(rows.len() <= TOOL_RESULT_MAX_ARRAY_ITEMS + 1);
        let has_marker = rows.iter().any(|item| {
            item.get("omitted_items")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
                && item.get("__truncated").and_then(Value::as_bool) == Some(true)
        });
        assert!(has_marker);
    }

    #[test]
    fn test_truncate_tool_result_data_keeps_paginated_arrays_under_500() {
        let mut items = Vec::new();
        for idx in 0..200 {
            items.push(json!(format!("file-{idx}.md")));
        }
        let mut value = json!({
            "items": items,
            "cursor": "0",
            "limit": 200,
            "next_cursor": "200",
            "has_more": true
        });
        let truncated = truncate_tool_result_data(
            &mut value,
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(!truncated);
        let rows = value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(rows.len(), 200);
        let has_marker = rows
            .iter()
            .any(|item| item.get("__truncated").and_then(Value::as_bool) == Some(true));
        assert!(!has_marker);
    }

    #[test]
    fn test_truncate_tool_result_data_keeps_final_paginated_page_under_500() {
        let mut items = Vec::new();
        for idx in 0..74 {
            items.push(json!(format!("file-{idx}.md")));
        }
        let mut value = json!({
            "items": items,
            "cursor": "0",
            "limit": 200,
            "has_more": false,
            "next_cursor": null
        });
        let truncated = truncate_tool_result_data(
            &mut value,
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(!truncated);
        let rows = value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(rows.len(), 74);
        let has_marker = rows
            .iter()
            .any(|item| item.get("__truncated").and_then(Value::as_bool) == Some(true));
        assert!(!has_marker);
    }

    #[test]
    fn test_collect_truncation_reasons_from_value_detects_array_and_string() {
        let value = json!({
            "items": [
                "a",
                {"__truncated": true, "omitted_items": 10}
            ],
            "preview": format!("head{}tail", TOOL_RESULT_TRUNCATION_MARKER)
        });
        let reasons = collect_truncation_reasons_from_value(&value, TOOL_RESULT_TRUNCATION_MARKER);
        assert!(reasons.iter().any(|item| item == "array_items"));
        assert!(reasons.iter().any(|item| item == "string_chars"));
    }

    #[test]
    fn test_observation_payload_is_compact_for_model_context() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({"items": ["a", "b"]}),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "normalized_transport_ok": true,
                "normalized_business_ok": true,
                "normalized_final_ok": true,
                "error_retryable": false,
                "duration_ms": 12,
                "truncated": true,
                "continuation_required": true,
                "continuation_hint": TRUNCATION_CONTINUATION_HINT,
            })),
        };

        let observation = payload.to_compact_payload("list_files");
        assert!(observation.get("timestamp").is_none());
        assert!(observation.get("final_ok").is_none());
        assert!(observation.get("transport_ok").is_none());
        assert!(observation.get("business_ok").is_none());
        assert!(observation.get("meta").is_none());
        let data = observation
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert!(data.get("items").is_none());
        assert!(data.get("items_jsonl").is_some());
    }

    #[test]
    fn test_event_payload_keeps_meta_for_frontend() {
        let payload = ToolResultPayload {
            ok: false,
            data: json!({"error": "boom"}),
            error: "boom".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "normalized_transport_ok": true,
                "normalized_business_ok": false,
                "normalized_final_ok": false,
                "error_retryable": false,
                "error_code": "TOOL_BUSINESS_FAILED"
            })),
        };

        let event_payload = payload.to_event_payload("demo_tool");
        assert!(event_payload.get("timestamp").is_none());
        assert!(event_payload.get("meta").is_some());
        assert!(event_payload.get("final_ok").is_none());
        assert_eq!(
            event_payload.get("error_code").and_then(Value::as_str),
            Some("TOOL_BUSINESS_FAILED")
        );
    }

    #[test]
    fn test_observation_payload_keeps_duration_for_workflow_display() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({"ok": true}),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "duration_ms": 1280,
                "error_retryable": false
            })),
        };

        let compacted = payload.to_compact_payload("demo_tool");
        assert_eq!(
            compacted.get("duration_ms").and_then(Value::as_i64),
            Some(1280)
        );
    }

    #[test]
    fn test_compact_payload_strips_ids_and_budget_noise() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "query": "九段线",
                "path": "kb",
                "budget": {"max_matches": 200},
                "scope": {"kind": "workspace_local"},
                "scope_note": "local only",
                "meta": {"search": {"elapsed_ms": 20}},
                "hits": [{"path": "a.md", "line": 1, "content": "九段线"}],
                "matches": ["a.md:1:九段线"]
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "error_retryable": false,
            })),
        };

        let mut compacted = payload.to_compact_payload("search_content");
        if let Value::Object(ref mut map) = compacted {
            map.insert(
                "tool_call_id".to_string(),
                Value::String("call_x".to_string()),
            );
            map.insert("trace_id".to_string(), Value::String("trace_x".to_string()));
            map.insert("model_round".to_string(), json!(2));
            map.insert("user_round".to_string(), json!(1));
        }
        strip_compact_payload_noise(&mut compacted, 0);

        let obj = compacted.as_object().cloned().unwrap_or_default();
        assert!(obj.get("tool_call_id").is_none());
        assert!(obj.get("trace_id").is_none());
        assert!(obj.get("model_round").is_none());
        assert!(obj.get("user_round").is_none());

        let data = obj
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert!(data.get("meta").is_none());
        assert!(data.get("budget").is_none());
        assert!(data.get("scope").is_none());
        assert!(data.get("scope_note").is_none());
        assert!(data.get("hits_jsonl").is_some());
        assert!(data.get("matches_jsonl").is_some());
    }

    #[test]
    fn test_compact_large_tool_result_data_includes_preview() {
        let mut rows = Vec::new();
        for idx in 0..160 {
            rows.push(json!({
                "id": idx,
                "text": format!("row-{idx:03}-{}", "x".repeat(64)),
            }));
        }
        let value = json!({ "rows": rows });
        let chars = estimate_tool_result_chars(&value);
        let compacted = compact_large_tool_result_data(
            &value,
            chars,
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
            true,
            &["char_budget".to_string()],
        );
        assert_eq!(
            compacted.get("truncated").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            compacted
                .get("original_chars")
                .and_then(Value::as_u64)
                .unwrap_or_default() as usize,
            chars
        );
        let preview = compacted
            .get("preview")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(!preview.is_empty());
        let reasons = compacted
            .get("truncation_reasons")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(reasons
            .iter()
            .any(|item| item.as_str() == Some("char_budget")));
        assert_eq!(
            compacted
                .get("continuation_required")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            compacted
                .get("continuation_hint")
                .and_then(Value::as_str)
                .unwrap_or(""),
            TRUNCATION_CONTINUATION_HINT
        );
    }

    #[test]
    fn test_compact_large_tool_result_data_keeps_combined_truncation_reasons() {
        let items = (0..900)
            .map(|idx| json!(format!("file-{idx}.md")))
            .collect::<Vec<_>>();
        let mut value = json!({
            "items": items,
            "cursor": "0",
            "next_cursor": "900",
            "has_more": true
        });
        let truncated = truncate_tool_result_data(
            &mut value,
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
        );
        assert!(truncated);

        let mut reasons =
            collect_truncation_reasons_from_value(&value, TOOL_RESULT_TRUNCATION_MARKER);
        append_truncation_reason(&mut reasons, "char_budget");
        dedupe_truncation_reasons(&mut reasons);

        let compacted = compact_large_tool_result_data(
            &value,
            estimate_tool_result_chars(&value),
            TOOL_RESULT_HEAD_CHARS,
            TOOL_RESULT_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
            true,
            &reasons,
        );
        let reasons = compacted
            .get("truncation_reasons")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(reasons
            .iter()
            .any(|item| item.as_str() == Some("array_items")));
        assert!(reasons
            .iter()
            .any(|item| item.as_str() == Some("char_budget")));
    }

    #[test]
    fn test_compact_observation_payload_marks_truncation_fields() {
        let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
        let mut payload = json!({
            "tool": "extra_mcp@db_query",
            "ok": true,
            "data": {
                "structured_content": {
                    "rows": [
                        {"text": text}
                    ]
                }
            },
            "meta": {
                "duration_ms": 12
            }
        });

        compact_observation_payload(&mut payload, "执行命令");

        assert_eq!(
            payload.get("truncated").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload
                .get("continuation_required")
                .and_then(Value::as_bool),
            None
        );
        assert!(
            payload
                .get("observation_output_chars")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                > 0
        );
        assert!(payload.get("meta").is_none());
    }

    #[test]
    fn test_compact_observation_payload_marks_continuation_when_resumable() {
        let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
        let mut payload = json!({
            "tool": "extra_mcp@db_query",
            "ok": true,
            "data": {
                "structured_content": {
                    "query_handle": "handle_123",
                    "rows": [
                        {"text": text}
                    ]
                }
            }
        });

        compact_observation_payload(&mut payload, "extra_mcp@db_query");

        assert_eq!(
            payload
                .get("continuation_required")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload.get("continuation_hint").and_then(Value::as_str),
            Some(TRUNCATION_CONTINUATION_HINT)
        );
    }

    #[test]
    fn test_compact_observation_payload_marks_continuation_for_read_file() {
        let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
        let mut payload = json!({
            "tool": "读取文件",
            "ok": true,
            "data": {
                "content": text,
                "meta": {
                    "files": [
                        {
                            "path": "notes.md",
                            "used_default_range": true,
                            "read_lines": 2000,
                            "total_lines": 4907
                        }
                    ],
                    "read": {
                        "requested_files": 1,
                        "processed_files": 1
                    }
                }
            }
        });

        compact_observation_payload(&mut payload, "读取文件");

        assert_eq!(
            payload
                .get("continuation_required")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload.get("continuation_hint").and_then(Value::as_str),
            Some(TRUNCATION_CONTINUATION_HINT)
        );
        let data = payload.get("data").and_then(Value::as_object);
        assert!(data.and_then(|value| value.get("meta")).is_none());
        assert!(data.and_then(|value| value.get("content")).is_some());
        assert_eq!(
            data.and_then(|value| value.get("content_omitted_chars"))
                .and_then(Value::as_u64),
            Some(
                (OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80
                    - OBSERVATION_READ_CONTENT_HEAD_CHARS) as u64
            )
        );
    }

    #[test]
    fn test_compact_observation_payload_read_file_strips_read_output_notice() {
        let mut payload = json!({
            "tool": "璇诲彇鏂囦欢",
            "ok": true,
            "data": {
                "content": "line1\nline2\n...(truncated read output, omitted 512 bytes)...",
                "meta": {
                    "files": [
                        {
                            "path": "notes.md",
                            "read_lines": 2,
                            "total_lines": 100,
                            "complete": false
                        }
                    ],
                    "read": {
                        "requested_files": 1,
                        "processed_files": 1
                    }
                }
            }
        });

        compact_observation_payload(&mut payload, "璇诲彇鏂囦欢");

        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            data.get("read_output_omitted_bytes")
                .and_then(Value::as_u64),
            Some(512)
        );
        assert_eq!(
            data.get("continuation_required").and_then(Value::as_bool),
            Some(true)
        );
        let content = data.get("content").and_then(Value::as_str).unwrap_or("");
        assert!(!content.contains("truncated read output"));
    }

    #[test]
    fn test_compact_observation_payload_read_file_limits_content_without_marker() {
        let long_content = "x".repeat(OBSERVATION_READ_CONTENT_HEAD_CHARS + 240);
        let mut payload = json!({
            "tool": "璇诲彇鏂囦欢",
            "ok": true,
            "data": {
                "content": long_content,
                "meta": {
                    "files": [
                        {
                            "path": "notes.md",
                            "read_lines": 800,
                            "total_lines": 2000,
                            "complete": false
                        }
                    ],
                    "read": {
                        "requested_files": 1,
                        "processed_files": 1
                    }
                }
            }
        });

        compact_observation_payload(&mut payload, "璇诲彇鏂囦欢");

        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            data.get("content_omitted_chars").and_then(Value::as_u64),
            Some(240)
        );
        assert_eq!(
            data.get("continuation_required").and_then(Value::as_bool),
            Some(true)
        );
        let content = data.get("content").and_then(Value::as_str).unwrap_or("");
        assert_eq!(content.chars().count(), OBSERVATION_READ_CONTENT_HEAD_CHARS);
        assert!(!content.contains(TOOL_RESULT_TRUNCATION_MARKER));
    }

    #[test]
    fn test_compact_observation_payload_compacts_search_payload() {
        let hits = (0..12)
            .map(|idx| {
                json!({
                    "path": format!("docs/{idx}.md"),
                    "line": idx + 1,
                    "content": format!("match-{idx}-{}", "x".repeat(240)),
                    "before": [],
                    "after": [],
                    "segments": [{"matched": true, "text": "match"}]
                })
            })
            .collect::<Vec<_>>();
        let mut payload = json!({
            "tool": "search_content",
            "ok": true,
            "data": {
                "query": "match",
                "query_mode": "literal",
                "path": "docs",
                "strategy": "literal_exact",
                "returned_match_count": 12,
                "matched_file_count": 12,
                "hits": hits,
                "matches": ["a:1:match", "b:2:match"],
                "meta": {"search": {"elapsed_ms": 20}},
                "scope": {"kind": "workspace_local"},
                "scope_note": "debug"
            }
        });

        compact_observation_payload(&mut payload, "search_content");

        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert!(data.get("meta").is_none());
        assert!(data.get("scope").is_none());
        assert!(data.get("scope_note").is_none());
        assert!(data.get("hits").is_none());
        assert!(data.get("hits_jsonl").and_then(Value::as_str).is_some());
        assert!(data.get("matches").is_none());
        assert!(data.get("matches_jsonl").and_then(Value::as_str).is_some());
        let first_hit = data
            .get("hits_jsonl")
            .and_then(Value::as_str)
            .and_then(|value| value.lines().next())
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        assert!(first_hit.get("content").is_none());
        assert!(first_hit
            .get("content_head")
            .and_then(Value::as_str)
            .is_some());
        assert!(first_hit
            .get("content_head")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.contains(TOOL_RESULT_TRUNCATION_MARKER)));
    }

    #[test]
    fn test_compact_dense_arrays_to_jsonl_converts_all_array_keys() {
        let mut data = json!({
            "items": ["a", "b"],
            "summary": {
                "top_files": ["x.md", "y.md"],
                "scores": [1, 2, 3]
            },
            "empty_list": []
        });

        compact_dense_arrays_to_jsonl(&mut data);

        let obj = data.as_object().cloned().unwrap_or_default();
        assert!(obj.get("items").is_none());
        assert_eq!(obj.get("items_count").and_then(Value::as_u64), Some(2));
        assert!(obj.get("items_jsonl").and_then(Value::as_str).is_some());
        assert_eq!(obj.get("empty_list_count").and_then(Value::as_u64), Some(0));
        assert_eq!(
            obj.get("empty_list_jsonl").and_then(Value::as_str),
            Some("")
        );

        let summary = obj
            .get("summary")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert!(summary.get("top_files").is_none());
        assert!(summary.get("scores").is_none());
        assert_eq!(
            summary.get("top_files_count").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(summary.get("scores_count").and_then(Value::as_u64), Some(3));
        assert!(summary
            .get("top_files_jsonl")
            .and_then(Value::as_str)
            .is_some());
        assert!(summary
            .get("scores_jsonl")
            .and_then(Value::as_str)
            .is_some());
    }

    #[test]
    fn test_compact_dense_arrays_to_jsonl_slims_execute_command_rows() {
        let mut data = json!({
            "results": [
                {
                    "command": "python draw_heart.py",
                    "command_index": 0,
                    "command_session_id": "cmd_123",
                    "returncode": 127,
                    "stdout": "",
                    "stderr": "python: command not found",
                    "output_meta": {
                        "truncated": false,
                        "total_bytes": 40
                    }
                }
            ]
        });

        compact_dense_arrays_to_jsonl(&mut data);

        let obj = data.as_object().cloned().unwrap_or_default();
        assert!(obj.get("results").is_none());
        assert_eq!(obj.get("results_count").and_then(Value::as_u64), Some(1));
        let line = obj
            .get("results_jsonl")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parsed = serde_json::from_str::<Value>(line).unwrap_or(Value::Null);
        let parsed_obj = parsed.as_object().cloned().unwrap_or_default();
        assert_eq!(
            parsed_obj.get("command").and_then(Value::as_str),
            Some("python draw_heart.py")
        );
        assert_eq!(
            parsed_obj.get("returncode").and_then(Value::as_i64),
            Some(127)
        );
        assert_eq!(
            parsed_obj.get("stderr").and_then(Value::as_str),
            Some("python: command not found")
        );
        assert!(parsed_obj.get("stdout").is_none());
        assert!(parsed_obj.get("output_meta").is_none());
        assert!(parsed_obj.get("command_session_id").is_none());
        assert!(parsed_obj.get("command_index").is_none());
    }

    #[test]
    fn test_compact_observation_payload_samples_large_rows() {
        let rows = (0..24)
            .map(|idx| json!({ "employee_id": format!("E{idx:06}"), "eligible": "yes" }))
            .collect::<Vec<_>>();
        let mut payload = json!({
            "tool": "extra_mcp@db_query",
            "ok": true,
            "data": {
                "structured_content": {
                    "ok": true,
                    "row_count": 24,
                    "rows": rows
                }
            }
        });

        compact_observation_payload(&mut payload, "extra_mcp@db_query");

        let data = payload.get("data").cloned().unwrap_or(Value::Null);
        assert!(data.get("rows").is_none());
        assert!(data.get("rows_jsonl").and_then(Value::as_str).is_some());
        assert_eq!(
            data.get("rows_sampled").and_then(Value::as_u64),
            Some(OBSERVATION_TABLE_SAMPLE_ROWS as u64)
        );
        assert_eq!(data.get("rows_omitted").and_then(Value::as_u64), Some(20));
    }

    #[test]
    fn test_compact_observation_payload_preserves_truncated_wrapper_preview_budget() {
        let preview = "x".repeat(OBSERVATION_MAX_CHARS + 4_096);
        let mut payload = json!({
            "tool": "extra_mcp@db_query",
            "ok": true,
            "data": {
                "truncated": true,
                "original_chars": 4096,
                "preview": preview,
                "truncation_reasons": ["array_items", "char_budget"],
                "continuation_required": true,
                "continuation_hint": TRUNCATION_CONTINUATION_HINT
            }
        });

        compact_observation_payload(&mut payload, "extra_mcp@db_query");

        let data = payload.get("data").cloned().unwrap_or(Value::Null);
        let preview = data.get("preview").and_then(Value::as_str).unwrap_or("");
        let mut data_without_preview = data.as_object().cloned().unwrap_or_default();
        data_without_preview.remove("preview");
        let expected_budget = OBSERVATION_MAX_CHARS.saturating_sub(
            estimate_tool_result_chars(&Value::Object(data_without_preview))
                .saturating_add("preview".chars().count()),
        );
        assert!(preview.contains(TOOL_RESULT_TRUNCATION_MARKER));
        assert!(expected_budget > 18_000);
        assert_eq!(preview.chars().count(), expected_budget);
        assert!(estimate_tool_result_chars(&data) <= OBSERVATION_MAX_CHARS);
        assert_eq!(
            data.get("continuation_required").and_then(Value::as_bool),
            Some(true)
        );
        let reasons = data
            .get("truncation_reasons")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(reasons
            .iter()
            .any(|item| item.as_str() == Some("array_items")));
        assert!(reasons
            .iter()
            .any(|item| item.as_str() == Some("char_budget")));
    }

    #[test]
    fn test_compact_observation_payload_skips_skill_call_truncation() {
        let text = "x".repeat(OBSERVATION_MAX_CHARS + 500);
        let mut payload = json!({
            "tool": "技能调用",
            "ok": true,
            "data": {
                "skill_md": text,
            }
        });

        compact_observation_payload(&mut payload, "技能调用");

        assert!(payload.get("meta").is_none());
        assert!(payload.get("truncated").is_none());
        assert!(
            payload
                .get("data")
                .and_then(|value| value.get("skill_md"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .count()
                > OBSERVATION_MAX_CHARS
        );
    }

    #[test]
    fn test_normalize_final_response_payload_from_json_object() {
        let answer = normalize_final_response_payload(r#"{"content":"0"}"#);
        assert_eq!(answer, "0");
    }

    #[test]
    fn test_normalize_final_response_payload_from_content_tag() {
        let answer = normalize_final_response_payload("<content>done</content>");
        assert_eq!(answer, "done");
    }

    #[test]
    fn test_normalize_final_response_payload_strips_think_block() {
        let answer = normalize_final_response_payload(
            r#"{"content":"<think>internal reasoning</think>\n\ndone"}"#,
        );
        assert_eq!(answer, "done");
    }
    #[test]
    fn test_extract_tagged_block_supports_final_response() {
        let answer = extract_tagged_block(
            "<final_response>{\"content\":\"ok\"}</final_response>",
            "final_response",
        );
        assert_eq!(answer.as_deref(), Some("{\"content\":\"ok\"}"));
    }

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
