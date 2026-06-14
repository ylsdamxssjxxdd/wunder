use serde_json::Value;

const MAX_PREVIEW_ITEMS: usize = 6;
const MAX_PREVIEW_LINES: usize = 6;
const MAX_PREVIEW_CHARS: usize = 360;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolDisplayLine {
    pub(crate) label: Option<String>,
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ToolDisplaySummary {
    pub(crate) summary: Option<String>,
    pub(crate) details: Vec<ToolDisplayLine>,
}

pub(crate) fn summarize_tool_result(
    tool_name: &str,
    payload: &Value,
) -> Option<ToolDisplaySummary> {
    let result = payload.get("result").unwrap_or(payload);
    let data = result.get("data").unwrap_or(result);
    let is_zh = looks_like_zh_text(tool_name);

    summarize_list_items(data, is_zh)
        .or_else(|| summarize_search_matches(data, is_zh))
        .or_else(|| summarize_read_files(data, is_zh))
        .or_else(|| summarize_write_file(result, data))
        .or_else(|| summarize_read_image(data))
        .or_else(|| summarize_skill_call(data, is_zh))
        .or_else(|| summarize_lsp_query(result, data, is_zh))
        .or_else(|| summarize_ptc(data, is_zh))
        .or_else(|| summarize_scalar_object(data, is_zh))
}

fn summarize_list_items(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let items = data.get("items").and_then(Value::as_array)?;
    let preview = collect_string_items(items, MAX_PREVIEW_ITEMS)?;
    let mut details = preview
        .into_iter()
        .map(|item| ToolDisplayLine {
            label: None,
            text: item,
        })
        .collect::<Vec<_>>();
    append_remaining_count(&mut details, items.len(), MAX_PREVIEW_ITEMS, is_zh);
    Some(ToolDisplaySummary {
        summary: Some(if is_zh {
            format!("{} 个条目", items.len())
        } else {
            format!("{} items", items.len())
        }),
        details,
    })
}

fn summarize_search_matches(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let matches = data.get("matches").and_then(Value::as_array)?;
    let preview = collect_string_items(matches, MAX_PREVIEW_ITEMS)?;
    let mut details = preview
        .into_iter()
        .map(|item| ToolDisplayLine {
            label: None,
            text: item,
        })
        .collect::<Vec<_>>();
    append_remaining_count(&mut details, matches.len(), MAX_PREVIEW_ITEMS, is_zh);
    Some(ToolDisplaySummary {
        summary: Some(if is_zh {
            format!("{} 个匹配", matches.len())
        } else {
            format!("{} matches", matches.len())
        }),
        details,
    })
}

fn summarize_read_files(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let content = data.get("content").and_then(Value::as_str)?;
    let files = data
        .get("meta")
        .and_then(|meta| meta.get("files"))
        .and_then(Value::as_array);
    let mut details = Vec::new();
    let mut summary = None;

    if let Some(files) = files {
        let mut file_lines = files
            .iter()
            .take(4)
            .filter_map(format_read_file_summary)
            .map(|text| ToolDisplayLine { label: None, text })
            .collect::<Vec<_>>();
        if !file_lines.is_empty() {
            details.append(&mut file_lines);
        }
        if let Some(first) = files.first().and_then(format_read_file_summary) {
            summary = Some(if files.len() == 1 {
                first
            } else if is_zh {
                format!("{} 个文件", files.len())
            } else {
                format!("{} files", files.len())
            });
        }
        append_remaining_count(&mut details, files.len(), 4, is_zh);
    }

    append_text_preview(
        &mut details,
        Some(if is_zh { "预览:" } else { "preview:" }),
        content,
        MAX_PREVIEW_LINES,
        MAX_PREVIEW_CHARS,
        is_zh,
    );

    Some(ToolDisplaySummary {
        summary: summary.or_else(|| {
            (!content.trim().is_empty()).then(|| {
                if is_zh {
                    "文件内容".to_string()
                } else {
                    "file content".to_string()
                }
            })
        }),
        details,
    })
}

fn summarize_write_file(result: &Value, data: &Value) -> Option<ToolDisplaySummary> {
    let path = result
        .get("path")
        .or_else(|| data.get("path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let bytes = number_value(result.get("bytes")).max(number_value(data.get("bytes")));
    let mut details = Vec::new();
    if let Some(bytes) = bytes {
        details.push(ToolDisplayLine {
            label: Some("bytes:".to_string()),
            text: format_bytes(bytes),
        });
    }
    Some(ToolDisplaySummary {
        summary: Some(path.to_string()),
        details,
    })
}

fn summarize_read_image(data: &Value) -> Option<ToolDisplaySummary> {
    let path = data.get("path").and_then(Value::as_str)?.trim();
    if path.is_empty() {
        return None;
    }
    let mut details = Vec::new();
    if let Some(mime) = data
        .get("mime_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(ToolDisplayLine {
            label: Some("mime:".to_string()),
            text: mime.to_string(),
        });
    }
    if let Some(size) = number_value(data.get("size_bytes")) {
        details.push(ToolDisplayLine {
            label: Some("size:".to_string()),
            text: format_bytes(size),
        });
    }
    if let Some(prompt) = data
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(ToolDisplayLine {
            label: Some("prompt:".to_string()),
            text: truncate_text(prompt, 120),
        });
    }
    Some(ToolDisplaySummary {
        summary: Some(path.to_string()),
        details,
    })
}

