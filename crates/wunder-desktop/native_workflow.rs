//! Shared bounded workflow projection for live events and durable replay.
use super::NativeWorkflowEntry;
use serde_json::Value;

#[path = "native_workflow_patch.rs"]
mod patch;
#[path = "native_workflow_sections.rs"]
mod sections;

/// Keep result payloads separate from transport envelopes. In particular image
/// metadata has its own `result` object, which is not another tool envelope.
pub(super) fn result_data(payload: &Value) -> &Value {
    let mut current = payload.get("result").unwrap_or(payload);
    const DISPLAY: &[&str] = &[
        "content",
        "text",
        "output",
        "stdout",
        "stderr",
        "questions",
        "options",
        "plan",
        "media_kind",
        "files",
        "rows",
        "rows_jsonl",
        "chunks",
        "items",
        "items_jsonl",
        "hits",
        "hits_jsonl",
        "matches",
        "sources",
        "jobs",
        "job",
        "scheduler",
    ];
    for _ in 0..5 {
        if let Some(nested) = current.get("data").filter(|value| value.is_object()) {
            current = nested;
        } else if !DISPLAY.iter().any(|key| current.get(*key).is_some()) {
            if let Some(nested) = current.get("result").filter(|value| value.is_object()) {
                current = nested;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    current
}

impl NativeWorkflowEntry {
    pub fn from_payload(id: &str, title: &str, detail: String, state: &str, data: &Value) -> Self {
        let args = data.get("args").or_else(|| data.get("arguments"));
        let compaction = title == "上下文压缩" || title == "context_compaction";
        let brief = if compaction {
            match data["status"].as_str().unwrap_or(state) {
                "running" | "pending" | "loading" | "streaming" => "正在压缩上下文…",
                "failed" | "error" => "压缩失败",
                "cancelled" | "canceled" => "已取消压缩",
                "skipped" => "无需压缩",
                _ => "上下文已压缩",
            }
        } else {
            ""
        };
        let preview = if compaction {
            brief.to_string()
        } else {
            call_preview(title, data)
        }
        .chars()
        .take(640)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
        let tokens = request_context_tokens(data)
            .map(|n| format!("{} token", compact(n)))
            .unwrap_or_default();
        let duration = number(data, &["duration_ms"], 0)
            .or_else(|| {
                number(
                    data.get("meta").unwrap_or(&Value::Null),
                    &["duration_ms"],
                    0,
                )
            })
            .map(|ms| {
                if ms < 1000.0 {
                    format!("{:.0}ms", ms.round())
                } else if ms < 10000.0 {
                    format!("{:.1}s", (ms / 100.0).round() / 10.0)
                } else {
                    format!("{:.0}s", (ms / 1000.0).round())
                }
            })
            .unwrap_or_default();
        let sections = sections::project(title, data, state, &detail);
        let patch_files = patch::project(title, data, args.unwrap_or(&Value::Null));
        Self {
            id: id.into(),
            title: title.into(),
            preview,
            detail: complete_copy_text(data).unwrap_or(detail),
            state: state.into(),
            tokens,
            duration,
            sections,
            patch_files,
        }
    }
}

/// Display sections are bounded independently. Copy keeps the original text
/// instead of silently substituting the clipped 120-line display projection.
fn complete_copy_text(data: &Value) -> Option<String> {
    let result = result_data(data);
    if let Some(value) = result
        .as_str()
        .or_else(|| result["content"].as_str())
        .or_else(|| result["text"].as_str())
        .or_else(|| result["output"].as_str())
    {
        if value.len() <= 2 * 1024 * 1024 {
            return Some(value.into());
        }
    }
    if result.get("stdout").is_some() || result.get("stderr").is_some() {
        let stdout = result["stdout"].as_str().unwrap_or_default();
        let stderr = result["stderr"].as_str().unwrap_or_default();
        if stdout.len().saturating_add(stderr.len()) <= 2 * 1024 * 1024 {
            return Some(if stderr.is_empty() {
                stdout.into()
            } else {
                format!("{stdout}\n{stderr}")
            });
        }
    }
    None
}

/// Skill loading is one tool with one row per call; the row must name the skill
/// being loaded, never the shared entry resource it reads.
fn is_skill_tool(tool: &str) -> bool {
    matches!(tool, "技能调用" | "skill_call" | "skill_get")
}

/// A collapsed row describes invocation arguments only. Results can contain
/// arbitrary `path`, `query` or `content` fields that are not the call target.
fn call_preview(tool: &str, data: &Value) -> String {    let args = data.get("args").or_else(|| data.get("arguments"));
    let parsed = args
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 8192)
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let args = parsed.as_ref().or(args).unwrap_or(&Value::Null);
    let command = matches!(
        tool,
        "execute_command" | "run_command" | "shell" | "运行命令" | "执行命令" | "command_session"
    );
    let keys: &[&str] = if command {
        &["content", "command", "cmd", "input", "script", "raw"]
    } else if is_skill_tool(tool) {
        // A skill call is identified by the skill it loads. The resource path
        // (usually SKILL.md) is an implementation detail of that load and would
        // read identically for every skill in the timeline.
        &["name", "skill_name", "skillName"]
    } else if matches!(tool, "web_fetch" | "webfetch" | "网页抓取") {
        &["url", "uri", "query"]
    } else {
        &[
            "query",
            "question",
            "keyword",
            "path",
            "file_path",
            "file",
            "filename",
            "url",
            "uri",
            "action",
            "operation",
        ]
    };
    if let Some(queries) = args["queries"].as_array() {
        return queries
            .iter()
            .filter_map(Value::as_str)
            .take(8)
            .map(|value| value.chars().take(104).collect::<String>())
            .collect::<Vec<_>>()
            .join(", ")
            .chars()
            .take(640)
            .collect();
    }
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .unwrap_or_default()
        .chars()
        .take(640)
        .collect()
}

/// Match the web timeline's collapsed-row metadata: context occupancy wins,
/// then request usage total, then its input/output sum. These are request
/// metrics, not numbers found in a tool's business result.
fn request_context_tokens(data: &Value) -> Option<f64> {
    if let Some(value) = data.get("request_context_tokens").and_then(positive_count) {
        return Some(value);
    }
    if let Some(usage) = data.get("request_usage").and_then(Value::as_object) {
        if let Some(total) = usage
            .get("total_tokens")
            .and_then(positive_count)
            .or_else(|| usage.get("total").and_then(positive_count))
        {
            return Some(total);
        }
        let input = usage.get("input_tokens").and_then(positive_count);
        let output = usage.get("output_tokens").and_then(positive_count);
        if let Some(total) = input.map(|value| value + output.unwrap_or_default()) {
            return Some(total);
        }
    }
    // A present modern metric (including null/zero) is authoritative. Never
    // replace it with cumulative session counts or tool-result usage.
    if data.get("request_context_tokens").is_some() || data.get("request_usage").is_some() {
        return None;
    }
    let explicit = [
        "request_consumed_tokens",
        "requestConsumedTokens",
        "consumed_tokens",
        "consumedTokens",
        "consumed",
    ];
    for record in [
        Some(data),
        data.get("meta"),
        data.get("payload"),
        data.get("result"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(value) = explicit
            .iter()
            .find_map(|key| record.get(*key).and_then(count))
        {
            return Some(value);
        }
    }
    // Compatibility for older call records only; result usage belongs to the
    // tool and must never be billed as the parent model request's usage.
    if matches!(
        data["event_type"].as_str(),
        Some("tool_result" | "tool_output")
    ) || data.get("result").is_some()
    {
        return None;
    }
    [
        "roundUsage",
        "round_usage",
        "billedUsage",
        "billed_usage",
        "usage",
    ]
    .iter()
    .find_map(|key| {
        let usage = data.get(*key)?;
        [
            "total",
            "total_tokens",
            "totalTokens",
            "input",
            "input_tokens",
            "inputTokens",
        ]
        .iter()
        .find_map(|key| usage.get(*key).and_then(count))
    })
}

fn positive_count(value: &Value) -> Option<f64> {
    count(value).filter(|value| *value > 0.0)
}

fn count(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| {
            let value = value.as_str()?.trim();
            if value.is_empty() {
                return None;
            }
            value.parse().ok()
        })
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(f64::floor)
}

fn number(value: &Value, keys: &[&str], depth: usize) -> Option<f64> {
    if depth > 4 {
        return None;
    }
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                .filter(|n| n.is_finite() && *n >= 0.0)
        })
        .or_else(|| {
            ["data", "result", "usage", "round_usage", "meta", "payload"]
                .iter()
                .find_map(|key| value.get(*key).and_then(|v| number(v, keys, depth + 1)))
        })
}
fn compact(n: f64) -> String {
    let n = n.floor();
    if n < 1000.0 {
        return format!("{n:.0}");
    }
    let (value, suffix) = if n >= 1_000_000.0 {
        (n / 1_000_000.0, "m")
    } else {
        (n / 1000.0, "k")
    };
    let value = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{}{suffix}", value.strip_suffix(".0").unwrap_or(&value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn nested_results_keep_structured_data_and_unclipped_copy_text() {
        let content = (0..160)
            .map(|index| format!("Fixture line {index}\n"))
            .collect::<String>();
        let payload = json!({"result":{"data":{"result":{"content":content}}}});
        let entry = NativeWorkflowEntry::from_payload(
            "fixture-tool",
            "read_file",
            "preview".into(),
            "completed",
            &payload,
        );
        assert_eq!(entry.detail, content);
        assert!(entry.sections[0].lines.len() <= 121);
        assert!(entry.sections[0].lines.last().unwrap().text.contains("120"));
        let image = json!({"result":{"data":{"path":"sample.png","media_kind":"image","size_bytes":128,"result":{"width":8,"height":8}}}});
        assert_eq!(result_data(&image)["path"], "sample.png");
        assert_eq!(result_data(&image)["media_kind"], "image");
    }
    #[test]
    fn invocation_preview_never_reads_result_fields() {
        for (tool, payload, expected) in [
            (
                "web_search",
                json!({"args":{"queries":["sample one", "sample two"]},"result":{"query":"wrong"}}),
                "sample one, sample two",
            ),
            (
                "web_fetch",
                json!({"arguments":"{\"url\":\"https://example.invalid\"}","result":{"url":"wrong"}}),
                "https://example.invalid",
            ),
            (
                "execute_command",
                json!({"args":{"content":"check sample.txt"}}),
                "check sample.txt",
            ),
            (
                "read_file",
                json!({"result":{"path":"wrong","content":"output"}}),
                "",
            ),
            (
                "技能调用",
                json!({
                    "args":{"name":"demo_skill","action":"read","path":"SKILL.md"},
                    "result":{"data":{"path":"SKILL.md"}}
                }),
                "demo_skill",
            ),
        ] {
            let entry = NativeWorkflowEntry::from_payload(
                "fixture-tool",
                tool,
                "result\nwrong".into(),
                "completed",
                &payload,
            );
            assert_eq!(entry.preview, expected);
        }
    }
    #[test]
    fn collapsed_metrics_match_web_and_ignore_business_metrics() {
        for (payload, expected) in [
            (
                json!({"request_usage":{"total":2400},"request_context_tokens":1800,"data":{"consumed_tokens":99999}}),
                "1.8k token",
            ),
            (
                json!({"request_usage":{"total_tokens":"3200","total":1}}),
                "3.2k token",
            ),
            (
                json!({"request_usage":{"total_tokens":null,"total":1250}}),
                "1.2k token",
            ),
            (json!({"request_usage":{"total":0}}), ""),
            (
                json!({"request_usage":{"total":-1},"consumed_tokens":123}),
                "",
            ),
            (json!({"request_usage":null,"usage":{"total":123}}), ""),
            (
                json!({"request_context_tokens":null,"data":{"consumed_tokens":123}}),
                "",
            ),
            (json!({"request_context_tokens":1800}), "1.8k token"),
            (
                json!({"args":{"consumed_tokens":123},"data":{"usage":{"total":456}}}),
                "",
            ),
            (
                json!({"event_type":"tool_result","usage":{"total":123}}),
                "",
            ),
        ] {
            let row = NativeWorkflowEntry::from_payload(
                "fixture-tool",
                "read_file",
                String::new(),
                "completed",
                &payload,
            );
            assert_eq!(row.tokens, expected, "{payload}");
        }
    }
    #[test]
    fn metadata_uses_own_call_and_missing_values_stay_empty() {
        let row = NativeWorkflowEntry::from_payload(
            "tool-1",
            "read_file",
            "Read · done\nresult".into(),
            "completed",
            &json!({"args":{"path":"example.txt"},"result":{"request_consumed_tokens":2400,"duration_ms":1250}}),
        );
        assert_eq!(
            (
                row.preview.as_str(),
                row.tokens.as_str(),
                row.duration.as_str()
            ),
            ("example.txt", "2.4k token", "1.3s")
        );
        let empty = NativeWorkflowEntry::from_payload(
            "tool-2",
            "read_file",
            String::new(),
            "running",
            &Value::Null,
        );
        assert!(empty.tokens.is_empty() && empty.duration.is_empty());
    }

    #[test]
    fn collapsed_metrics_prefer_request_context_then_usage_and_meta_duration() {
        for (payload, expected) in [
            (
                json!({"request_context_tokens":1800,"request_usage":{"total":2400},"meta":{"duration_ms":19}}),
                ("1.8k token", "19ms"),
            ),
            (
                json!({"request_usage":{"input_tokens":1100,"output_tokens":120},"meta":{"duration_ms":1999}}),
                ("1.2k token", "2.0s"),
            ),
            (
                json!({"result":{"usage":{"total":9999}},"meta":{"duration_ms":20000}}),
                ("", "20s"),
            ),
        ] {
            let row = NativeWorkflowEntry::from_payload(
                "fixture-tool",
                "read_file",
                String::new(),
                "completed",
                &payload,
            );
            assert_eq!((row.tokens.as_str(), row.duration.as_str()), expected);
        }
    }
}
