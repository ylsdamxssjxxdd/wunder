use super::*;

pub(super) struct ToolCall {
    pub(super) id: Option<String>,
    pub(super) name: String,
    pub(super) arguments: Value,
}

pub(super) fn apply_tool_name_map(
    calls: Vec<ToolCall>,
    map: &HashMap<String, String>,
) -> Vec<ToolCall> {
    if map.is_empty() {
        return calls;
    }
    calls
        .into_iter()
        .map(|call| {
            let name = map.get(call.name.trim()).cloned().unwrap_or(call.name);
            ToolCall {
                id: call.id,
                name,
                arguments: call.arguments,
            }
        })
        .collect()
}

fn tool_call_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        compile_regex(
            r"(?is)<tool_call\b[^>]*>(?P<payload>.*?)</tool_call\s*>",
            "tool_call_block",
        )
    })
    .as_ref()
}

fn tool_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        compile_regex(
            r"(?is)<tool\b[^>]*>(?P<payload>.*?)</tool\s*>",
            "tool_block",
        )
    })
    .as_ref()
}

fn tool_open_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| compile_regex(r"(?is)<(tool_call|tool)\b[^>]*>", "tool_open_tag"))
        .as_ref()
}

fn tool_close_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| compile_regex(r"(?is)</(tool_call|tool)\s*>", "tool_close_tag"))
        .as_ref()
}

fn find_json_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    for (index, &ch) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == b'\\' {
                escape = true;
                continue;
            }
            if ch == b'"' {
                in_string = false;
            }
            continue;
        }
        if ch == b'"' {
            in_string = true;
            continue;
        }
        if ch == b'{' || ch == b'[' {
            stack.push(ch);
            continue;
        }
        if ch == b'}' || ch == b']' {
            let opening = stack.pop()?;
            if opening == b'{' && ch != b'}' {
                return None;
            }
            if opening == b'[' && ch != b']' {
                return None;
            }
            if stack.is_empty() {
                return Some(index + 1);
            }
        }
    }
    None
}

fn extract_json_segments(payload: &str) -> Vec<(usize, usize, Value)> {
    let bytes = payload.as_bytes();
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'{' && bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let Some(end) = find_json_end(payload, index) else {
            index += 1;
            continue;
        };
        let Some(candidate) = payload.get(index..end) else {
            index += 1;
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            values.push((index, end, value));
            index = end;
            continue;
        }
        index += 1;
    }
    values
}

fn extract_prefixed_tool_name(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(token) = trimmed.split_whitespace().last() {
        let cleaned = clean_tool_call_name(token);
        if is_tool_name_token(cleaned.as_str()) {
            return Some(cleaned);
        }
    }

    for token in trimmed.split_whitespace().rev() {
        let cleaned = clean_tool_call_name(token);
        if !is_tool_name_token(cleaned.as_str()) {
            continue;
        }
        if cleaned.contains('_') || cleaned.contains('-') {
            return Some(cleaned);
        }
    }

    None
}

fn is_tool_name_token(cleaned: &str) -> bool {
    if cleaned.is_empty() || cleaned.len() > 64 {
        return false;
    }
    if cleaned.contains('{')
        || cleaned.contains('[')
        || cleaned.contains('}')
        || cleaned.contains(']')
    {
        return false;
    }

    let lowered = cleaned.to_ascii_lowercase();
    if matches!(
        lowered.as_str(),
        "tool" | "tool_call" | "function_call" | "json" | "bash" | "shell"
    ) {
        return false;
    }

    true
}

fn clean_tool_call_name(raw: &str) -> String {
    let mut name = raw.trim().to_string();
    if name.is_empty() {
        return name;
    }
    if let Some(pos) = name.find('>') {
        let prefix = name[..pos].to_lowercase();
        if prefix.contains("tool_call")
            || prefix.contains("function_call")
            || prefix.contains("<tool")
        {
            name = name[pos + 1..].to_string();
        }
    }
    for sep in [':', '\u{FF1A}'] {
        if let Some(pos) = name.rfind(sep) {
            let prefix = name[..pos].to_lowercase();
            if prefix.contains("tool_call")
                || prefix.contains("function_call")
                || prefix.contains("tool")
            {
                name = name[pos + 1..].to_string();
            }
        }
    }
    if let Some(pos) = name.find('<') {
        name = name[..pos].to_string();
    }
    let mut cleaned = name
        .trim_matches(|ch: char| {
            matches!(
                ch,
                ':' | '\u{FF1A}' | '=' | '>' | ')' | '(' | '`' | '"' | '\'' | ',' | ';'
            )
        })
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return String::new();
    }

    if !cleaned.is_ascii() || cleaned.contains('?') {
        if let Some(tail) = extract_ascii_tool_tail(cleaned.as_str()) {
            cleaned = tail;
        }
    }

    let lowered = cleaned.to_lowercase();
    if matches!(lowered.as_str(), "tool" | "tool_call" | "function_call") {
        return String::new();
    }
    cleaned
}

