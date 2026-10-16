//! Readable native equivalents of the web tool-result groups. Array and text
//! budgets apply before projection; binary media stays out of the UI model.
use super::{line, push_body, text, Section};
use serde_json::Value;

fn section(title: String, kind: &str) -> Section {
    Section {
        title,
        kind: kind.into(),
        ..Default::default()
    }
}

fn rows(value: &Value, array: &str, jsonl: &str, limit: usize) -> Vec<Value> {
    if let Some(values) = value[array].as_array() {
        return values.iter().take(limit).cloned().collect();
    }
    value[jsonl]
        .as_str()
        .unwrap_or_default()
        .lines()
        .take(limit)
        .map(|row| {
            serde_json::from_str(row)
                .unwrap_or_else(|_| Value::String(row.chars().take(2048).collect()))
        })
        .collect()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.chars().take(2048).collect(),
        Value::Null => String::new(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        _ => String::new(),
    }
}

fn status(value: &str) -> &str {
    match value {
        "completed" | "done" | "complete" | "ok" => "已完成",
        "in_progress" | "running" => "进行中",
        "pending" | "awaiting_input" => "待处理",
        other => other,
    }
}

pub(super) fn project(tool: &str, data: &Value, args: &Value) -> Vec<Section> {
    match tool {
        "read_file" | "读取文件" => {
            let body = data["content"]
                .as_str()
                .or_else(|| data["text"].as_str())
                .unwrap_or_default();
            if !body.starts_with(">>> ") {
                return Vec::new();
            }
            let mut groups = Vec::new();
            for row in body.lines() {
                if let Some(path) = row.strip_prefix(">>> ") {
                    if groups.len() == 8 {
                        break;
                    }
                    groups.push(section(path.chars().take(2048).collect(), "text"));
                } else if let Some(group) = groups.last_mut() {
                    if group.lines.len() < 120 {
                        group.lines.push(line(
                            row.chars().take(2048).collect(),
                            "context",
                            String::new(),
                        ));
                    } else if group.lines.len() == 120 {
                        group
                            .lines
                            .push(line("仅展示前 120 行".into(), "note", String::new()));
                    }
                }
            }
            groups
        }
        "schedule_task" | "定时任务" => {
            let mut groups = Vec::new();
            for job in data["jobs"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| {
                    if data["job"].is_object() {
                        std::slice::from_ref(&data["job"])
                    } else {
                        &[]
                    }
                })
                .iter()
                .take(16)
            {
                let title = text(job, &["name", "job_id"]);
                let mut group = section(
                    if title.is_empty() {
                        "定时任务".into()
                    } else {
                        title
                    },
                    "schedule",
                );
                group.meta = if job["running"] == true {
                    "运行中"
                } else if job["enabled"] == false {
                    "已停用"
                } else {
                    "等待执行"
                }
                .into();
                let schedule = &job["schedule"];
                let rule = match schedule["kind"].as_str().unwrap_or_default() {
                    "every" => schedule["every_ms"]
                        .as_f64()
                        .map(|ms| format!("每 {} 秒", ms / 1000.0))
                        .unwrap_or_default(),
                    "at" => value_text(&schedule["at"]),
                    _ => format!("{} {}", text(schedule, &["cron"]), text(schedule, &["tz"]))
                        .trim()
                        .to_string(),
                };
                for (label, value) in [
                    ("规则", rule),
                    ("下次执行", value_text(&job["next_run_at"])),
                    ("上次结果", text(job, &["last_status"])),
                    ("错误", text(job, &["last_error"])),
                ] {
                    if !value.is_empty() {
                        push_body(
                            &mut group,
                            &format!("{label}：{value}"),
                            if label == "错误" {
                                "error"
                            } else {
                                "context"
                            },
                        );
                    }
                }
                groups.push(group);
            }
            if groups.is_empty() && !data.is_null() {
                let mut group = section("定时任务".into(), "schedule");
                group.meta = text(data, &["action"]);
                push_body(
                    &mut group,
                    &text(data, &["reason", "last_error"]),
                    "context",
                );
                if let Some(enabled) = data["scheduler"]["enabled"].as_bool() {
                    push_body(
                        &mut group,
                        if enabled {
                            "调度器：已启用"
                        } else {
                            "调度器：已停用"
                        },
                        "context",
                    );
                }
                if group.lines.is_empty() {
                    push_body(&mut group, "暂无任务", "note");
                }
                groups.push(group);
            }
            groups
        }
        "question_panel" | "ask_panel" | "question panel" | "问询面板" => {
            let source = if data["questions"].is_array() || data["options"].is_array() {
                data
            } else {
                args
            };
            source["questions"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(source))
                .iter()
                .take(4)
                .filter_map(|question| {
                    let title = text(question, &["question"]);
                    if title.is_empty() {
                        return None;
                    }
                    let mut group = section(title, "question");
                    group.meta = if question["multiple"] == true {
                        "多选"
                    } else {
                        "单选"
                    }
                    .into();
                    for option in question["options"].as_array().into_iter().flatten().take(4) {
                        let label = option
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| text(option, &["label"]));
                        if label.is_empty() {
                            continue;
                        }
                        group.lines.push(line(
                            format!(
                                "{}{}",
                                label,
                                if option["recommended"] == true {
                                    " · 推荐"
                                } else {
                                    ""
                                }
                            ),
                            "context",
                            String::new(),
                        ));
                        let description = text(option, &["description"]);
                        if !description.is_empty() {
                            push_body(&mut group, &description, "note");
                        }
                    }
                    Some(group)
                })
                .collect()
        }
        "update_plan" | "plan_update" | "计划面板" => {
            let source = if data["plan"].is_array() { data } else { args };
            let mut group = section("计划".into(), "plan");
            group.meta = text(source, &["explanation"]);
            for step in source["plan"].as_array().into_iter().flatten().take(12) {
                let label = text(step, &["step"]);
                if !label.is_empty() {
                    group.lines.push(line(
                        format!(
                            "{} · {}",
                            status(step["status"].as_str().unwrap_or("pending")),
                            label
                        ),
                        "context",
                        String::new(),
                    ));
                }
            }
            if group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        "read_image" | "view_image" | "读图" => {
            let path = text(data, &["path", "file_path"]);
            let mut group = section(
                if path.is_empty() {
                    text(args, &["path", "file_path"])
                } else {
                    path
                },
                "media",
            );
            for (label, value) in [
                ("类型", &data["media_kind"]),
                ("字节", &data["size_bytes"]),
                ("帧数", &data["media"]["frame_count"]),
                ("时长（ms）", &data["media"]["duration_ms"]),
            ] {
                let value = value_text(value);
                if !value.is_empty() {
                    group
                        .lines
                        .push(line(format!("{label}：{value}"), "context", String::new()));
                }
            }
            for warning in data["media"]["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .take(4)
            {
                push_body(&mut group, &value_text(warning), "note");
            }
            if group.title.is_empty() && group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        "list_files" | "list_file" | "列出文件" | "glob" | "glob_files" | "文件名匹配" => {
            let mut group = section(text(data, &["path", "root"]), "files");
            for item in rows(data, "items", "items_jsonl", 80) {
                let path = if item.is_string() {
                    value_text(&item)
                } else {
                    text(&item, &["path", "file", "file_path", "name"])
                };
                if path.is_empty() {
                    continue;
                }
                group.lines.push(line(path, "context", String::new()));
            }
            if let Some(count) = data["items_count"].as_u64() {
                group.meta = format!("{count} 项");
            }
            if group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        "search_content" | "search_files" | "搜索内容" => {
            let hits = if data["hits"].is_array() || data["hits_jsonl"].is_string() {
                rows(data, "hits", "hits_jsonl", 60)
            } else {
                rows(data, "matches", "matches_jsonl", 60)
            };
            let mut group = section(text(args, &["query", "pattern"]), "search");
            for hit in hits {
                if hit.is_string() {
                    push_body(&mut group, &value_text(&hit), "context");
                    continue;
                }
                let path = text(&hit, &["path", "file", "file_path"]);
                let number = hit["line"].as_i64().or_else(|| hit["line_number"].as_i64());
                let body = text(&hit, &["text", "content", "preview", "line_text"]);
                group.lines.push(line(
                    format!(
                        "{path}{}  {body}",
                        number.map(|n| format!(":{n}")).unwrap_or_default()
                    ),
                    "context",
                    String::new(),
                ));
            }
            if group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        _ if tool == "db_query" || tool.starts_with("db_query_") || tool.contains("@db_query") => {
            let mut group = section("查询结果".into(), "table");
            let records = rows(data, "rows", "rows_jsonl", 30);
            let columns: Vec<_> = data["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .take(12)
                .map(|column| {
                    column
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| text(column, &["name", "column"]))
                })
                .filter(|column| !column.is_empty())
                .collect();
            group.meta = format!(
                "{} 行 · {} 列",
                data["row_count"].as_u64().unwrap_or(records.len() as u64),
                columns.len()
            );
            if !columns.is_empty() {
                group
                    .lines
                    .push(line(columns.join(" | "), "header", String::new()));
            }
            for record in records {
                let cells: Vec<_> = if let Some(values) = record.as_array() {
                    values.iter().take(12).map(value_text).collect()
                } else if !columns.is_empty() {
                    columns
                        .iter()
                        .map(|column| value_text(&record[column]))
                        .collect()
                } else {
                    record
                        .as_object()
                        .into_iter()
                        .flat_map(|map| map.iter().take(12))
                        .map(|(key, value)| format!("{key}: {}", value_text(value)))
                        .collect()
                };
                group
                    .lines
                    .push(line(cells.join(" | "), "context", String::new()));
            }
            if group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        _ if tool == "kb_query" || tool.starts_with("kb_query_") || tool.contains("@kb_query") => {
            let mut group = section("检索结果".into(), "knowledge");
            let answer = text(data, &["answer", "text"]);
            if !answer.is_empty() {
                push_body(&mut group, &answer, "context");
            }
            for hit in rows(data, "chunks", "chunks_jsonl", 12) {
                let title = text(&hit, &["document_name", "document", "title"]);
                if !title.is_empty() {
                    push_body(&mut group, &title, "header");
                }
                push_body(
                    &mut group,
                    &text(&hit, &["highlight", "content", "text", "answer"]),
                    "context",
                );
            }
            if group.lines.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn question_plan_search_and_table_preserve_readable_structure() {
        let question = project(
            "question_panel",
            &Value::Null,
            &json!({"questions":[{"question":"Fixture question", "multiple":true,"options":[{"label":"Choice", "description":"Detail", "recommended":true}]}]}),
        );
        assert_eq!(question[0].meta, "多选");
        assert!(question[0].lines[0].text.contains("推荐"));
        assert_eq!(question[0].lines[1].text, "Detail");
        let plan = project(
            "update_plan",
            &json!({"plan":[{"step":"Fixture step", "status":"completed"}]}),
            &Value::Null,
        );
        assert_eq!(plan[0].lines[0].text, "已完成 · Fixture step");
        let hits = project(
            "search_content",
            &json!({"hits_jsonl":"{\"path\":\"sample.txt\",\"line\":5,\"text\":\"match\"}"}),
            &Value::Null,
        );
        assert_eq!(hits[0].lines[0].text, "sample.txt:5  match");
        let table = project(
            "db_query",
            &json!({"columns":["value"],"rows":[{"value":0}]}),
            &Value::Null,
        );
        assert_eq!(table[0].lines[1].text, "0");
        let files = project(
            "read_file",
            &json!({"content":">>> sample.txt\nfirst\n>>> other.txt\nsecond"}),
            &Value::Null,
        );
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].title, "other.txt");
        assert_eq!(files[1].lines[0].text, "second");
        let jobs = project(
            "schedule_task",
            &json!({"jobs":[{"name":"Fixture task","enabled":true,"schedule":{"kind":"every","every_ms":60000}}]}),
            &Value::Null,
        );
        assert_eq!(jobs[0].meta, "等待执行");
        assert_eq!(jobs[0].lines[0].text, "规则：每 60 秒");
    }
}
