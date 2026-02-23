use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptWindowSpec {
    pub(super) start_entry: usize,
    pub(super) end_entry_exclusive: usize,
    pub(super) local_scroll: u16,
    pub(super) total_lines: usize,
}

pub(super) fn compute_transcript_window_spec(
    line_counts: &[usize],
    viewport_height: u16,
    offset_from_bottom: usize,
) -> TranscriptWindowSpec {
    if line_counts.is_empty() {
        return TranscriptWindowSpec {
            start_entry: 0,
            end_entry_exclusive: 0,
            local_scroll: 0,
            total_lines: 0,
        };
    }

    let viewport = usize::from(viewport_height.max(1));
    let total_lines = line_counts.iter().copied().sum::<usize>();
    let max_scroll = total_lines.saturating_sub(viewport);
    let offset = offset_from_bottom.min(max_scroll);
    let top_line = max_scroll.saturating_sub(offset);

    let mut cumulative = 0usize;
    let mut start_entry = 0usize;
    let mut start_line = 0usize;
    for (index, count) in line_counts.iter().copied().enumerate() {
        let next = cumulative.saturating_add(count);
        if top_line < next {
            start_entry = index;
            start_line = cumulative;
            break;
        }
        cumulative = next;
        start_entry = index.saturating_add(1);
        start_line = cumulative;
    }

    if start_entry >= line_counts.len() {
        start_entry = line_counts.len().saturating_sub(1);
        start_line = total_lines.saturating_sub(line_counts[start_entry]);
    }

    let local_scroll_lines = top_line.saturating_sub(start_line);
    let needed_lines = local_scroll_lines
        .saturating_add(viewport)
        .saturating_add(1);

    let mut end_entry_exclusive = start_entry;
    let mut rendered_lines = 0usize;
    while end_entry_exclusive < line_counts.len() && rendered_lines < needed_lines {
        rendered_lines = rendered_lines.saturating_add(line_counts[end_entry_exclusive]);
        end_entry_exclusive = end_entry_exclusive.saturating_add(1);
    }
    if end_entry_exclusive <= start_entry {
        end_entry_exclusive = start_entry.saturating_add(1).min(line_counts.len());
    }

    TranscriptWindowSpec {
        start_entry,
        end_entry_exclusive,
        local_scroll: local_scroll_lines.min(u16::MAX as usize) as u16,
        total_lines,
    }
}

pub(crate) fn log_prefix(kind: LogKind) -> &'static str {
    match kind {
        LogKind::Info => "- ",
        LogKind::User => "you> ",
        LogKind::Assistant => "assistant> ",
        LogKind::Reasoning => "think> ",
        LogKind::Tool => "tool> ",
        LogKind::Error => "error> ",
    }
}

pub(super) fn wrapped_log_visual_line_count(kind: LogKind, text: &str, width: usize) -> usize {
    let width = width.max(1).min(u16::MAX as usize) as u16;
    let mut rendered = String::with_capacity(log_prefix(kind).len().saturating_add(text.len()));
    rendered.push_str(log_prefix(kind));
    rendered.push_str(text);
    Paragraph::new(rendered)
        .wrap(Wrap { trim: false })
        .line_count(width)
        .max(1)
}

pub(super) fn backtrack_user_text(entry: &LogEntry) -> Option<String> {
    if entry.kind != LogKind::User {
        return None;
    }
    let text = entry.text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

pub(super) fn collect_recent_user_logs(logs: &[LogEntry], limit: usize) -> Vec<String> {
    logs.iter()
        .rev()
        .filter_map(backtrack_user_text)
        .take(limit)
        .collect()
}

pub(super) fn backtrack_preview_line(text: &str, max_chars: usize) -> String {
    let cleaned = text.trim();
    if cleaned.chars().count() <= max_chars {
        return cleaned.to_string();
    }
    let mut out = cleaned.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

pub(super) fn normalize_popup_token(raw: &str) -> Option<String> {
    let cleaned = raw
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | ','
                        | '.'
                        | ';'
                        | ':'
                        | ')'
                        | '('
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '\n'
                        | '\r'
                )
        })
        .trim();
    let first = cleaned.chars().next()?;
    if !matches!(first, '@' | '$' | '#') {
        return None;
    }
    let rest = cleaned[first.len_utf8()..].trim();
    if rest.is_empty() {
        return None;
    }
    let normalized_rest = if first == '@' {
        rest.replace('\\', "/")
    } else {
        rest.to_string()
    };
    Some(format!("{first}{normalized_rest}"))
}

pub(super) fn popup_token_matches(token: &str, prefix: char, lowered_query: &str) -> bool {
    let Some(rest) = token.strip_prefix(prefix) else {
        return false;
    };
    if lowered_query.is_empty() {
        return true;
    }
    rest.to_ascii_lowercase().contains(lowered_query)
}

