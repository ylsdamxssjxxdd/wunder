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

/// Skill loading reuses one tool for every skill. The row must be identified by
/// the skill it loads, not by the entry resource (usually `SKILL.md`) that every
/// skill in the timeline shares.
fn is_skill_tool(tool: &str) -> bool {
    matches!(tool, "技能调用" | "skill_call" | "skill_get")
}

/// The memory tool serves every action through one schema; the card must name
/// the concrete operation (which memory was written, which query ran) instead
/// of a bare tool name.
fn is_memory_tool(tool: &str) -> bool {
    matches!(tool, "记忆管理" | "memory_manager" | "memory_manage")
}

fn memory_action_name(raw: &str) -> &'static str {
    match raw.trim().to_lowercase().as_str() {
        "list" => "list",
        "get" | "read" | "detail" => "get",
        "add" | "create" | "append" => "add",
        "update" | "upsert" => "update",
        "delete" | "remove" => "remove",
        "clear" | "reset" => "clear",
        "recall" | "search" | "query" | "retrieve" => "search",
        _ => "",
    }
}

/// Bounded body for one memory field: enough to recognise what was stored,
/// never the whole record (the full content stays in args and audit events).
fn memory_body_preview(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(400).collect();
    let truncated = chars.next().is_some();
    let mut lines = head.lines();
    let mut output = lines.by_ref().take(4).collect::<Vec<_>>().join("\n");
    if truncated || lines.next().is_some() {
        output.push_str("\n…");
    }
    output
}

/// Fallback operation line for older persisted threads whose result `data`
/// predates the backend-stamped `detail` field.
fn memory_operation_line(action: &str, args: &Value, data: &Value) -> String {
    let title = string(args, &["title"]);
    let query = string(args, &["query"]);
    let memory_id = [string(args, &["memory_id"]), string(data, &["memory_id"])]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("");
    let subject = |text: &str| {
        if text.is_empty() {
            String::new()
        } else {
            format!("「{}」", text)
        }
    };
    match action {
        "add" => format!("保存记忆{}", subject(title)),
        "update" => format!("更新记忆{}", subject(title)),
        "get" => format!("读取记忆{}", subject(if title.is_empty() { memory_id } else { title })),
        "remove" => format!("删除记忆{}", subject(memory_id)),
        "clear" => "清空记忆".to_string(),
        "list" => {
            let count = data.get("count").and_then(Value::as_u64).unwrap_or(0);
            format!("列出记忆（{count} 条）")
        }
        "search" => format!("搜索记忆{}", subject(query)),
        _ => String::new(),
    }
}