fn extract_ascii_tool_tail(input: &str) -> Option<String> {
    let mut start = input.len();

    for (index, ch) in input.char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            start = index;
            continue;
        }
        if start < input.len() {
            break;
        }
    }

    if start >= input.len() {
        return None;
    }

    let tail = input[start..].trim();
    if tail.is_empty() {
        return None;
    }
    Some(tail.to_string())
}

fn parse_prefixed_tool_calls(payload: &str, segments: &[(usize, usize, Value)]) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut last_end = 0usize;
    for (start, end, value) in segments {
        let prefix = payload.get(last_end..*start).unwrap_or("");
        let Some(name) = extract_prefixed_tool_name(prefix) else {
            last_end = *end;
            continue;
        };
        let arguments = match value {
            Value::Object(map) => {
                if is_tool_result_map(map) {
                    last_end = *end;
                    continue;
                }
                Value::Object(map.clone())
            }
            Value::Array(items) => Value::Array(items.clone()),
            other => json!({ "value": other }),
        };
        calls.push(ToolCall {
            id: None,
            name,
            arguments,
        });
        last_end = *end;
    }
    calls
}

fn has_tool_call_args(map: &serde_json::Map<String, Value>) -> bool {
    map.contains_key("arguments")
        || map.contains_key("args")
        || map.contains_key("parameters")
        || map.contains_key("params")
        || map.contains_key("input")
        || map.contains_key("payload")
}

fn extract_tool_call_id(map: &serde_json::Map<String, Value>) -> Option<String> {
    for key in [
        "id",
        "tool_call_id",
        "toolCallId",
        "call_id",
        "callId",
        "tool_use_id",
        "toolUseId",
    ] {
        if let Some(value) = map.get(key) {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(num) => num.to_string(),
                _ => continue,
            };
            let cleaned = text.trim().to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn is_tool_result_map(map: &serde_json::Map<String, Value>) -> bool {
    let tool = map.get("tool").and_then(Value::as_str).unwrap_or("").trim();
    if tool.is_empty() {
        return false;
    }
    let has_ok = map.get("ok").and_then(Value::as_bool).is_some();
    let has_data = map.contains_key("data") || map.contains_key("result");
    let has_error = map.contains_key("error");
    let has_timestamp = map
        .get("timestamp")
        .and_then(Value::as_str)
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false);
    (has_ok && (has_data || has_error)) || (has_timestamp && has_data)
}

fn normalize_tool_call(map: &serde_json::Map<String, Value>) -> Option<ToolCall> {
    normalize_tool_call_with_id(map, None)
}

fn normalize_tool_call_with_id(
    map: &serde_json::Map<String, Value>,
    id_override: Option<String>,
) -> Option<ToolCall> {
    if is_tool_result_map(map) {
        return None;
    }
    if !has_tool_call_args(map) {
        return None;
    }
    let name_value = map
        .get("name")
        .or_else(|| map.get("tool"))
        .or_else(|| map.get("tool_name"))
        .or_else(|| map.get("toolName"))
        .or_else(|| map.get("function_name"))
        .or_else(|| map.get("functionName"))?;
    let name = match name_value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let name = clean_tool_call_name(&name);
    if name.is_empty() {
        return None;
    }

    let args_value = map
        .get("arguments")
        .or_else(|| map.get("args"))
        .or_else(|| map.get("parameters"))
        .or_else(|| map.get("params"))
        .or_else(|| map.get("input"))
        .or_else(|| map.get("payload"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arguments = match args_value {
        Value::Null => json!({}),
        Value::String(text) => {
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "raw": text }))
        }
        other => other,
    };
    let id = id_override.or_else(|| extract_tool_call_id(map));
    Some(ToolCall {
        id,
        name,
        arguments,
    })
}