pub(super) fn contains_token_case_insensitive(values: &[String], target: &str) -> bool {
    values.iter().any(|item| item.eq_ignore_ascii_case(target))
}

pub(super) fn dedupe_case_insensitive(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in values {
        let cleaned = item.trim();
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_ascii_lowercase();
        if seen.insert(key) {
            output.push(cleaned.to_string());
        }
    }
    output
}

pub(super) fn format_mcp_server_lines_for_tui(
    server: &UserMcpServer,
    is_zh: bool,
    detailed: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let state = format_mcp_state_label(server.enabled, is_zh);
    let auth = format_mcp_auth_state_for_tui(server, is_zh);
    if detailed {
        lines.push(if is_zh {
            format!("- 状态: {state}")
        } else {
            format!("- status: {state}")
        });
        lines.push(if is_zh {
            format!("- 鉴权: {auth}")
        } else {
            format!("- auth: {auth}")
        });
    } else {
        lines.push(format!("- {} ({state}, {auth})", server.name));
    }

    let transport = if server.transport.trim().is_empty() {
        "streamable-http"
    } else {
        server.transport.trim()
    };
    lines.push(if is_zh {
        format!("  传输: {transport}")
    } else {
        format!("  transport: {transport}")
    });
    lines.push(if is_zh {
        format!("  地址: {}", server.endpoint)
    } else {
        format!("  endpoint: {}", server.endpoint)
    });

    if detailed {
        let description = if server.description.trim().is_empty() {
            "-"
        } else {
            server.description.trim()
        };
        let display_name = if server.display_name.trim().is_empty() {
            "-"
        } else {
            server.display_name.trim()
        };
        lines.push(if is_zh {
            format!("  描述: {description}")
        } else {
            format!("  description: {description}")
        });
        lines.push(if is_zh {
            format!("  显示名: {display_name}")
        } else {
            format!("  display_name: {display_name}")
        });
        if !server.allow_tools.is_empty() {
            lines.push(if is_zh {
                format!("  允许工具: {}", server.allow_tools.join(", "))
            } else {
                format!("  allow_tools: {}", server.allow_tools.join(", "))
            });
        }
        if !server.shared_tools.is_empty() {
            lines.push(if is_zh {
                format!("  共享工具: {}", server.shared_tools.join(", "))
            } else {
                format!("  shared_tools: {}", server.shared_tools.join(", "))
            });
        }
    }

    if !server.tool_specs.is_empty() {
        lines.push(if is_zh {
            format!("  缓存工具数: {}", server.tool_specs.len())
        } else {
            format!("  cached_tools: {}", server.tool_specs.len())
        });
    }

    if is_zh {
        lines.push(format!(
            "  登录: wunder-cli mcp login {} --bearer-token <TOKEN>",
            server.name
        ));
        lines.push(format!("  退出: wunder-cli mcp logout {}", server.name));
    } else {
        lines.push(format!(
            "  login: wunder-cli mcp login {} --bearer-token <TOKEN>",
            server.name
        ));
        lines.push(format!("  logout: wunder-cli mcp logout {}", server.name));
    }
    lines
}

pub(super) fn format_mcp_state_label(enabled: bool, is_zh: bool) -> &'static str {
    if is_zh {
        if enabled {
            "启用"
        } else {
            "禁用"
        }
    } else if enabled {
        "enabled"
    } else {
        "disabled"
    }
}

pub(super) fn format_mcp_auth_state_for_tui(server: &UserMcpServer, is_zh: bool) -> String {
    if let Some(key) = detect_mcp_auth_key_for_tui(server) {
        if is_zh {
            format!("已登录（{}）", mcp_auth_key_label_for_tui(key, true))
        } else {
            format!("logged in ({})", mcp_auth_key_label_for_tui(key, false))
        }
    } else if is_zh {
        "未登录".to_string()
    } else {
        "not logged in".to_string()
    }
}

pub(super) fn find_mcp_server_index_for_tui(
    servers: &[UserMcpServer],
    target: &str,
) -> Option<usize> {
    let cleaned = target.trim();
    if cleaned.is_empty() {
        return None;
    }
    servers
        .iter()
        .position(|server| server.name.trim().eq_ignore_ascii_case(cleaned))
}

pub(super) fn mcp_auth_key_from_alias_for_tui(raw: &str) -> Option<&'static str> {
    match raw
        .trim()
        .trim_start_matches('-')
        .to_ascii_lowercase()
        .as_str()
    {
        "bearer-token" | "bearer_token" | "bearer" => Some("bearer_token"),
        "token" => Some("token"),
        "api-key" | "api_key" | "apikey" => Some("api_key"),
        _ => None,
    }
}