fn summarize_skill_call(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let name = data.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let mut details = Vec::new();
    if let Some(path) = data
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(ToolDisplayLine {
            label: Some("path:".to_string()),
            text: path.to_string(),
        });
    }
    if let Some(root) = data
        .get("root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(ToolDisplayLine {
            label: Some("root:".to_string()),
            text: root.to_string(),
        });
    }
    if let Some(tree) = data.get("tree").and_then(Value::as_array) {
        details.push(ToolDisplayLine {
            label: Some(if is_zh { "树:" } else { "tree:" }.to_string()),
            text: if is_zh {
                format!("{} 个条目", tree.len())
            } else {
                format!("{} items", tree.len())
            },
        });
    }
    Some(ToolDisplaySummary {
        summary: Some(name.to_string()),
        details,
    })
}

fn summarize_lsp_query(result: &Value, data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let operation = result
        .get("operation")
        .or_else(|| data.get("operation"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let path = result
        .get("path")
        .or_else(|| data.get("path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let results = result
        .get("results")
        .or_else(|| data.get("results"))
        .and_then(Value::as_array);
    let mut details = Vec::new();
    if let Some(results) = results {
        for item in results.iter().take(4) {
            let server_name = item
                .get("server_name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("server");
            details.push(ToolDisplayLine {
                label: None,
                text: server_name.to_string(),
            });
        }
        append_remaining_count(&mut details, results.len(), 4, is_zh);
    }
    let summary = if path.is_empty() {
        operation.to_string()
    } else {
        format!("{operation} {path}")
    };
    Some(ToolDisplaySummary {
        summary: Some(summary),
        details,
    })
}

fn summarize_ptc(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let path = data.get("path").and_then(Value::as_str)?.trim();
    let workdir = data
        .get("workdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let returncode = number_value(data.get("returncode"));
    if path.is_empty() || returncode.is_none() {
        return None;
    }
    let mut details = Vec::new();
    if !workdir.is_empty() {
        details.push(ToolDisplayLine {
            label: Some(if is_zh { "目录:" } else { "cwd:" }.to_string()),
            text: workdir.to_string(),
        });
    }
    append_text_preview(
        &mut details,
        Some("stdout:"),
        data.get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        4,
        260,
        is_zh,
    );
    append_text_preview(
        &mut details,
        Some("stderr:"),
        data.get("stderr")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        4,
        260,
        is_zh,
    );
    Some(ToolDisplaySummary {
        summary: Some(format!("{path} · exit={}", returncode.unwrap_or_default())),
        details,
    })
}

fn summarize_scalar_object(data: &Value, is_zh: bool) -> Option<ToolDisplaySummary> {
    let object = data.as_object()?;
    let mut parts = Vec::new();
    for key in [
        "path",
        "name",
        "operation",
        "query",
        "url",
        "mime_type",
        "bytes",
        "size_bytes",
    ] {
        let Some(value) = object.get(key) else {
            continue;
        };
        if let Some(text) = scalar_value_text(value) {
            parts.push(format!("{key}={text}"));
        }
        if parts.len() >= 3 {
            break;
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(ToolDisplaySummary {
        summary: Some(parts.join(if is_zh { "，" } else { ", " })),
        details: Vec::new(),
    })
}

fn format_read_file_summary(file: &Value) -> Option<String> {
    let path = file.get("path").and_then(Value::as_str)?.trim();
    if path.is_empty() {
        return None;
    }
    let read_lines = number_value(file.get("read_lines")).unwrap_or_default();
    let total_lines = number_value(file.get("total_lines")).unwrap_or_default();
    let complete = file
        .get("complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(if total_lines > 0 {
        if complete {
            format!("{path} · {total_lines}/{total_lines} lines")
        } else {
            format!("{path} · {read_lines}/{total_lines} lines")
        }
    } else {
        path.to_string()
    })
}

fn append_remaining_count(
    details: &mut Vec<ToolDisplayLine>,
    total: usize,
    shown: usize,
    is_zh: bool,
) {
    if total > shown {
        details.push(ToolDisplayLine {
            label: None,
            text: if is_zh {
                format!("… 还有 {} 项", total.saturating_sub(shown))
            } else {
                format!("... {} more", total.saturating_sub(shown))
            },
        });
    }
}

fn append_text_preview(
    details: &mut Vec<ToolDisplayLine>,
    label: Option<&str>,
    text: &str,
    max_lines: usize,
    max_chars: usize,
    is_zh: bool,
) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return;
    }
    let (preview, truncated_chars) = truncate_by_chars(trimmed, max_chars);
    let lines = preview.lines().take(max_lines).collect::<Vec<_>>();
    if lines.is_empty() {
        return;
    }
    details.push(ToolDisplayLine {
        label: label.map(ToString::to_string),
        text: lines[0].to_string(),
    });
    for line in lines.iter().skip(1) {
        details.push(ToolDisplayLine {
            label: None,
            text: (*line).to_string(),
        });
    }
    let total_lines = preview.lines().count();
    if total_lines > max_lines || truncated_chars {
        let hidden_lines = total_lines.saturating_sub(max_lines);
        let suffix = if hidden_lines > 0 && truncated_chars {
            if is_zh {
                format!("… 还有 {hidden_lines} 行，已截断")
            } else {
                format!("... {hidden_lines} more lines, truncated")
            }
        } else if hidden_lines > 0 {
            if is_zh {
                format!("… 还有 {hidden_lines} 行")
            } else {
                format!("... {hidden_lines} more lines")
            }
        } else if is_zh {
            "… 已截断".to_string()
        } else {
            "... truncated".to_string()
        };
        details.push(ToolDisplayLine {
            label: None,
            text: suffix,
        });
    }
}

fn collect_string_items(items: &[Value], limit: usize) -> Option<Vec<String>> {
    let preview = items
        .iter()
        .take(limit)
        .map(|item| item.as_str().map(|text| truncate_text(text.trim(), 140)))
        .collect::<Option<Vec<_>>>()?;
    Some(preview)
}

fn scalar_value_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|text| truncate_text(text.trim(), 96))
        .filter(|text| !text.is_empty())
        .or_else(|| number_value(Some(value)).map(format_bytes))
        .or_else(|| value.as_bool().map(|flag| flag.to_string()))
}

fn format_bytes(value: i64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    if value < 1024 * 1024 {
        return format!("{:.1} KB", value as f64 / 1024.0);
    }
    format!("{:.1} MB", value as f64 / (1024.0 * 1024.0))
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let (preview, truncated) = truncate_by_chars(text, max_chars);
    if truncated {
        format!("{preview}…")
    } else {
        preview
    }
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

fn number_value(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().map(|num| num.min(i64::MAX as u64) as i64))
            .or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<i64>().ok())
            })
    })
}

