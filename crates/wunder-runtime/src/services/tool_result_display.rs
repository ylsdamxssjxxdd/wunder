//! Bounded user-facing tool output for native clients. Never serialize an
//! unknown payload as a fallback; transport/debug fields are not UI content.
use serde_json::Value;

pub fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(2400).collect();
    let truncated = chars.next().is_some();
    let mut lines = head.lines();
    let mut output = lines.by_ref().take(16).collect::<Vec<_>>().join("\n");
    if truncated || lines.next().is_some() {
        output.push_str("\n…");
    }
    output
}

fn string<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
}

/// Keep one action and its retained result together. `pending` is supplied by
/// the event lifecycle, so absent output never implies successful completion.
pub fn tool_result_display(tool: &str, payload: &Value, pending: bool) -> String {
    let result = payload.get("result").unwrap_or(payload);
    let mut data = result;
    for _ in 0..4 {
        if let Some(nested) = data.get("data").filter(|v| v.is_object()) {
            data = nested;
        } else {
            break;
        }
    }
    let args = payload
        .get("args")
        .or_else(|| payload.get("arguments"))
        .or_else(|| result.get("args"))
        .unwrap_or(&Value::Null);
    let label = match tool {
        "read_file" | "读取文件" => "读取",
        "write_file" | "写入文件" => "写入",
        "apply_patch" | "应用补丁" => "应用补丁",
        "文本编辑" | "编辑" | "edit" => "文本编辑",
        "execute_command" | "执行命令" => "执行",
        "search_content" | "搜索内容" => "搜索",
        "list_files" | "列出文件" => "列出",
        "" => "工具",
        other => other,
    };
    let failed = [payload, result, data].iter().any(|v| {
        v.get("ok") == Some(&Value::Bool(false)) || v.get("success") == Some(&Value::Bool(false))
    });
    let cancelled = [payload, result]
        .iter()
        .any(|v| matches!(v["status"].as_str(), Some("cancelled" | "canceled")));
    let status = if cancelled {
        "已取消"
    } else if pending {
        "运行中"
    } else if failed {
        "失败"
    } else {
        "完成"
    };
    let mut lines = vec![format!("{} · {status}", preview(label))];
    let target = string(args, &["path", "file_path", "command", "cmd", "query"]);
    if !target.is_empty() {
        lines.push(preview(target));
    }
    if pending {
        return lines.join("\n");
    }
    let error = string(result, &["error", "message"]);
    if failed && !error.is_empty() {
        lines.push(preview(error));
    }
    let command_results = data.get("results").and_then(Value::as_array);
    if label == "执行" {
        let records = command_results
            .map(Vec::as_slice)
            .unwrap_or(std::slice::from_ref(data));
        for record in records.iter().take(3) {
            let cmd = string(record, &["command"]);
            if !cmd.is_empty() && cmd != target {
                lines.push(preview(cmd));
            }
            for field in ["stdout", "stderr"] {
                let text = string(record, &[field]);
                if !text.is_empty() {
                    lines.push(preview(text));
                }
            }
            if let Some(code) = record.get("returncode").or_else(|| record.get("exit_code")) {
                if code.is_number() {
                    lines.push(format!("exit {code}"));
                }
            }
        }
    } else if label == "应用补丁" {
        if let Some(files) = data.get("files").and_then(Value::as_array) {
            for file in files.iter().take(6) {
                lines.push(preview(string(file, &["path", "to_path"])));
                if let Some(blocks) = file.get("diff_blocks").and_then(Value::as_array) {
                    for block in blocks.iter().take(2) {
                        if let Some(diff) = block.get("lines").and_then(Value::as_array) {
                            for line in diff.iter().take(12) {
                                let sign = match line["kind"].as_str() {
                                    Some("add") => "+",
                                    Some("delete") => "-",
                                    _ => " ",
                                };
                                lines.push(format!("{sign}{}", preview(string(line, &["text"]))));
                            }
                            if diff.len() > 12 {
                                lines.push("…".into());
                            }
                        }
                    }
                }
            }
            if files.len() > 6 {
                lines.push("…".into());
            }
        }
    } else {
        let text = string(
            data,
            &[
                "summary",
                "message",
                "answer",
                "content_preview",
                "content",
                "text",
                "output",
            ],
        );
        if !text.is_empty() {
            lines.push(preview(text));
        } else if label == "写入" {
            let content = string(args, &["content", "text"]);
            if !failed && !cancelled && !content.is_empty() {
                lines.push(preview(content));
            }
        }
        let path = string(data, &["path", "file_path", "url", "title"]);
        if !path.is_empty() && path != target {
            lines.push(preview(path));
        }
        for key in ["items", "matches", "hits", "chunks", "documents", "results"] {
            if let Some(items) = data.get(key).and_then(Value::as_array) {
                for item in items.iter().take(6) {
                    let text = item.as_str().unwrap_or_else(|| {
                        string(
                            item,
                            &["content", "text", "message", "path", "title", "name"],
                        )
                    });
                    if !text.is_empty() {
                        lines.push(preview(text));
                    }
                }
                if items.len() > 6 {
                    lines.push("…".into());
                }
                break;
            }
        }
    }
    preview(
        &lines
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn results_keep_content_and_hide_internal_fields() {
        let output = tool_result_display(
            "read_file",
            &json!({"ok":true,"data":{"content":"sample text","query_handle":"hidden"}}),
            false,
        );
        assert!(output.contains("sample text"));
        assert!(!output.contains("hidden"));
        let output = tool_result_display(
            "custom_tool",
            &json!({"data":{"cursor":123,"query_handle":"hidden"}}),
            false,
        );
        assert!(!output.contains("123"));
        assert!(!output.contains("hidden"));
    }
    #[test]
    fn pending_and_failed_writes_never_claim_applied_content() {
        let payload = json!({"ok":false,"error":"write denied","args":{"content":"not written"}});
        let output = tool_result_display("write_file", &payload, false);
        assert!(output.contains("write denied"));
        assert!(!output.contains("not written"));
        assert!(tool_result_display("write_file", &payload, true).contains("运行中"));
    }
    #[test]
    fn commands_and_patches_keep_useful_output_with_bounds() {
        let output = tool_result_display(
            "execute_command",
            &json!({"data":{"results":[{"stdout":"hello","returncode":-1}]}}),
            false,
        );
        assert!(output.contains("hello"));
        assert!(output.contains("exit -1"));
        let output = tool_result_display(
            "apply_patch",
            &json!({"data":{"files":[{"path":"sample.txt","diff_blocks":[{"lines":[{"kind":"add","text":"new"},{"kind":"delete","text":"old"}]}]}]}}),
            false,
        );
        assert!(output.contains("+new"));
        assert!(output.contains("-old"));
        assert!(preview(&"x".repeat(10000)).len() < 2500);
    }
}