pub(super) fn detect_mcp_auth_key_for_tui(server: &UserMcpServer) -> Option<&'static str> {
    let Some(Value::Object(map)) = server.auth.as_ref() else {
        return None;
    };
    ["bearer_token", "token", "api_key"]
        .into_iter()
        .find(|key| {
            map.get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
        })
}

pub(super) fn mcp_auth_key_label_for_tui(key: &str, is_zh: bool) -> &'static str {
    match key {
        "bearer_token" => {
            if is_zh {
                "Bearer Token"
            } else {
                "bearer token"
            }
        }
        "token" => "token",
        "api_key" => {
            if is_zh {
                "API Key"
            } else {
                "api key"
            }
        }
        _ => {
            if is_zh {
                "未知"
            } else {
                "unknown"
            }
        }
    }
}

pub(super) fn normalize_statusline_item(raw: &str) -> Option<String> {
    let key = raw.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    let normalized = match key.as_str() {
        "running" | "run" | "status" | "状态" | "运行" => "running",
        "usage" | "token" | "tokens" | "用量" | "token占用" => "usage",
        "scroll" | "滚动" => "scroll",
        "mouse" | "鼠标" => "mouse",
        "focus" | "焦点" => "focus",
        "context" | "ctx" | "上下文" => "context",
        "session" | "sid" | "会话" => "session",
        "agent" | "agent_id" | "智能体" => "agent",
        "model" | "模型" => "model",
        "mode" | "tool_call_mode" | "工具模式" | "模式" => "mode",
        "approval" | "approvals" | "审批" | "授权" => "approval",
        "attach" | "attachments" | "附件" => "attach",
        _ => return None,
    };
    Some(normalized.to_string())
}

pub(super) fn normalize_name_list_for_tui(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut output = Vec::new();
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        if !seen.insert(cleaned.to_string()) {
            continue;
        }
        output.push(cleaned.to_string());
    }
    output
}

pub(super) fn is_paste_shortcut(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'v') => {
            let modifiers = key.modifiers;
            let has_paste_modifier = modifiers.contains(KeyModifiers::CONTROL)
                || modifiers.contains(KeyModifiers::SUPER);
            has_paste_modifier && !modifiers.contains(KeyModifiers::ALT)
        }
        KeyCode::Insert => key.modifiers.contains(KeyModifiers::SHIFT),
        _ => false,
    }
}

pub(super) fn normalize_clipboard_text(text: String) -> Option<String> {
    if text.is_empty() {
        return None;
    }

    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{0}', "");
    if normalized.is_empty() {
        return None;
    }

    Some(normalized)
}