/// Memory rows lead with the concrete operation, then the stored content or
/// matched entries. `data.detail` (backend-stamped, localized) is preferred;
/// raw args keep older replays readable.
fn append_memory_display(
    args: &Value,
    data: &Value,
    pending: bool,
    failed: bool,
    cancelled: bool,
    lines: &mut Vec<String>,
) {
    let action = memory_action_name(string(args, &["action"]));
    let detail = string(data, &["detail"]);
    let operation = if detail.is_empty() {
        memory_operation_line(action, args, data)
    } else {
        detail.to_string()
    };
    if !operation.is_empty() {
        lines.push(preview(&operation));
    }
    if pending {
        return;
    }
    match action {
        "add" | "update" if !failed && !cancelled => {
            let content = string(args, &["content"]);
            if !content.is_empty() {
                lines.push(memory_body_preview(content));
            }
        }
        "get" if !failed && !cancelled => {
            let content = string(data.pointer("/item").unwrap_or(&Value::Null), &["content"]);
            if !content.is_empty() {
                lines.push(memory_body_preview(content));
            }
        }
        "list" | "search" if !failed && !cancelled => {
            if let Some(items) = data.get("items").and_then(Value::as_array) {
                for item in items.iter().take(6) {
                    let title = string(item, &["title"]);
                    let snippet = string(item, &["snippet"]);
                    let line = if snippet.is_empty() {
                        title.to_string()
                    } else if title.is_empty() {
                        snippet.to_string()
                    } else {
                        format!("{title} · {snippet}")
                    };
                    if !line.is_empty() {
                        lines.push(preview(&line));
                    }
                }
                if items.len() > 6 {
                    lines.push("…".into());
                }
            }
        }
        _ => {}
    }
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
        "glob_files" | "glob" | "文件名匹配" => "匹配",
        "web_search" | "网页搜索" => "网页搜索",
        "web_fetch" | "网页抓取" => "网页抓取",
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
    if is_memory_tool(tool) {
        append_memory_display(args, data, pending, failed, cancelled, &mut lines);
        return preview(&lines.join("\n"));
    }
    let target = if is_skill_tool(tool) {
        let declared = string(args, &["name", "skill_name", "skillName"]);
        if declared.is_empty() {
            string(data, &["name"])
        } else {
            declared
        }
    } else {
        string(
            args,
            &["path", "file_path", "command", "cmd", "query", "url"],
        )
    };
    // `queries` is a batch (`string[]`) and dsh's search card title is the
    // joined batch, so fall back to it when no single-string target exists.
    let queries_target: String = args
        .get("queries")
        .and_then(Value::as_array)
        .map(|queries| {
            queries
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let target_text = if target.is_empty() {
        queries_target.as_str()
    } else {
        target
    };
    // Card-grade previews (storage rows, live process events) replace the raw
    // args/result objects with precomputed single-line previews.
    let target_text = if target_text.is_empty() {
        string(payload, &["args_preview"])
    } else {
        target_text
    };
    if !target_text.is_empty() {
        lines.push(preview(target_text));
    }
    if pending {
        return lines.join("\n");
    }
    let before_result = lines.len();
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
    } else if label == "网页搜索" {
        // dsh search seam: surface `sources:[{url,title,snippet}]` in order and
        // tell the reader when the batch was clipped to the result cap.
        if let Some(sources) = data.get("sources").and_then(Value::as_array) {
            for source in sources.iter().take(8) {
                let url = string(source, &["url"]);
                if url.is_empty() {
                    continue;
                }
                let title = string(source, &["title"]);
                if title.is_empty() {
                    lines.push(preview(url));
                } else {
                    lines.push(preview(&format!("{title} · {url}")));
                }
            }
            if sources.is_empty() {
                let text = string(data, &["summary", "message", "content"]);
                if !text.is_empty() {
                    lines.push(preview(text));
                }
            } else if sources.len() > 8 {
                lines.push("…".into());
            }
        }
        if data.get("truncated").and_then(Value::as_bool) == Some(true) {
            lines.push("结果已按上限截断".into());
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
        let path = if is_skill_tool(tool) {
            // The loaded resource path is an implementation detail of the load;
            // echoing `SKILL.md` would make every skill row look identical.
            ""
        } else {
            string(data, &["path", "file_path", "url", "title"])
        };
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
    if lines.len() == before_result {
        let result_preview = string(payload, &["result_preview"]);
        if !result_preview.is_empty() {
            lines.push(preview(result_preview));
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
    fn skill_rows_show_the_skill_name_instead_of_the_entry_resource() {
        let output = tool_result_display(
            "技能调用",
            &json!({
                "ok":true,
                "args": {"name":"demo_skill","action":"read","path":"SKILL.md"},
                "data": {"name":"demo_skill","path":"SKILL.md","content":"# Demo"}
            }),
            false,
        );
        let mut lines = output.lines();
        assert!(lines.next().unwrap_or_default().contains("技能调用"));
        assert_eq!(lines.next(), Some("demo_skill"));
        assert!(!output.contains("SKILL.md"));
    }
    #[test]
    fn pending_and_failed_writes_never_claim_applied_content() {        let payload = json!({"ok":false,"error":"write denied","args":{"content":"not written"}});
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
    #[test]
    fn web_tools_surface_sources_and_targets() {
        let output = tool_result_display(
            "web_search",
            &json!({"args":{"queries":["rust","async"]},"data":{"count":2,"truncated":true,"provider":"firecrawl","sources":[{"title":"Rust","url":"https://rust.test"},{"url":"https://async.test"}]}}),
            false,
        );
        assert!(output.contains("网页搜索 · 完成"));
        assert!(output.contains("rust, async"));
        assert!(output.contains("Rust · https://rust.test"));
        assert!(output.contains("https://async.test"));
        assert!(output.contains("结果已按上限截断"));
        assert!(!output.contains("firecrawl"));
        let fetch = tool_result_display(
            "web_fetch",
            &json!({"args":{"url":"https://example.test/a"},"data":{"url":"https://example.test/a","status":200,"content":"hello"}}),
            false,
        );
        assert!(fetch.contains("网页抓取 · 完成"));
        assert!(fetch.contains("https://example.test/a"));
        assert!(fetch.contains("hello"));
    }
    #[test]
    fn memory_rows_name_the_operation_and_content() {
        let output = tool_result_display(
            "记忆管理",
            &json!({
                "args": {"action":"add","title":"Reply language","content":"Reply in Chinese by default."},
                "data": {"action":"add","memory_id":"preftone","saved":true,"title":"Reply language","detail":"已保存记忆「Reply language」"}
            }),
            false,
        );
        assert!(output.contains("记忆管理 · 完成"));
        assert!(output.contains("已保存记忆「Reply language」"));
        assert!(output.contains("Reply in Chinese by default."));
        let failed = tool_result_display(
            "memory_manager",
            &json!({"ok":false,"error":"denied","args":{"action":"add","content":"not stored"},"data":{}}),
            false,
        );
        assert!(failed.contains("失败"));
        assert!(!failed.contains("not stored"));
    }
    #[test]
    fn memory_rows_derive_operations_from_args_for_old_replays() {
        let output = tool_result_display(
            "memory_manager",
            &json!({"args":{"action":"create","title":"Reply language","content":"Reply in Chinese by default."},"data":{"memory_id":"preftone","saved":true}}),
            false,
        );
        assert!(output.contains("保存记忆「Reply language」"));
        let listed = tool_result_display(
            "记忆管理",
            &json!({"args":{"action":"list"},"data":{"action":"list","count":2,"items":[{"memory_id":"a","title":"Alpha"},{"memory_id":"b","title":"Beta"}]}}),
            false,
        );
        assert!(listed.contains("列出记忆（2 条）"));
        assert!(listed.contains("Alpha"));
        assert!(listed.contains("Beta"));
        let searched = tool_result_display(
            "记忆管理",
            &json!({"args":{"action":"recall","query":"language"},"data":{"action":"search","query":"language","count":1,"items":[{"title":"Reply language","snippet":"Reply in Chinese"}]}}),
            false,
        );
        assert!(searched.contains("搜索记忆「language」"));
        assert!(searched.contains("Reply language · Reply in Chinese"));
        let pending = tool_result_display(
            "记忆管理",
            &json!({"args":{"action":"add","title":"t"}}),
            true,
        );
        assert!(pending.contains("运行中"));
        assert!(pending.contains("保存记忆「t」"));
    }
}
