//! Typed, bounded tool details shared by live projection and durable replay.
use crate::native::{NativeWorkflowLine as Line, NativeWorkflowSection as Section};
use serde_json::Value;

fn text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value[*key].as_str())
        .unwrap_or_default()
        .chars()
        .take(2048)
        .collect()
}
fn line(text: String, kind: &str, number: String) -> Line {
    Line {
        ratio: -1.0,
        text,
        kind: kind.into(),
        number,
    }
}
/// One file action verb for the section meta line.
fn action_label(action: &str) -> &'static str {
    match action {
        "add" => "新增",
        "delete" => "删除",
        "move" => "移动",
        "preview" => "预览",
        _ => "更新",
    }
}
fn push_body(section: &mut Section, body: &str, kind: &str) {
    for value in body
        .lines()
        .take(120usize.saturating_sub(section.lines.len()))
    {
        section.lines.push(line(
            value.chars().take(2048).collect(),
            kind,
            String::new(),
        ));
    }
    if body.lines().count() > 120 {
        section
            .lines
            .push(line("仅展示前 120 行".into(), "note", String::new()));
    }
}

pub(super) fn project(tool: &str, payload: &Value, state: &str, fallback: &str) -> Vec<Section> {
    if tool == "上下文压缩" || tool == "context_compaction" {
        return compaction::project(payload, state);
    }
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
        .unwrap_or(&Value::Null);
    let failed = matches!(state, "failed" | "error" | "rejected")
        || result["ok"] == false
        || result["success"] == false;
    let pending = matches!(state, "running" | "queued");
    let mut sections = Vec::new();
    if tool == "apply_patch" || tool.contains("应用补丁") {
        // The structured patch cards are projected once in the shared module;
        // this view renders the same files as readable sections.
        for file in super::patch::project(tool, payload, args).iter().take(8) {
            let meta = format!(
                "{}  +{} −{}",
                action_label(&file.action),
                file.added,
                file.deleted
            );
            let mut section = Section {
                title: file.path.clone(),
                meta,
                kind: "patch".into(),
                ..Default::default()
            };
            section.lines.extend(file.lines.iter().map(|line| Line {
                ratio: -1.0,
                text: line.text.clone(),
                kind: line.kind.clone(),
                number: line.number.clone(),
            }));
            sections.push(section);
        }
    } else if tool == "execute_command" || tool.contains("执行命令") {
        let records = data["results"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(std::slice::from_ref(data));
        for record in records.iter().take(3) {
            let mut command = text(record, &["command", "cmd"]);
            if command.is_empty() {
                command = text(args, &["command", "cmd"]);
            }
            let exit = record
                .get("returncode")
                .or_else(|| record.get("exit_code"))
                .and_then(Value::as_i64);
            let mut section = Section {
                title: if command.is_empty() {
                    "命令输出".into()
                } else {
                    format!("$ {command}")
                },
                meta: exit.map(|v| format!("exit {v}")).unwrap_or_else(|| {
                    if pending {
                        "运行中"
                    } else if failed {
                        "失败"
                    } else {
                        "已完成"
                    }
                    .into()
                }),
                kind: "command".into(),
                ..Default::default()
            };
            for (key, kind) in [("stdout", "context"), ("stderr", "error")] {
                if let Some(body) = record[key].as_str() {
                    push_body(&mut section, body, kind);
                }
            }
            sections.push(section);
        }
    } else if tool == "web_search" || tool.contains("网页搜索") {
        // dsh search seam: one section titled by the (joined) query batch, one
        // line per source, and an explicit note when the batch was clipped.
        let queries = data["queries"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let single = text(args, &["query"]);
                (!single.is_empty()).then_some(single)
            })
            .unwrap_or_else(|| "网页搜索".to_string());
        let count = data["count"].as_u64().unwrap_or_default();
        let truncated = data["truncated"].as_bool().unwrap_or(false);
        let mut section = Section {
            title: queries,
            meta: if truncated {
                format!("{count} 条 · 已截断")
            } else {
                format!("{count} 条")
            },
            kind: "link".into(),
            ..Default::default()
        };
        if let Some(sources) = data["sources"].as_array() {
            for source in sources.iter().take(20) {
                let url = text(source, &["url"]);
                if url.is_empty() {
                    continue;
                }
                let title = text(source, &["title"]);
                let label = if title.is_empty() {
                    url
                } else {
                    format!("{title} · {url}")
                };
                section.lines.push(line(label, "link", String::new()));
            }
        }
        if section.lines.is_empty() {
            push_body(&mut section, fallback, "context");
        }
        sections.push(section);
    } else if tool == "web_fetch" || tool.contains("网页抓取") {
        let url = {
            let from_data = text(data, &["url", "normalized_url"]);
            if from_data.is_empty() {
                text(args, &["url"])
            } else {
                from_data
            }
        };
        let status = data
            .get("status")
            .or_else(|| data.get("status_code"))
            .or_else(|| data.get("statusCode"))
            .and_then(Value::as_i64);
        let truncated = data["truncated"].as_bool().unwrap_or(false);
        let mut meta = status.map(|value| format!("HTTP {value}")).unwrap_or_default();
        if truncated {
            if meta.is_empty() {
                meta = "已截断".into();
            } else {
                meta.push_str(" · 已截断");
            }
        }
        let mut section = Section {
            title: if url.is_empty() { "网页抓取".into() } else { url },
            meta,
            kind: "text".into(),
            ..Default::default()
        };
        let body = data["content"]
            .as_str()
            .or_else(|| data["text"].as_str())
            .or_else(|| data["output"].as_str());
        if let Some(body) = body {
            push_body(&mut section, body, "context");
        }
        if section.lines.is_empty() {
            push_body(&mut section, fallback, "context");
        }
        sections.push(section);
    }
    if sections.is_empty()
        && !failed
        && matches!(
            tool,
            "read_file"
                | "读取文件"
                | "write_file"
                | "写入文件"
                | "list_files"
                | "列出文件"
                | "search_content"
                | "搜索内容"
        )
    {
        let mut title = text(data, &["path", "file_path"]);
        if title.is_empty() {
            title = text(args, &["path", "file_path", "query"]);
        }
        let mut section = Section {
            title: if title.is_empty() { tool.into() } else { title },
            kind: "text".into(),
            ..Default::default()
        };
        let body = data["content"]
            .as_str()
            .or_else(|| data["text"].as_str())
            .or_else(|| data["output"].as_str());
        if let Some(body) = body {
            push_body(&mut section, body, "context");
        }
        if section.lines.is_empty() {
            push_body(&mut section, fallback, "context");
        }
        sections.push(section);
    }
    if failed {
        let error = text(result, &["error", "message"]);
        sections.push(Section {
            title: "执行失败".into(),
            kind: "error".into(),
            lines: vec![line(
                if error.is_empty() {
                    fallback.into()
                } else {
                    error
                },
                "error",
                String::new(),
            )],
            ..Default::default()
        });
    }
    if sections.is_empty() {
        let mut section = Section {
            title: "工具结果".into(),
            kind: "text".into(),
            ..Default::default()
        };
        push_body(&mut section, fallback, "context");
        sections.push(section);
    }
    // Bound total retained content across file sections, not only each line.
    let mut remaining = 64 * 1024;
    for section in &mut sections {
        let mut cut = false;
        section.lines.retain_mut(|line| {
            if remaining == 0 {
                cut = true;
                return false;
            }
            if line.text.len() > remaining {
                let mut end = remaining;
                while !line.text.is_char_boundary(end) {
                    end -= 1;
                }
                line.text.truncate(end);
                cut = true;
            }
            remaining -= line.text.len();
            true
        });
        if cut {
            section
                .lines
                .push(line("内容达到展示上限".into(), "note", String::new()));
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn patch_details_preserve_file_groups_line_numbers_and_failure() {
        let data = json!({"result":{"data":{"files":[{"path":"sample.txt","action":"update","diff_blocks":[{"header":"@@ -2 +2 @@","lines":[{"kind":"delete","old_line":2,"text":"old"},{"kind":"add","new_line":2,"text":"new"}]}]}]}}});
        let sections = project("apply_patch", &data, "completed", "fallback");
        assert_eq!(sections[0].title, "sample.txt");
        assert_eq!(sections[0].lines[1].number, "2");
        assert_eq!(sections[0].lines[2].kind, "add");
        let failed = project(
            "apply_patch",
            &json!({"ok":false,"error":"fixture failure"}),
            "failed",
            "fallback",
        );
        assert_eq!(failed[0].kind, "error");
        assert!(!failed.iter().any(|s| s.meta == "已完成"));
    }
    #[test]
    fn command_streams_and_exit_code_remain_separate() {
        let sections = project(
            "execute_command",
            &json!({"args":{"command":"fixture"},"result":{"data":{"stdout":"output","stderr":"error","returncode":1}}}),
            "completed",
            "fallback",
        );
        assert_eq!(sections[0].title, "$ fixture");
        assert_eq!(sections[0].meta, "exit 1");
        assert_eq!(sections[0].lines[1].kind, "error");
    }
    #[test]
    fn web_search_projects_sources_and_web_fetch_projects_status() {
        let sections = project(
            "web_search",
            &json!({"args":{"queries":["rust","async"]},"result":{"data":{"count":1,"truncated":true,"sources":[{"title":"Rust","url":"https://rust.test"}]}}}),
            "completed",
            "fallback",
        );
        assert_eq!(sections[0].title, "rust, async");
        assert_eq!(sections[0].meta, "1 条 · 已截断");
        assert!(sections[0].lines[0].text.contains("https://rust.test"));
        let fetch = project(
            "web_fetch",
            &json!({"args":{"url":"https://example.test/a"},"result":{"data":{"url":"https://example.test/a","status":200,"content":"hello"}}}),
            "completed",
            "fallback",
        );
        assert_eq!(fetch[0].title, "https://example.test/a");
        assert_eq!(fetch[0].meta, "HTTP 200");
        assert_eq!(fetch[0].lines[0].text, "hello");
    }
}

#[path = "native_compaction_view.rs"]
mod compaction;