pub(super) fn read_system_clipboard_text() -> Result<Option<String>> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "$ErrorActionPreference='Stop'; $value = Get-Clipboard -Raw; if ($null -ne $value) { [Console]::Out.Write($value) }",
            ])
            .output()
            .map_err(|error| anyhow!("failed to invoke powershell clipboard reader: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                return Ok(None);
            }
            return Err(anyhow!("powershell clipboard reader failed: {stderr}"));
        }
        Ok(normalize_clipboard_text(
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ))
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("pbpaste")
            .output()
            .map_err(|error| anyhow!("failed to invoke pbpaste: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                return Ok(None);
            }
            return Err(anyhow!("pbpaste failed: {stderr}"));
        }
        return Ok(normalize_clipboard_text(
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        const CLIPBOARD_COMMANDS: &[(&str, &[&str])] = &[
            ("wl-paste", &["-n"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ];

        for (program, args) in CLIPBOARD_COMMANDS {
            match Command::new(program).args(*args).output() {
                Ok(output) if output.status.success() => {
                    return Ok(normalize_clipboard_text(
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                    ));
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    if !stderr.is_empty() {
                        return Err(anyhow!("{program} failed: {stderr}"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(anyhow!("{program} failed: {error}")),
            }
        }

        return Err(anyhow!(
            "no supported clipboard command found (tried wl-paste, xclip, xsel)"
        ));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        Ok(None)
    }
}

#[cfg(test)]
pub(super) fn wrapped_visual_line_count_parts(prefix: &str, text: &str, width: usize) -> usize {
    let width = width.max(1);
    if prefix.is_empty() && text.is_empty() {
        return 1;
    }

    let mut line_count = 1usize;
    let mut line_columns = 0usize;
    for ch in prefix.chars().chain(text.chars()) {
        if ch == '\n' {
            line_count = line_count.saturating_add(1);
            line_columns = 0;
            continue;
        }

        let char_width = display_char_width(ch);
        if line_columns > 0 && line_columns.saturating_add(char_width) > width {
            line_count = line_count.saturating_add(1);
            line_columns = 0;
        }
        line_columns = line_columns.saturating_add(char_width).min(width);
    }

    line_count
}

#[cfg(test)]
pub(super) fn wrapped_visual_line_count(text: &str, width: usize) -> usize {
    wrapped_visual_line_count_parts("", text, width)
}

pub(super) fn move_cursor_vertical(text: &str, width: usize, cursor: usize, delta: i8) -> usize {
    let lines = build_wrapped_input_lines(text, width);
    if lines.len() <= 1 {
        return cursor.min(text.len());
    }

    let (row, col) = cursor_visual_position(text, &lines, cursor.min(text.len()));
    let target_row = if delta < 0 {
        row.saturating_sub(1)
    } else {
        (row + 1).min(lines.len().saturating_sub(1))
    };
    if target_row == row {
        return cursor.min(text.len());
    }

    let target = lines[target_row];
    byte_index_for_display_column(text, target.start, target.end, col)
}

pub(super) fn build_wrapped_input_lines(text: &str, width: usize) -> Vec<WrappedInputLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_columns = 0usize;

    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(WrappedInputLine {
                start: line_start,
                end: index,
            });
            line_start = index + ch.len_utf8();
            line_columns = 0;
            continue;
        }

        let char_width = display_char_width(ch);
        if line_columns > 0 && line_columns.saturating_add(char_width) > width {
            lines.push(WrappedInputLine {
                start: line_start,
                end: index,
            });
            line_start = index;
            line_columns = 0;
        }
        line_columns = line_columns.saturating_add(char_width);
    }

    lines.push(WrappedInputLine {
        start: line_start,
        end: text.len(),
    });
    lines
}

pub(super) fn cursor_visual_position(
    text: &str,
    lines: &[WrappedInputLine],
    cursor_index: usize,
) -> (usize, usize) {
    let cursor = cursor_index.min(text.len());
    for (row, line) in lines.iter().enumerate() {
        if cursor < line.start {
            continue;
        }
        if cursor <= line.end {
            if cursor == line.end && row + 1 < lines.len() && lines[row + 1].start == cursor {
                continue;
            }
            let col = display_width(&text[line.start..cursor]);
            return (row, col);
        }
    }

    let fallback = lines
        .last()
        .copied()
        .unwrap_or(WrappedInputLine { start: 0, end: 0 });
    let col = display_width(&text[fallback.start..cursor.min(fallback.end)]);
    (lines.len().saturating_sub(1), col)
}

pub(super) fn display_char_width(ch: char) -> usize {
    UnicodeWidthChar::width_cjk(ch)
        .or_else(|| UnicodeWidthChar::width(ch))
        .unwrap_or(0)
        .max(1)
}

pub(super) fn display_width(text: &str) -> usize {
    text.chars().map(display_char_width).sum()
}

pub(super) fn normalize_wrapped_cursor_position(
    (row, col): (usize, usize),
    width: usize,
) -> (usize, usize) {
    if width == 0 {
        return (row, 0);
    }
    if col < width {
        return (row, col);
    }

    (row.saturating_add(col / width), col % width)
}

pub(super) fn history_content_to_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item
                    .as_object()
                    .and_then(|obj| obj.get("text"))
                    .and_then(Value::as_str)
                {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            if parts.is_empty() {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                parts.join(
                    "
",
                )
            }
        }
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default()),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub(super) fn format_session_timestamp(ts: f64) -> String {
    if !ts.is_finite() || ts <= 0.0 {
        return "-".to_string();
    }
    let secs = ts.floor() as i64;
    let nanos = ((ts - secs as f64).max(0.0) * 1_000_000_000.0).round() as u32;
    chrono::Local
        .timestamp_opt(secs, nanos.min(999_999_999))
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(windows)]
pub(super) fn is_altgr(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::ALT) && modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(not(windows))]
pub(super) fn is_altgr(_modifiers: KeyModifiers) -> bool {
    false
}

pub(super) fn prev_char_boundary(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    let mut cursor = index.saturating_sub(1).min(text.len().saturating_sub(1));
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor = cursor.saturating_sub(1);
    }
    if text.is_char_boundary(cursor) {
        cursor
    } else {
        0
    }
}

pub(super) fn next_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut cursor = index.saturating_add(1);
    while cursor < text.len() && !text.is_char_boundary(cursor) {
        cursor += 1;
    }
    cursor.min(text.len())
}

pub(super) fn byte_index_for_display_column(
    text: &str,
    start: usize,
    end: usize,
    column: usize,
) -> usize {
    let mut consumed = 0usize;
    let mut cursor = start;

    for (offset, ch) in text[start..end].char_indices() {
        if consumed >= column {
            return start + offset;
        }
        let width = display_char_width(ch);
        consumed = consumed.saturating_add(width);
        cursor = start + offset + ch.len_utf8();
        if consumed >= column {
            return cursor;
        }
    }

    cursor.min(end)
}

