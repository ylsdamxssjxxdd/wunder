use super::context::ToolContext;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) const TOOL_SELF_STATUS: &str = "\u{81ea}\u{6211}\u{72b6}\u{6001}";
pub(crate) const TOOL_SELF_STATUS_ALIAS: &str = "self_status";

const DEFAULT_EVENTS_LIMIT: usize = 20;
const MAX_EVENTS_LIMIT: usize = 200;

#[derive(Debug, Deserialize, Default)]
struct SelfStatusArgs {
    #[serde(default, alias = "detailLevel")]
    detail_level: Option<String>,
    #[serde(default, alias = "includeEvents")]
    include_events: Option<bool>,
    #[serde(default, alias = "eventsLimit")]
    events_limit: Option<usize>,
    #[serde(default, alias = "includeSystemMetrics")]
    include_system_metrics: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailLevel {
    Basic,
    Standard,
    Full,
}

impl DetailLevel {
    fn from_raw(raw: Option<&str>) -> Self {
        let normalized = raw.unwrap_or("standard").trim().to_ascii_lowercase();
        match normalized.as_str() {
            "basic" | "lite" | "summary" => Self::Basic,
            "full" | "detail" | "detailed" | "verbose" => Self::Full,
            _ => Self::Standard,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }
}

#[derive(Default)]
struct EventStats {
    total: usize,
    by_type: BTreeMap<String, usize>,
    llm_request_count: usize,
    tool_call_count: usize,
    tool_result_count: usize,
    approval_request_count: usize,
    approval_resolved_count: usize,
    latest_user_round: Option<i64>,
    latest_model_round: Option<i64>,
    last_event_id: Option<i64>,
}

pub(crate) async fn execute_self_status_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: SelfStatusArgs = if args.is_null() {
        SelfStatusArgs::default()
    } else {
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?
    };
    let detail_level = DetailLevel::from_raw(payload.detail_level.as_deref());
    let include_events = payload
        .include_events
        .unwrap_or(matches!(detail_level, DetailLevel::Full));
    let include_system_metrics = payload
        .include_system_metrics
        .unwrap_or(matches!(detail_level, DetailLevel::Full));
    let events_limit = clamp_events_limit(payload.events_limit);