fn looks_like_zh_text(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || ('\u{20000}'..='\u{2a6df}').contains(&ch)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_list_files_result_uses_preview_lines() {
        let summary = summarize_tool_result(
            "列出文件",
            &serde_json::json!({ "items": ["src/", "src/main.rs", "Cargo.toml"] }),
        )
        .expect("summary");
        assert_eq!(summary.summary.as_deref(), Some("3 个条目"));
        assert_eq!(summary.details.len(), 3);
        assert_eq!(summary.details[0].text, "src/");
    }

    #[test]
    fn summarize_write_file_result_prefers_path_and_size() {
        let summary = summarize_tool_result(
            "write_file",
            &serde_json::json!({ "ok": true, "path": "src/main.rs", "bytes": 1536 }),
        )
        .expect("summary");
        assert_eq!(summary.summary.as_deref(), Some("src/main.rs"));
        assert!(summary
            .details
            .iter()
            .any(|line| line.text.contains("1.5 KB")));
    }

    #[test]
    fn summarize_read_files_result_uses_meta_and_preview() {
        let summary = summarize_tool_result(
            "读取文件",
            &serde_json::json!({
                "content": ">>> src/main.rs\n1: fn main() {}",
                "meta": {
                    "files": [
                        { "path": "src/main.rs", "read_lines": 1, "total_lines": 10, "complete": false }
                    ]
                }
            }),
        )
        .expect("summary");
        assert!(summary
            .summary
            .as_deref()
            .is_some_and(|text| text.contains("src/main.rs")));
        assert!(summary
            .details
            .iter()
            .any(|line| line.text.contains("1: fn main() {}")));
    }
}