pub(super) fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub(super) fn payload_has_tool_calls(payload: &Value) -> bool {
    match payload.get("tool_calls") {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

#[cfg(test)]
pub(super) fn sanitize_assistant_delta(delta: &str) -> String {
    let mut in_tool_markup = false;
    sanitize_assistant_delta_streaming(delta, &mut in_tool_markup)
}

pub(super) fn sanitize_assistant_delta_streaming(delta: &str, in_tool_markup: &mut bool) -> String {
    if delta.trim().is_empty() {
        return String::new();
    }

    let stripped = strip_streaming_tool_markup(delta, in_tool_markup);
    if stripped.trim().is_empty() {
        return String::new();
    }

    let cleaned = strip_tool_block_tags(stripped.as_str());
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || looks_like_tool_payload(trimmed) {
        return String::new();
    }

    cleaned
}

pub(super) fn sanitize_reasoning_text(text: &str) -> String {
    text.trim().to_string()
}

pub(super) fn sanitize_assistant_text(text: &str) -> String {
    strip_tool_block_tags(text).trim().to_string()
}

pub(super) fn looks_like_tool_payload(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("<tool_call")
        || lowered.contains("</tool_call>")
        || lowered.contains("\"name\"") && lowered.contains("\"arguments\"")
}

pub(super) fn strip_tool_block_tags(text: &str) -> String {
    let without_tool_call = strip_tag_block(text.to_string(), "<tool_call", "</tool_call>");
    strip_tag_block(without_tool_call, "<tool", "</tool>")
}

pub(super) fn strip_tag_block(mut text: String, start_tag: &str, end_tag: &str) -> String {
    loop {
        let lowered = text.to_ascii_lowercase();
        let Some(start) = lowered.find(start_tag) else {
            break;
        };

        let after_start = start + start_tag.len();
        let Some(close_offset) = lowered[after_start..].find('>') else {
            text.truncate(start);
            break;
        };
        let body_start = after_start + close_offset + 1;

        if let Some(end_offset) = lowered[body_start..].find(end_tag) {
            let end = body_start + end_offset + end_tag.len();
            text.replace_range(start..end, "");
        } else {
            text.truncate(start);
            break;
        }
    }
    text
}

pub(super) fn strip_streaming_tool_markup(text: &str, in_tool_markup: &mut bool) -> String {
    let mut output = String::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if *in_tool_markup {
            let lowered = remaining.to_ascii_lowercase();
            let end_call = lowered.find("</tool_call>");
            let end_tool = lowered.find("</tool>");
            let Some((end_index, end_tag)) = (match (end_call, end_tool) {
                (Some(left), Some(right)) if left <= right => Some((left, "</tool_call>")),
                (Some(_), Some(right)) => Some((right, "</tool>")),
                (Some(left), None) => Some((left, "</tool_call>")),
                (None, Some(right)) => Some((right, "</tool>")),
                (None, None) => None,
            }) else {
                return output;
            };

            let after_end = end_index + end_tag.len();
            remaining = &remaining[after_end..];
            *in_tool_markup = false;
            continue;
        }

        let lowered = remaining.to_ascii_lowercase();
        let start_call = lowered.find("<tool_call");
        let start_tool = lowered.find("<tool");
        let Some(start_index) = (match (start_call, start_tool) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        }) else {
            output.push_str(remaining);
            break;
        };

        output.push_str(&remaining[..start_index]);
        let after_start = &remaining[start_index..];
        let lowered_after = after_start.to_ascii_lowercase();
        let Some(close_offset) = lowered_after.find('>') else {
            *in_tool_markup = true;
            return output;
        };

        let body_start = start_index + close_offset + 1;
        remaining = &remaining[body_start..];
        *in_tool_markup = true;
    }

    output
}

pub(super) fn merge_stream_text(existing: &mut String, incoming: &str) {
    if incoming.is_empty() {
        return;
    }
    if existing.is_empty() {
        existing.push_str(incoming);
        return;
    }
    if existing == incoming {
        return;
    }
    if incoming.starts_with(existing.as_str()) {
        *existing = incoming.to_string();
        return;
    }
    if existing.ends_with(incoming) {
        return;
    }

    let overlap = longest_suffix_prefix_overlap(existing.as_str(), incoming);
    if overlap > 0 {
        existing.push_str(&incoming[overlap..]);
        return;
    }
    // Most streaming providers send raw deltas; when there is no overlap we should
    // append directly instead of forcing a newline, otherwise output becomes token-per-line.
    existing.push_str(incoming);
}

