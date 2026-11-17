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
    let mut values = body.lines();
    for value in values
        .by_ref()
        .take(120usize.saturating_sub(section.lines.len()))
    {
        section.lines.push(line(
            value.chars().take(2048).collect(),
            kind,
            String::new(),
        ));
    }
    if values.next().is_some() {
        section
            .lines
            .push(line("仅展示前 120 行".into(), "note", String::new()));
    }
}

/// A command section already prints the command as its title and the state as
/// its meta chip, and `fallback` is the row display text — "动作 · 状态", then
/// the command, then the retained output. Echoing those two leading lines into
/// the body repeated the whole command under itself, so they are dropped and
/// only lines the section does not already show are kept.
fn without_display_echo(fallback: &str, command: &str, meta: &str) -> String {
    let command = command.trim();
    let mut rest = fallback.lines().peekable();
    let header_echo = rest
        .peek()
        .copied()
        .and_then(|value| value.split_once(" · "))
        .is_some_and(|(action, status)| {
            matches!(action.trim(), "执行" | "运行命令")
                && matches!(status.trim(), "运行中" | "完成" | "失败" | "已取消")
        });
    if header_echo {
        rest.next();
    }
    if let Some(value) = rest.peek().copied() {
        // The call target sits right after the header. Display previews are
        // bounded, so a kept echo may be a "…"-terminated prefix.
        let value = value.trim();
        let head = value.trim_end_matches('…');
        let echoed = !command.is_empty()
            && (value == command || (header_echo && !head.is_empty() && command.starts_with(head)));
        if echoed {
            rest.next();
        }
    }
    let meta = meta.trim();
    rest.filter(|value| {
        let value = value.trim();
        !value.is_empty() && (meta.is_empty() || value != meta)
    })
    .collect::<Vec<_>>()
    .join("\n")
}