fn collect_tool_calls_from_value(value: &Value, calls: &mut Vec<ToolCall>) {
    match value {
        Value::Object(map) => {
            let mut handled = false;
            if let Some(call) = normalize_tool_call(map) {
                calls.push(call);
                handled = true;
            }
            if !handled {
                if let Some(function) = map.get("function").and_then(Value::as_object) {
                    let id = extract_tool_call_id(map);
                    if let Some(call) = normalize_tool_call_with_id(function, id) {
                        calls.push(call);
                    }
                }
            }
            for key in [
                "tool_calls",
                "toolCalls",
                "tool_call",
                "toolCall",
                "function_call",
                "functionCall",
            ] {
                if let Some(inner) = map.get(key) {
                    collect_tool_calls_from_value(inner, calls);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tool_calls_from_value(item, calls);
            }
        }
        _ => {}
    }
}

fn normalize_tool_calls(value: Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    collect_tool_calls_from_value(&value, &mut calls);
    calls
}

fn parse_tool_calls_payload(payload: &str, allow_prefixed: bool) -> Vec<ToolCall> {
    let payload = payload.trim();
    if payload.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        let calls = normalize_tool_calls(value);
        if !calls.is_empty() {
            return calls;
        }
    }
    let segments = extract_json_segments(payload);
    let mut calls = Vec::new();
    for (_, _, value) in &segments {
        calls.extend(normalize_tool_calls(value.clone()));
    }
    if allow_prefixed && calls.is_empty() && !segments.is_empty() {
        calls.extend(parse_prefixed_tool_calls(payload, &segments));
    }
    calls
}

fn parse_tool_calls_from_text(content: &str) -> Vec<ToolCall> {
    parse_tool_calls_from_text_inner(content, false)
}

fn parse_tool_calls_from_text_strict(content: &str) -> Vec<ToolCall> {
    parse_tool_calls_from_text_inner(content, true)
}

fn parse_tool_calls_from_text_inner(content: &str, strict: bool) -> Vec<ToolCall> {
    if content.trim().is_empty() {
        return Vec::new();
    }

    let mut calls = Vec::new();
    let mut blocks: Vec<(usize, String)> = Vec::new();
    if let Some(regex) = tool_call_block_regex() {
        for captures in regex.captures_iter(content) {
            if let Some(mat) = captures.get(0) {
                let payload = captures.name("payload").map(|m| m.as_str()).unwrap_or("");
                blocks.push((mat.start(), payload.to_string()));
            }
        }
    }
    if let Some(regex) = tool_block_regex() {
        for captures in regex.captures_iter(content) {
            if let Some(mat) = captures.get(0) {
                let payload = captures.name("payload").map(|m| m.as_str()).unwrap_or("");
                blocks.push((mat.start(), payload.to_string()));
            }
        }
    }
    blocks.sort_by_key(|(start, _)| *start);

    if !blocks.is_empty() {
        for (_, payload) in blocks {
            calls.extend(parse_tool_calls_payload(&payload, true));
        }
    }

    let open_matches = tool_open_tag_regex()
        .map(|regex| regex.find_iter(content).collect::<Vec<_>>())
        .unwrap_or_default();
    if !open_matches.is_empty() {
        for (index, mat) in open_matches.iter().enumerate() {
            let start = mat.end();
            let end = if index + 1 < open_matches.len() {
                open_matches[index + 1].start()
            } else {
                content.len()
            };
            let Some(payload) = content.get(start..end) else {
                continue;
            };
            calls.extend(parse_tool_calls_payload(payload, true));
        }
    }

    if !strict {
        calls.extend(parse_tool_calls_payload(content, false));
        if calls.is_empty() && content.contains("```") {
            calls.extend(parse_tool_calls_payload(content, true));
        }
        if calls.is_empty() {
            calls.extend(parse_shell_read_file_fallback(content));
        }
    }
    dedupe_tool_calls(calls)
}

fn fenced_code_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        compile_regex(
            r"(?is)```(?P<lang>[a-zA-Z0-9_+\-]*)[ \t]*\r?\n(?P<body>.*?)```",
            "fenced_code_block",
        )
    })
    .as_ref()
}