pub(super) fn longest_suffix_prefix_overlap(left: &str, right: &str) -> usize {
    let mut len = left.len().min(right.len());
    while len > 0 {
        if !left.is_char_boundary(left.len() - len) || !right.is_char_boundary(len) {
            len = len.saturating_sub(1);
            continue;
        }
        if left[left.len() - len..] == right[..len] {
            return len;
        }
        len = len.saturating_sub(1);
    }
    0
}

pub(super) fn compact_text_for_compare(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub(super) fn is_equivalent_text(left: &str, right: &str) -> bool {
    if left.trim().is_empty() || right.trim().is_empty() {
        return false;
    }
    compact_text_for_compare(left) == compact_text_for_compare(right)
}

pub(super) fn format_tool_call_line(tool: &str, args: &Value) -> String {
    if tool == "执行命令" {
        if let Some(command) = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("[tool_call] {tool} `{command}`");
        }
    }

    format!("[tool_call] {tool} {}", compact_json(args))
}

pub(super) fn format_tool_result_lines(tool: &str, payload: &Value) -> Vec<String> {
    let result = payload.get("result").unwrap_or(payload);
    if tool == "执行命令" {
        let lines = format_execute_command_result_lines(tool, result);
        if !lines.is_empty() {
            return lines;
        }
    }

    let ok = result.get("ok").and_then(Value::as_bool);
    let mut headline = format!("[tool_result] {tool}");
    if let Some(ok) = ok {
        headline.push_str(if ok { " ok" } else { " failed" });
    }

    let mut lines = vec![headline];
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("  error: {error}"));
    }

    let data = result.get("data").unwrap_or(result);
    lines.push(format!("  data: {}", compact_json(data)));
    lines
}

pub(super) fn format_execute_command_result_lines(tool: &str, result: &Value) -> Vec<String> {
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
    let returncode = value_as_i64(first.get("returncode"))
        .or_else(|| value_as_i64(result.get("meta").and_then(|meta| meta.get("exit_code"))));
    let duration_ms = value_as_i64(result.get("meta").and_then(|meta| meta.get("duration_ms")));

    let status = match returncode {
        Some(0) => "ok",
        Some(_) => "failed",
        None => "done",
    };

    let mut header = format!("[tool_result] {tool} {status}");
    let mut metrics = Vec::new();
    if let Some(returncode) = returncode {
        metrics.push(format!("exit={returncode}"));
    }
    if let Some(duration_ms) = duration_ms {
        metrics.push(format!("{duration_ms}ms"));
    }
    if !metrics.is_empty() {
        header.push_str(&format!(" ({})", metrics.join(", ")));
    }

    let mut lines = vec![header];
    if !command.is_empty() {
        lines.push(format!("  cmd: {command}"));
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
    if returncode.unwrap_or(0) == 0 {
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 6, 900);
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 4, 300);
    } else {
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 6, 900);
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 4, 300);
    }

    if !has_output {
        lines.push("  output: <empty>".to_string());
    }

    lines
}

pub(super) fn append_text_preview(
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

pub(super) fn truncate_by_chars(text: &str, max_chars: usize) -> (String, bool) {
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

pub(super) fn value_as_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().map(|num| num.min(i64::MAX as u64) as i64))
            .or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<i64>().ok())
            })
    })
}

pub(super) fn parse_error_message(data: &Value) -> String {
    let payload = event_payload(data);
    let nested_message = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|inner| inner.get("message"))
        .and_then(Value::as_str);
    payload
        .as_str()
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .or_else(|| payload.get("detail").and_then(Value::as_str))
        .or_else(|| payload.get("error").and_then(Value::as_str))
        .or(nested_message)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| compact_json(payload))
}

pub(super) fn event_payload(data: &Value) -> &Value {
    data.get("data").unwrap_or(data)
}

pub(super) fn compact_json(value: &Value) -> String {
    const MAX_INLINE_JSON_CHARS: usize = 200;
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_INLINE_JSON_CHARS {
        let mut safe_boundary = MAX_INLINE_JSON_CHARS;
        while safe_boundary > 0 && !text.is_char_boundary(safe_boundary) {
            safe_boundary = safe_boundary.saturating_sub(1);
        }
        text.truncate(safe_boundary);
        text.push_str("...");
    }
    text
}

