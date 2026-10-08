//! Same authoritative compaction fields as the web; unknown occupancy is never zero.
use super::{line, push_body, Section};
use serde_json::Value;
fn number(data: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| {
            data[*key]
                .as_f64()
                .or_else(|| data[*key].as_str()?.parse().ok())
        })
        .filter(|n| n.is_finite() && *n >= 0.0)
}
pub(super) fn project(data: &Value, state: &str) -> Vec<Section> {
    let before = number(
        data,
        &[
            "projected_request_tokens",
            "total_tokens",
            "context_tokens",
            "context_guard_tokens_before",
        ],
    );
    let unobserved = ["context_usage_source_after", "context_usage_source"]
        .iter()
        .any(|key| {
            data[*key]
                .as_str()
                .is_some_and(|s| s.starts_with("unobserved_after_"))
        });
    let after = if unobserved {
        None
    } else {
        number(
            data,
            &[
                "projected_request_tokens_after",
                "total_tokens_after",
                "context_tokens_after",
                "context_guard_tokens_after",
            ],
        )
    };
    let limit = number(
        data,
        &[
            "max_context",
            "context_max_tokens",
            "maxContext",
            "contextMaxTokens",
        ],
    )
    .or_else(|| {
        number(
            &data["context_usage"],
            &[
                "max_context",
                "context_max_tokens",
                "maxContext",
                "contextMaxTokens",
            ],
        )
    })
    .or_else(|| number(data, &["limit"]));
    let mut sections = Vec::new();
    if before.is_some() || after.is_some() || unobserved {
        let mut usage = Section {
            title: "上下文占用".into(),
            kind: "compaction-usage".into(),
            ..Default::default()
        };
        for (label, kind, count) in [("压缩前", "before", before), ("压缩后", "after", after)]
        {
            let mut row = line(
                match count {
                    Some(n) => format!("{label} {n:.0} tokens"),
                    None if kind == "after" && unobserved => "压缩后等待模型回写".into(),
                    None => format!("{label}：—"),
                },
                kind,
                String::new(),
            );
            row.ratio = count
                .zip(limit.filter(|n| *n > 0.0))
                .map(|(n, cap)| (n / cap).clamp(0.0, 1.0) as f32)
                .unwrap_or(-1.0);
            if let Some((n, cap)) = count.zip(limit.filter(|n| *n > 0.0)) {
                row.text.push_str(&format!("（{:.1}%）", n / cap * 100.0));
            }
            usage.lines.push(row);
        }
        sections.push(usage);
    }
    for (key, title) in [
        ("summary_model_output", "压缩模型输出"),
        ("summary_text", "实际注入上下文"),
    ] {
        if let Some(body) = data[key].as_str().filter(|s| !s.is_empty()) {
            let mut section = Section {
                title: title.into(),
                kind: "compaction-output".into(),
                ..Default::default()
            };
            push_body(&mut section, body, "context");
            sections.push(section);
        }
    }
    let status = data["status"].as_str().unwrap_or(state);
    let running = matches!(
        status,
        "running" | "loading" | "pending" | "streaming" | "queued"
    );
    let failed = matches!(status, "failed" | "error" | "timeout");
    let cancelled = matches!(status, "cancelled" | "canceled");
    let reason = match data["reason"].as_str().unwrap_or("") {
        "manual" => "手动压缩",
        "history" => "历史上下文达到阈值",
        "overflow" => "上下文超限",
        "overflow_recovery" => "上下文超限恢复",
        other => other,
    };
    let result = if running {
        "正在压缩上下文…"
    } else if failed {
        "压缩失败"
    } else if cancelled {
        "已取消压缩"
    } else if status == "skipped" {
        "无需压缩"
    } else if status == "guard_only" {
        "已执行上下文保护"
    } else if data["summary_fallback"] == true {
        "已使用降级摘要"
    } else {
        "压缩完成"
    };
    let mut details = Section {
        title: result.into(),
        kind: "compaction-details".into(),
        ..Default::default()
    };
    if !reason.is_empty() {
        details
            .lines
            .push(line(format!("触发原因：{reason}"), "note", String::new()));
    }
    for (key, title) in [
        ("stage", "阶段"),
        ("message_budget", "消息预算"),
        ("request_overhead_tokens", "请求开销"),
        ("persisted_context_tokens", "持久化基线"),
        ("reset_mode", "重置方式"),
    ] {
        if !data[key].is_null() {
            let value = data[key]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| data[key].to_string());
            details
                .lines
                .push(line(format!("{title}：{value}"), "note", String::new()));
        }
    }
    sections.push(details);
    if failed || cancelled || data["summary_fallback"] == true {
        let mut error = Section {
            title: if cancelled {
                "压缩已停止"
            } else {
                "压缩提示"
            }
            .into(),
            kind: "error".into(),
            ..Default::default()
        };
        for key in [
            "error_code",
            "error_message",
            "message",
            "summary_failure_message",
        ] {
            if let Some(body) = data[key].as_str() {
                push_body(&mut error, body, "error");
            }
        }
        push_body(
            &mut error,
            "可重试压缩；如持续超限，请减少输入或新建线程。",
            "note",
        );
        sections.push(error);
    }
    sections
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compaction_keeps_unknown_usage_and_separates_outputs() {
        let sections = project(
            &json!({"projected_request_tokens":8000,"context_tokens_after":0,"limit":10000,"context_usage_source_after":"unobserved_after_compaction","summary_model_output":"model output","summary_text":"injected summary","reason":"manual"}),
            "completed",
        );
        assert_eq!(sections[0].lines[0].ratio, 0.8);
        assert_eq!(sections[0].lines[1].ratio, -1.0);
        assert!(sections[0].lines[1].text.contains("等待"));
        assert_eq!(sections[1].lines[0].text, "model output");
        assert_eq!(sections[2].lines[0].text, "injected summary");
        assert_eq!(
            project(&json!({"error_message":"fixture failure"}), "failed")
                .last()
                .unwrap()
                .kind,
            "error"
        );
    }
}