fn parse_shell_read_file_fallback(content: &str) -> Vec<ToolCall> {
    let Some(regex) = fenced_code_block_regex() else {
        return Vec::new();
    };

    let mut calls = Vec::new();
    for captures in regex.captures_iter(content) {
        let lang = captures
            .name("lang")
            .map(|value| value.as_str().trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !lang.is_empty()
            && !matches!(
                lang.as_str(),
                "bash" | "sh" | "zsh" | "shell" | "cmd" | "powershell" | "pwsh"
            )
        {
            continue;
        }

        let body = captures
            .name("body")
            .map(|value| value.as_str())
            .unwrap_or_default();
        if let Some(path) = parse_read_file_path_from_shell_block(body) {
            calls.push(ToolCall {
                id: None,
                name: "read_file".to_string(),
                arguments: json!({ "path": path }),
            });
        }
    }

    calls
}

fn parse_read_file_path_from_shell_block(body: &str) -> Option<String> {
    let commands = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if commands.len() != 1 {
        return None;
    }

    let command = commands[0];
    let parts = shell_words::split(command).ok()?;
    if parts.is_empty() {
        return None;
    }

    let command_name = parts[0].to_ascii_lowercase();
    if command_name == "cat" || command_name == "type" {
        for token in parts.iter().skip(1) {
            let cleaned = token.trim();
            if cleaned.is_empty() || cleaned.starts_with('-') {
                continue;
            }
            if matches!(cleaned, "|" | "||" | "&&" | ";") {
                break;
            }
            return Some(cleaned.to_string());
        }
        return None;
    }

    if command_name == "head" {
        for token in parts.iter().skip(1) {
            let cleaned = token.trim();
            if cleaned.is_empty() || cleaned.starts_with('-') {
                continue;
            }
            if matches!(cleaned, "|" | "||" | "&&" | ";") {
                break;
            }
            return Some(cleaned.to_string());
        }
        return None;
    }

    None
}

fn tool_call_name_args_signature(call: &ToolCall) -> String {
    let normalized_name = resolve_tool_name(call.name.trim());
    let args = serde_json::to_string(&canonicalize_json(&call.arguments)).unwrap_or_default();
    format!("{normalized_name}|{args}")
}

fn tool_call_signature(call: &ToolCall) -> String {
    if let Some(id) = call
        .id
        .as_ref()
        .map(|value| value.trim())
        .filter(|id| !id.is_empty())
    {
        return format!("id:{id}");
    }
    tool_call_name_args_signature(call)
}

fn dedupe_tool_calls(calls: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut seen = HashSet::new();
    let mut seen_name_args_with_id = HashSet::new();
    for call in &calls {
        if call
            .id
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            seen_name_args_with_id.insert(tool_call_name_args_signature(call));
        }
    }
    let mut merged = Vec::new();
    for call in calls {
        let name_args = tool_call_name_args_signature(&call);
        if call
            .id
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            let signature = tool_call_signature(&call);
            if seen.insert(signature) {
                merged.push(call);
            }
            continue;
        }
        if seen_name_args_with_id.contains(&name_args) {
            continue;
        }
        if seen.insert(name_args) {
            merged.push(call);
        }
    }
    merged
}

pub(super) fn collect_tool_calls_from_output(
    content: &str,
    reasoning: &str,
    tool_calls_payload: Option<&Value>,
) -> Vec<ToolCall> {
    let mut calls = parse_tool_calls_from_text(content);
    if calls.is_empty() {
        calls.extend(parse_tool_calls_from_text_strict(reasoning));
    }
    if let Some(payload) = tool_calls_payload {
        calls.extend(normalize_tool_calls(payload.clone()));
    }

    dedupe_tool_calls(calls)
}