    let monitor_record = context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(context.session_id));
    let monitor_events = monitor_record
        .as_ref()
        .and_then(|record| record.get("events"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let event_stats = collect_event_stats(monitor_events);
    let configured_model = configured_model_info(context);
    let active_model = active_model_info(monitor_events);

    let workspace_context_tokens = context
        .workspace
        .load_session_context_tokens(context.user_id, context.session_id);
    let workspace_context_overflow = context
        .workspace
        .load_session_context_overflow(context.user_id, context.session_id);
    let runtime_snapshot = context.orchestrator.as_ref().and_then(|orchestrator| {
        orchestrator.get_tool_session_runtime_snapshot(context.session_id)
    });

    let monitor_status = monitor_record
        .as_ref()
        .and_then(|record| normalize_optional_string(record.get("status").and_then(Value::as_str)));
    let monitor_stage = monitor_record
        .as_ref()
        .and_then(|record| normalize_optional_string(record.get("stage").and_then(Value::as_str)));
    let monitor_summary = monitor_record.as_ref().and_then(|record| {
        normalize_optional_string(record.get("summary").and_then(Value::as_str))
    });
    let monitor_updated_time = monitor_record
        .as_ref()
        .and_then(|record| record.get("updated_time"))
        .cloned();

    let monitor_context_tokens = monitor_record
        .as_ref()
        .and_then(|record| value_to_i64(record.get("context_tokens")));
    let monitor_context_tokens_peak = monitor_record
        .as_ref()
        .and_then(|record| value_to_i64(record.get("context_tokens_peak")));
    let user_rounds = monitor_record
        .as_ref()
        .and_then(|record| value_to_i64(record.get("user_rounds")));

    let mut response = json!({
        "tool": TOOL_SELF_STATUS_ALIAS,
        "detail_level": detail_level.as_str(),
        "session": {
            "user_id": context.user_id,
            "session_id": context.session_id,
            "workspace_id": context.workspace_id,
            "agent_id": context.agent_id,
            "is_admin": context.is_admin,
        },
        "model": {
            "active": active_model,
            "configured_default": configured_model,
        },
        "context": {
            "context_occupancy_tokens": workspace_context_tokens,
            "context_overflow": workspace_context_overflow,
            "monitor_context_tokens": monitor_context_tokens,
            "monitor_context_tokens_peak": monitor_context_tokens_peak,
        },
        "monitor": {
            "status": monitor_status,
            "stage": monitor_stage,
            "summary": monitor_summary,
            "updated_time": monitor_updated_time,
        },
        "thread": runtime_snapshot,
        "rounds": {
            "user_rounds": user_rounds,
            "model_rounds_peak": event_stats.latest_model_round,
            "event_user_round_peak": event_stats.latest_user_round,
        },
        "events": {
            "total": event_stats.total,
            "last_event_id": event_stats.last_event_id,
            "counts": {
                "llm_request": event_stats.llm_request_count,
                "tool_call": event_stats.tool_call_count,
                "tool_result": event_stats.tool_result_count,
                "approval_request": event_stats.approval_request_count,
                "approval_resolved": event_stats.approval_resolved_count,
            },
            "by_type": event_stats.by_type,
        },
    });

    if include_events {
        let include_data = matches!(detail_level, DetailLevel::Full);
        let recent_events = collect_recent_events(monitor_events, events_limit, include_data);
        if let Value::Object(ref mut map) = response {
            if let Some(events) = map.get_mut("events").and_then(Value::as_object_mut) {
                events.insert("recent_limit".to_string(), json!(events_limit));
                events.insert("recent".to_string(), Value::Array(recent_events));
            }
        }
    }

    if matches!(detail_level, DetailLevel::Basic) {
        if let Value::Object(ref mut map) = response {
            if let Some(events) = map.get_mut("events").and_then(Value::as_object_mut) {
                events.remove("by_type");
            }
        }
    }

    if include_system_metrics {
        let system_metrics = context
            .monitor
            .as_ref()
            .and_then(|monitor| serde_json::to_value(monitor.get_system_metrics()).ok())
            .unwrap_or(Value::Null);
        if let Value::Object(ref mut map) = response {
            map.insert("system_metrics".to_string(), system_metrics);
        }
    }

    Ok(response)
}

fn clamp_events_limit(raw: Option<usize>) -> usize {
    raw.unwrap_or(DEFAULT_EVENTS_LIMIT)
        .clamp(1, MAX_EVENTS_LIMIT)
}

fn configured_model_info(context: &ToolContext<'_>) -> Value {
    let default_name = context.config.llm.default.trim().to_string();
    if default_name.is_empty() {
        return Value::Null;
    }
    let model_config = context.config.llm.models.get(&default_name);
    json!({
        "profile": default_name,
        "provider": model_config
            .and_then(|item| normalize_optional_string(item.provider.as_deref())),
        "model": model_config.and_then(|item| normalize_optional_string(item.model.as_deref())),
        "base_url": model_config
            .and_then(|item| normalize_optional_string(item.base_url.as_deref())),
    })
}

fn active_model_info(events: &[Value]) -> Value {
    for event in events.iter().rev() {
        if event_type(event) != Some("llm_request") {
            continue;
        }
        let data = event.get("data").and_then(Value::as_object);
        let model = data
            .and_then(|item| item.get("model"))
            .and_then(Value::as_str)
            .and_then(|item| normalize_optional_string(Some(item)));
        let provider = data
            .and_then(|item| item.get("provider"))
            .and_then(Value::as_str)
            .and_then(|item| normalize_optional_string(Some(item)));
        let base_url = data
            .and_then(|item| item.get("base_url"))
            .and_then(Value::as_str)
            .and_then(|item| normalize_optional_string(Some(item)));
        if model.is_none() && provider.is_none() && base_url.is_none() {
            continue;
        }
        return json!({
            "model": model,
            "provider": provider,
            "base_url": base_url,
            "source": "monitor.llm_request",
            "event_id": value_to_i64(event.get("event_id")),
        });
    }
    Value::Null
}

fn collect_event_stats(events: &[Value]) -> EventStats {
    let mut stats = EventStats {
        total: events.len(),
        ..EventStats::default()
    };
    for event in events {
        if let Some(event_id) = value_to_i64(event.get("event_id")) {
            stats.last_event_id = Some(
                stats
                    .last_event_id
                    .map_or(event_id, |current| current.max(event_id)),
            );
        }
        let Some(event_type) = event_type(event) else {
            continue;
        };
        *stats.by_type.entry(event_type.to_string()).or_default() += 1;
        match event_type {
            "llm_request" => stats.llm_request_count += 1,
            "tool_call" => stats.tool_call_count += 1,
            "tool_result" => stats.tool_result_count += 1,
            "approval_request" => stats.approval_request_count += 1,
            "approval_resolved" => stats.approval_resolved_count += 1,
            _ => {}
        }
        if let Some(data) = event.get("data").and_then(Value::as_object) {
            update_round_peak(
                &mut stats.latest_user_round,
                value_to_i64(data.get("user_round")),
            );
            update_round_peak(
                &mut stats.latest_model_round,
                value_to_i64(data.get("model_round")),
            );
        }
    }
    stats
}

fn collect_recent_events(events: &[Value], limit: usize, include_data: bool) -> Vec<Value> {
    let start = events.len().saturating_sub(limit);
    events[start..]
        .iter()
        .map(|event| {
            let mut item = Map::new();
            if let Some(event_id) = value_to_i64(event.get("event_id")) {
                item.insert("event_id".to_string(), json!(event_id));
            }
            if let Some(timestamp) = event.get("timestamp") {
                item.insert("timestamp".to_string(), timestamp.clone());
            }
            if let Some(event_type) = event_type(event) {
                item.insert("type".to_string(), Value::String(event_type.to_string()));
            }
            if let Some(data) = event.get("data").and_then(Value::as_object) {
                if let Some(summary) = data
                    .get("summary")
                    .and_then(Value::as_str)
                    .and_then(|value| normalize_optional_string(Some(value)))
                {
                    item.insert("summary".to_string(), Value::String(summary));
                }
                if let Some(user_round) = value_to_i64(data.get("user_round")) {
                    item.insert("user_round".to_string(), json!(user_round));
                }
                if let Some(model_round) = value_to_i64(data.get("model_round")) {
                    item.insert("model_round".to_string(), json!(model_round));
                }
                if include_data {
                    item.insert("data".to_string(), Value::Object(data.clone()));
                }
            }
            Value::Object(item)
        })
        .collect()
}

fn event_type(event: &Value) -> Option<&str> {
    event
        .get("type")
        .or_else(|| event.get("event_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn value_to_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().and_then(|raw| i64::try_from(raw).ok()))
    })
}

fn normalize_optional_string(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn update_round_peak(target: &mut Option<i64>, incoming: Option<i64>) {
    let Some(incoming) = incoming else {
        return;
    };
    *target = Some(target.map_or(incoming, |current| current.max(incoming)));
}

#[cfg(test)]
mod tests {
    use super::{collect_event_stats, DetailLevel, TOOL_SELF_STATUS_ALIAS};
    use serde_json::json;

    #[test]
    fn detail_level_defaults_to_standard() {
        assert_eq!(DetailLevel::from_raw(None).as_str(), "standard");
        assert_eq!(DetailLevel::from_raw(Some("basic")).as_str(), "basic");
        assert_eq!(DetailLevel::from_raw(Some("full")).as_str(), "full");
    }

    #[test]
    fn event_stats_collect_rounds_and_counts() {
        let events = vec![
            json!({
                "event_id": 1,
                "type": "llm_request",
                "data": { "user_round": 2, "model_round": 1 }
            }),
            json!({
                "event_id": 2,
                "type": "tool_call",
                "data": { "user_round": 2, "model_round": 2 }
            }),
            json!({
                "event_id": 3,
                "type": "approval_resolved",
                "data": { "user_round": 2, "model_round": 2 }
            }),
        ];

        let stats = collect_event_stats(&events);
        assert_eq!(stats.total, 3);
        assert_eq!(stats.llm_request_count, 1);
        assert_eq!(stats.tool_call_count, 1);
        assert_eq!(stats.approval_resolved_count, 1);
        assert_eq!(stats.latest_user_round, Some(2));
        assert_eq!(stats.latest_model_round, Some(2));
        assert_eq!(stats.last_event_id, Some(3));
    }

    #[test]
    fn alias_is_stable() {
        assert_eq!(TOOL_SELF_STATUS_ALIAS, "self_status");
    }
}