pub(super) fn project(tool: &str, payload: &Value, state: &str, fallback: &str) -> Vec<Section> {
    if tool == "上下文压缩" || tool == "context_compaction" {
        return compaction::project(payload, state);
    }
    let result = payload.get("result").unwrap_or(payload);
    let data = super::result_data(payload);
    let args = payload
        .get("args")
        .or_else(|| payload.get("arguments"))
        .unwrap_or(&Value::Null);
    let parsed_args = args
        .as_str()
        .filter(|value| value.len() <= 8192)
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let args = parsed_args.as_ref().unwrap_or(args);
    let failed = matches!(state, "failed" | "error" | "rejected")
        || result["ok"] == false
        || result["success"] == false;
    let pending = matches!(
        state,
        "running" | "queued" | "pending" | "loading" | "streaming"
    );
    let mut sections = structured::project(tool, data, args);
    if !sections.is_empty() {
        // Structured groups already preserve the tool's readable result.
    } else if super::patch::is_patch_tool(tool) {
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
    } else if matches!(
        tool,
        "execute_command" | "run_command" | "shell" | "运行命令" | "command_session"
    ) || tool.contains("执行命令")
    {
        let records = data["results"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(std::slice::from_ref(data));
        for record in records.iter().take(3) {
            let mut command = text(record, &["command", "cmd"]);
            if command.is_empty() {
                command = text(args, &["command", "cmd", "content", "input", "script"]);
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
            for (key, kind) in [
                ("stdout", "context"),
                ("stderr", "error"),
                // Streamed sessions keep the tail in `output`; without it the
                // body fell back to the row's display text.
                ("output", "context"),
            ] {
                if let Some(body) = record[key].as_str() {
                    push_body(&mut section, body, kind);
                }
            }
            if section.lines.is_empty() {
                let body = without_display_echo(fallback, &command, &section.meta);
                if !body.is_empty() {
                    push_body(
                        &mut section,
                        &body,
                        if failed { "error" } else { "context" },
                    );
                }
            }
            sections.push(section);
        }
    } else if tool == "web_search" || tool.contains("网页搜索") {
        // dsh search seam: one section titled by the (joined) query batch, one
        // line per source, and an explicit note when the batch was clipped.
        let queries = data["queries"]
            .as_array()
            .or_else(|| args["queries"].as_array())
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
        let mut meta = status
            .map(|value| format!("HTTP {value}"))
            .unwrap_or_default();
        if truncated {
            if meta.is_empty() {
                meta = "已截断".into();
            } else {
                meta.push_str(" · 已截断");
            }
        }
        let mut section = Section {
            title: if url.is_empty() {
                "网页抓取".into()
            } else {
                url
            },
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
    fn write_file_result_renders_the_same_patch_section() {
        let data = json!({"tool":"write_file","ok":true,"data":{
            "ok":true,"action":"write_file","state":"completed",
            "summary":"Created file sample.txt.","data":{
                "path":"sample.txt","bytes":12,"existed":false,"previous_bytes":0,
                "added_lines":2,"deleted_lines":0,"changed_files":1,
                "files":[{"path":"sample.txt","action":"add","hunks":1,
                    "added_lines":2,"deleted_lines":0,
                    "diff_blocks":[{"header":"new file","lines":[
                        {"kind":"add","new_line":1,"text":"first"},
                        {"kind":"add","new_line":2,"text":"second"},
                    ]}]
                }]
            }
        }});
        let sections = project("写入文件", &data, "completed", "fallback");
        assert_eq!(sections[0].kind, "patch");
        assert_eq!(sections[0].title, "sample.txt");
        assert_eq!(sections[0].meta, "新增  +2 −0");
        assert_eq!(sections[0].lines[1].text, "first");
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
    fn command_detail_never_repeats_the_command_it_already_titles() {
        let command = "cd /work && cargo test --bin demo 2>&1 | tail -8";
        // Pending call: the display text only carries the header and the
        // command, both of which the section prints itself.
        let pending_display = ["执行 · 运行中", command].join("\n");
        let pending = project(
            "execute_command",
            &json!({"args":{"command":command}}),
            "running",
            &pending_display,
        );
        assert_eq!(pending[0].title, format!("$ {command}"));
        assert_eq!(pending[0].meta, "运行中");
        assert!(pending[0].lines.is_empty(), "{:?}", pending[0].lines);
        // Streamed session: the tail arrives in `output`, and neither the
        // header, the command, nor the exit code may be echoed again.
        let streamed = json!({"args":{"command":command},"result":{"data":{
            "command":command,"output":"line one\nline two","status":"exited","exit_code":0
        }}});
        let streamed_display =
            ["执行 · 完成", command, "line one", "line two", "exit 0"].join("\n");
        let sections = project("execute_command", &streamed, "completed", &streamed_display);
        assert_eq!(sections[0].meta, "exit 0");
        let body: Vec<&str> = sections[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(body, vec!["line one", "line two"]);
    }
    #[test]
    fn read_image_details_name_the_region_and_its_numbers() {
        let payload = json!({
            "args":{"path":"shot.png","crop":"0,0,400,300","scale":0.5,"stats":true},
            "result":{"ok":true,"data":{
                "path":"shot.png","media_kind":"image","size_bytes":320_512,
                "view":{"source_width":1536,"source_height":1024,"width":400,"height":300},
                "stats":{"width":400,"height":300,"mean_luma":128.3,
                    "background_rgb":[30,30,30],"non_background_ratio":0.42,"edge_ratio":0.03},
                "result":{"kind":"image","frame_count":1,
                    "attachments":[{"name":"shot_view.png","content_type":"image/png"}]}
            }}
        });
        let sections = project("读图工具", &payload, "completed", "fallback");
        assert_eq!(sections[0].title, "shot.png");
        assert_eq!(sections[0].meta, "图像 1536×1024");
        let body: Vec<&str> = sections[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert!(body.contains(&"体积 313 KB · PNG"));
        assert!(body.contains(&"裁切 0,0,400,300"));
        assert!(body.contains(&"缩放 0.5×"));
        assert!(body.iter().any(|line| line.contains("亮度 128.3")));
        assert!(body.contains(&"输出 shot_view.png"));
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

#[path = "native_workflow_structured.rs"]
mod structured;
