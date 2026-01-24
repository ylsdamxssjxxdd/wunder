use super::*;

pub(super) const TOOL_TIMEOUT_ERROR: &str = "tool_timeout";

pub(super) struct ToolResultPayload {
    pub(super) ok: bool,
    pub(super) data: Value,
    pub(super) error: String,
    pub(super) sandbox: bool,
    pub(super) timestamp: DateTime<Utc>,
}

impl ToolResultPayload {
    pub(super) fn from_value(value: Value) -> Self {
        let timestamp = Utc::now();
        if let Value::Object(map) = &value {
            if map.get("ok").and_then(Value::as_bool).is_some() && map.contains_key("data") {
                let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
                let data = map.get("data").cloned().unwrap_or_else(|| json!({}));
                let error = map
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let sandbox = map.get("sandbox").and_then(Value::as_bool).unwrap_or(false);
                return Self {
                    ok,
                    data,
                    error,
                    sandbox,
                    timestamp,
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
        }
    }

    fn to_observation_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
            "timestamp": self.timestamp.with_timezone(&Local).to_rfc3339(),
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
        payload
    }

    pub(super) fn to_event_payload(&self, tool_name: &str) -> Value {
        self.to_observation_payload(tool_name)
    }
}

impl Orchestrator {
    pub(super) fn append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
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
        let _ = self.workspace.append_chat(user_id, &payload);
    }

    pub(super) fn build_tool_observation(
        &self,
        tool_name: &str,
        result: &ToolResultPayload,
    ) -> String {
        serde_json::to_string(&result.to_observation_payload(tool_name))
            .unwrap_or_else(|_| "{}".to_string())
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
        if !include_payload {
            payload["__omit_payload"] = Value::Bool(true);
        }
        if result.sandbox {
            payload["sandbox"] = Value::Bool(true);
        }
        let _ = self.workspace.append_tool_log(user_id, &payload);
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
            let _ = self.workspace.append_artifact_log(user_id, &entry);
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
            ("替换文本", "replace"),
            ("编辑文件", "edit"),
        ]);
        if let Some(action) = file_actions.get(tool_name) {
            let paths = extract_file_paths(args);
            for path in paths {
                let mut meta = serde_json::Map::new();
                if let Value::Object(data) = &result.data {
                    if *action == "replace" {
                        if let Some(value) = data.get("replaced") {
                            meta.insert("replaced".to_string(), value.clone());
                        }
                    } else if *action == "write" {
                        if let Some(value) = data.get("bytes") {
                            meta.insert("bytes".to_string(), value.clone());
                        }
                    } else if *action == "edit" {
                        if let Some(value) = data.get("lines") {
                            meta.insert("lines".to_string(), value.clone());
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
        strip_tool_calls(content).trim().to_string()
    }

    pub(super) fn resolve_final_answer_from_tool(&self, args: &Value) -> String {
        if let Some(obj) = args.as_object() {
            let value = obj
                .get("content")
                .or_else(|| obj.get("answer"))
                .cloned()
                .unwrap_or(Value::Null);
            match value {
                Value::String(text) => text.trim().to_string(),
                Value::Null => String::new(),
                other => serde_json::to_string(&other).unwrap_or_else(|_| other.to_string()),
            }
        } else if let Some(text) = args.as_str() {
            text.trim().to_string()
        } else {
            String::new()
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
        let mut timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
        if tool_name == "a2a等待" {
            let wait_s = parse_timeout_secs(args.get("wait_s")).unwrap_or(0.0);
            if wait_s > 0.0 {
                timeout_s = timeout_s.max(wait_s);
            }
            if timeout_s <= 0.0 {
                timeout_s = config.a2a.timeout_s as f64;
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
