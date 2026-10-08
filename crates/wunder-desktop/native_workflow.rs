//! Shared bounded workflow projection for live events and durable replay.
use super::NativeWorkflowEntry;
use serde_json::Value;

#[path = "native_workflow_patch.rs"]
mod patch;
#[path = "native_workflow_sections.rs"]
mod sections;

impl NativeWorkflowEntry {
    pub fn from_payload(id: &str, title: &str, detail: String, state: &str, data: &Value) -> Self {
        let args = data.get("args").or_else(|| data.get("arguments"));
        let target = args.and_then(|args| {
            ["path", "file_path", "command", "cmd", "query"]
                .iter()
                .find_map(|key| args.get(*key).and_then(Value::as_str))
        });
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
            target.unwrap_or_else(|| detail.lines().nth(1).unwrap_or(""))
        };
        let preview = brief
            .chars()
            .take(640)
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect();
        let tokens = request_consumed_tokens(data)
            .map(|n| format!("{} token", compact(n)))
            .unwrap_or_default();
        let duration = number(
            data,
            &[
                "duration_ms",
                "elapsed_ms",
                "durationMs",
                "elapsedMs",
                "latency_ms",
                "latencyMs",
            ],
            0,
        )
        .map(|ms| {
            if ms < 1000.0 {
                format!("{:.0}ms", ms.floor())
            } else if ms < 10000.0 {
                format!("{:.1}s", ms / 1000.0)
            } else {
                format!("{:.0}s", ms / 1000.0)
            }
        })
        .unwrap_or_default();
        let sections = sections::project(title, data, state, &detail);
        let patch_files = patch::project(title, data, args.unwrap_or(&Value::Null));
        Self {
            id: id.into(),
            title: title.into(),
            preview,
            detail,
            state: state.into(),
            tokens,
            duration,
            sections,
            patch_files,
        }
    }
}

/// Match the web's applyWorkflowMetrics: invocation usage is transport
/// metadata, not a number to search for inside a tool's business result.
fn request_consumed_tokens(data: &Value) -> Option<f64> {
    if let Some(usage) = data.get("request_usage") {
        let value = usage
            .get("total_tokens")
            .filter(|v| !v.is_null())
            .or_else(|| usage.get("total"));
        return value.and_then(count);
    }
    // Presence of runtime metadata (even null) means this is a modern event.
    // Do not substitute context occupancy, result usage or cumulative totals.
    if data.get("request_context_tokens").is_some() {
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
    fn invocation_usage_matches_web_and_ignores_business_metrics() {
        for (payload, expected) in [
            (
                json!({"request_usage":{"total":2400},"request_context_tokens":1800,"data":{"consumed_tokens":99999}}),
                "2.4k token",
            ),
            (
                json!({"request_usage":{"total_tokens":"3200","total":1}}),
                "3.2k token",
            ),
            (
                json!({"request_usage":{"total_tokens":null,"total":1250}}),
                "1.2k token",
            ),
            (json!({"request_usage":{"total":0}}), "0 token"),
            (
                json!({"request_usage":{"total":-1},"consumed_tokens":123}),
                "",
            ),
            (json!({"request_usage":null,"usage":{"total":123}}), ""),
            (
                json!({"request_context_tokens":null,"data":{"consumed_tokens":123}}),
                "",
            ),
            (json!({"request_context_tokens":1800}), ""),
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
            ("example.txt", "2.4k token", "1.2s")
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
}