pub(super) fn localize_cli_notice(language: &str, text: &str) -> String {
    if !crate::locale::is_zh_language(language) {
        return text.to_string();
    }

    if let Some(value) = text.strip_prefix("approved once: ") {
        return format!("已单次批准: {value}");
    }
    if let Some(value) = text.strip_prefix("approved for session: ") {
        return format!("已批准本会话: {value}");
    }
    if let Some(value) = text.strip_prefix("denied: ") {
        return format!("已拒绝: {value}");
    }
    if let Some(value) = text.strip_prefix("already using session: ") {
        return format!("当前已在会话: {value}");
    }
    if let Some(value) = text.strip_prefix("session not found: ") {
        return format!("会话不存在: {value}");
    }
    if let Some(value) = text.strip_prefix("switched to session: ") {
        return format!("已切换到会话: {value}");
    }
    if let Some(value) = text.strip_prefix("resumed session: ") {
        return format!("已恢复会话: {value}");
    }
    if let Some(value) = text.strip_prefix("model set: ") {
        return format!("模型已切换: {value}");
    }
    if let Some(value) = text.strip_prefix("current model: ") {
        return format!("当前模型: {value}");
    }
    if let Some(value) = text.strip_prefix("available models: ") {
        return format!("可用模型: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid /mouse args: ") {
        return format!("无效的 /mouse 参数: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid mode: ") {
        return format!("非法模式: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid approval mode: ") {
        return format!("非法审批模式: {value}");
    }
    if let Some(value) = text.strip_prefix("no files found for: ") {
        return format!("未找到文件: {value}");
    }
    if let Some(value) = text.strip_prefix("mention results (") {
        return format!("搜索结果 ({value}");
    }
    if let Some(value) = text.strip_prefix("extra prompt saved (") {
        return format!("额外提示词已保存 ({value}");
    }
    if let Some(value) = text.strip_prefix("tool_call_mode set: ") {
        return format!("工具调用模式已设置: {value}");
    }
    if let Some(value) = text.strip_prefix("approval_mode set: ") {
        return format!("审批模式已设置: {value}");
    }
    if let Some(value) = text.strip_prefix("model not found: ") {
        return format!("模型不存在: {value}");
    }
    if let Some(value) = text.strip_prefix("session index out of range: ") {
        return format!("会话索引越界: {value}");
    }
    if let Some(value) = text.strip_prefix("unknown command: ") {
        return format!("未知命令: {value}");
    }
    if let Some(value) = text.strip_prefix("model not found in config: ") {
        return format!("配置中不存在模型: {value}");
    }
    if let Some(value) = text.strip_prefix("- provider: ") {
        return format!("- 提供商: {value}");
    }
    if let Some(value) = text.strip_prefix("- base_url: ") {
        return format!("- base_url: {value}");
    }
    if let Some(value) = text.strip_prefix("- model: ") {
        return format!("- 模型: {value}");
    }
    if let Some(value) = text.strip_prefix("- tool_call_mode: ") {
        return format!("- 工具调用模式: {value}");
    }
    if let Some(value) = text.strip_prefix("- session: ") {
        return format!("- 会话: {value}");
    }
    if let Some(value) = text.strip_prefix("- id: ") {
        return format!("- 会话 ID: {value}");
    }
    if let Some(value) = text.strip_prefix("review task cancelled: ") {
        return format!("review 任务已取消: {value}");
    }
    if let Some(value) = text.strip_prefix("- extra_prompt: enabled (") {
        return format!("- 额外提示词: 已启用（{value}");
    }

    match text {
        "wunder-cli tui mode. type /help for commands." => {
            "wunder-cli TUI 模式。输入 /help 查看命令。".to_string()
        }
        "no historical sessions found" => "未找到历史会话".to_string(),
        "tip: start chatting first, then use /resume to switch" => {
            "提示：先发起对话，再用 /resume 切换。".to_string()
        }
        "tip: send a few messages first, then /resume to switch" => {
            "提示：先发送几条消息，再用 /resume 切换。".to_string()
        }
        "focus switched to input" => "焦点已切换到输入区".to_string(),
        "focus switched to output (arrows now select transcript)" => {
            "焦点已切换到输出区（方向键可选择日志）".to_string()
        }
        "assistant is still running, wait for completion before creating a new session" => {
            "助手仍在运行，请等待完成后再新建会话".to_string()
        }
        "assistant is still running, wait for completion before sending a new prompt" => {
            "助手仍在运行，请等待完成后再发送新消息".to_string()
        }
        "assistant is still running, wait for completion before running /review" => {
            "助手仍在运行，请等待完成后再执行 /review".to_string()
        }
        "assistant is still running, wait for completion before resuming another session" => {
            "助手仍在运行，请等待完成后再恢复其他会话".to_string()
        }
        "interrupt requested, waiting for running round to stop..." => {
            "已请求中断，等待当前轮次停止...".to_string()
        }
        "no cancellable round found, press Ctrl+C again to exit" => {
            "未找到可中断轮次，再按一次 Ctrl+C 退出".to_string()
        }
        "press Ctrl+C again to exit (or wait to continue)" => {
            "再按一次 Ctrl+C 退出（或等待继续）".to_string()
        }
        "usage: /mention <query>" => "用法: /mention <query>".to_string(),
        "usage: /approvals [show|suggest|auto_edit|full_auto]" => {
            "用法: /approvals [show|suggest|auto_edit|full_auto]".to_string()
        }
        "valid modes: suggest, auto_edit, full_auto" => {
            "可选模式: suggest, auto_edit, full_auto".to_string()
        }
        "usage: /tool-call-mode <tool_call|function_call> [model]" => {
            "用法: /tool-call-mode <tool_call|function_call> [model]".to_string()
        }
        "valid modes: tool_call, function_call" => "可选模式: tool_call, function_call".to_string(),
        "too many arguments" => "参数过多".to_string(),
        "config values cannot be empty" => "配置值不能为空".to_string(),
        "configure llm model (step 1/4)" => "配置 LLM 模型（步骤 1/4）".to_string(),
        "config cancelled" => "配置已取消".to_string(),
        "input base_url (empty line to cancel)" => "请输入 base_url（空行取消）".to_string(),
        "input api_key (step 2/4)" => "请输入 api_key（步骤 2/4）".to_string(),
        "input model name (step 3/4)" => "请输入模型名称（步骤 3/4）".to_string(),
        "input max_context (step 4/4, optional; Enter for auto probe)" => {
            "请输入 max_context（步骤 4/4，可选；回车自动探测）".to_string()
        }
        "model configured" => "模型配置完成".to_string(),
        "- max_context: auto probe unavailable (or keep existing)" => {
            "- max_context: 自动探测不可用（或保留现有值）".to_string()
        }
        "mouse mode: auto (wheel + temporary selection passthrough)" => {
            "鼠标模式：auto（滚轮 + 临时选择透传）".to_string()
        }
        "mouse mode: scroll (wheel enabled)" => "鼠标模式：scroll（启用滚轮）".to_string(),
        "mouse mode: select/copy (wheel disabled)" => {
            "鼠标模式：select/copy（禁用滚轮）".to_string()
        }
        "usage: /mouse [auto|scroll|select]  (F2 optional)" => {
            "用法: /mouse [auto|scroll|select]  （F2 可切换）".to_string()
        }
        "usage: /mouse [scroll|select]  (F2 to toggle)" => {
            "用法: /mouse [scroll|select]  （F2 切换）".to_string()
        }
        "resume picker opened (Up/Down to choose, Enter to resume, Esc to cancel)" => {
            "已打开会话恢复面板（上下选择，Enter 恢复，Esc 取消）".to_string()
        }
        "tip: /resume to list available sessions" => "提示: 用 /resume 列出可用会话".to_string(),
        "tip: run /resume list to inspect available sessions" => {
            "提示: 运行 /resume list 查看可用会话".to_string()
        }
        "type /help to list available slash commands" => {
            "输入 /help 查看可用 slash 命令".to_string()
        }
        "available models:" => "可用模型：".to_string(),
        "no models configured. run /config first." => "尚未配置模型，请先运行 /config".to_string(),
        "no saved session found" => "未找到保存的会话".to_string(),
        "no llm model configured" => "尚未配置 LLM 模型".to_string(),
        "session id is empty" => "会话 ID 不能为空".to_string(),
        "extra prompt cleared" => "额外提示词已清除".to_string(),
        "extra prompt is empty" => "额外提示词为空".to_string(),
        "- extra_prompt: none" => "- 额外提示词: 无".to_string(),
        "usage: /system [set <extra_prompt>|clear]" => {
            "用法: /system [set <extra_prompt>|clear]".to_string()
        }
        "invalid /system args" => "无效的 /system 参数".to_string(),
        "system" => "系统提示词".to_string(),
        "--- system prompt ---" => "--- 系统提示词开始 ---".to_string(),
        "--- end system prompt ---" => "--- 系统提示词结束 ---".to_string(),
        "stream ended without model output or final answer" => {
            "流式输出结束，但未收到模型输出或最终答案".to_string()
        }
        _ => text.to_string(),
    }
}

pub(super) fn build_workspace_file_index(root: &std::path::Path) -> Vec<IndexedFile> {
    const MAX_INDEX_FILES: usize = 50_000;
    let excluded_dirs = [
        ".git",
        "target",
        "WUNDER_TEMP",
        "data",
        "frontend",
        "web",
        "node_modules",
        "参考项目",
        "backups",
    ];
    let mut items = Vec::new();
    let walker = walkdir::WalkDir::new(root).follow_links(false);
    for entry in walker
        .into_iter()
        .filter_entry(|entry| {
            let path = entry.path();
            if path == root {
                return true;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return true;
            };
            !excluded_dirs
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded))
        })
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let rel = relative.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        items.push(IndexedFile {
            lowered: rel.to_ascii_lowercase(),
            path: rel,
        });
        if items.len() >= MAX_INDEX_FILES {
            break;
        }
    }
    items.sort_by(|left, right| left.path.cmp(&right.path));
    items
}