pub(super) fn collect_tool_calls_from_payload(payload: &Value) -> Vec<ToolCall> {
    let calls = normalize_tool_calls(payload.clone());
    dedupe_tool_calls(calls)
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let mut ordered = Map::new();
            for key in keys {
                if let Some(entry) = map.get(&key) {
                    ordered.insert(key, canonicalize_json(entry));
                }
            }
            Value::Object(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

pub(super) fn strip_tool_calls(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut stripped = content.to_string();
    if let Some(regex) = tool_call_block_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_block_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_open_tag_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_close_tag_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    stripped.trim().to_string()
}

pub(super) fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid orchestrator regex {label}: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call_closed_tag() {
        let content = r#"<tool_call>{"name":"读取文件","arguments":{"files":[{"path":"a.txt"}]}}</tool_call>"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "读取文件");
        assert_eq!(
            calls[0]
                .arguments
                .get("files")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("path"))
                .and_then(Value::as_str),
            Some("a.txt")
        );
    }

    #[test]
    fn test_parse_tool_call_tool_tag_and_string_arguments() {
        let content =
            r#"<tool>{"name":"execute_command","arguments":"{\"content\":\"echo hi\"}"}</tool>"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "execute_command");
        assert_eq!(
            calls[0].arguments.get("content").and_then(Value::as_str),
            Some("echo hi")
        );
    }

    #[test]
    fn test_parse_tool_call_open_tag_without_close() {
        let content = r#"<tool_call>{"name":"最终回复","arguments":{"content":"ok"}}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "最终回复");
        assert_eq!(
            calls[0].arguments.get("content").and_then(Value::as_str),
            Some("ok")
        );
    }

    #[test]
    fn test_parse_tool_call_prefixed_name_without_close() {
        let content =
            r#"<tool_call>ptc{"content":"print('ok')","filename":"demo.py","workdir":"."}</think>"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ptc");
        assert_eq!(
            calls[0].arguments.get("filename").and_then(Value::as_str),
            Some("demo.py")
        );
    }

    #[test]
    fn test_parse_tool_call_tool_field_with_tag_prefix() {
        let content = r#"{"tool":"tool_call>ptc","args":{"filename":"demo.py"}}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ptc");
        assert_eq!(
            calls[0].arguments.get("filename").and_then(Value::as_str),
            Some("demo.py")
        );
    }

    #[test]
    fn test_collect_tool_calls_multiple() {
        let content = concat!(
            "<tool_call>{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>",
            "<tool_call>{\"name\":\"write_file\",\"arguments\":{\"path\":\"b.txt\",\"content\":\"x\"}}</tool_call>"
        );
        let calls = collect_tool_calls_from_output(content, "", None);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[1].name, "write_file");
    }

    #[test]
    fn test_strip_tool_calls_supports_tool_and_tool_call() {
        let content = "prefix <tool>{\"name\":\"x\",\"arguments\":{}}</tool> mid <tool_call>{\"name\":\"y\",\"arguments\":{}}</tool_call> suffix";
        assert_eq!(strip_tool_calls(content), "prefix  mid  suffix");
    }

    #[test]
    fn test_collect_tool_calls_from_reasoning() {
        let content = "no tools here";
        let reasoning =
            r#"<tool_call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>"#;
        let calls = collect_tool_calls_from_output(content, reasoning, None);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_collect_tool_calls_dedup() {
        let payload = r#"<tool_call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>"#;
        let calls = collect_tool_calls_from_output(payload, payload, None);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_collect_tool_calls_from_payload_value() {
        let payload = json!({
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" }
            }]
        });
        let calls = collect_tool_calls_from_output("", "", Some(&payload));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].id.as_deref(), Some("call_1"));
    }

    #[test]
    fn test_parse_tool_call_json_without_tags() {
        let content = "call: {\"tool\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}";
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("a.txt")
        );
    }

    #[test]
    fn test_parse_tool_call_function_wrapper() {
        let content = r#"{"tool_calls":[{"type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("a.txt")
        );
    }

    #[test]
    fn test_parse_tool_call_ignores_tool_result_payload() {
        let content = r#"{"tool":"read_file","ok":true,"data":{"path":"a.txt"}}"#;
        let calls = parse_tool_calls_from_text(content);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_call_dedup_with_argument_order() {
        let content = r#"<tool_call>{"name":"read_file","arguments":{"path":"a.txt","mode":"r"}}</tool_call>{"tool":"read_file","arguments":{"mode":"r","path":"a.txt"}}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_parse_tool_call_prefixed_json_fenced_block() {
        let content = r#"run read_file with fenced json

```json
{"path":"Cargo.toml"}
```"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("Cargo.toml")
        );
    }

    #[test]
    fn test_extract_prefixed_tool_name_supports_fullwidth_colon() {
        let name = extract_prefixed_tool_name("prefix\u{FF1A}read_file ");
        assert_eq!(name.as_deref(), Some("read_file"));
    }

    #[test]
    fn test_parse_tool_call_prefixed_json_with_file_path_alias() {
        let content = "call read_file with payload\n```json\n{\"file_path\":\"Cargo.toml\"}\n```";
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("file_path").and_then(Value::as_str),
            Some("Cargo.toml")
        );
    }

    #[test]
    fn test_parse_tool_call_shell_read_file_fallback_from_cat_block() {
        let content = "read file with shell block\n```bash\ncat /workspaces/Cargo.toml\n```";
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("/workspaces/Cargo.toml")
        );
    }

    #[test]
    fn test_parse_tool_call_shell_read_file_fallback_extracts_path_before_pipe() {
        let content = "```bash\ncat Cargo.toml | grep name\n```";
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("Cargo.toml")
        );
    }
}
