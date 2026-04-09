use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use serde_json::{json, Map, Value};

const AGENT_SWARM_TOOL_NAME: &str = "agent_swarm";
const AGENT_SWARM_TOOL_LABEL: &str = "智能体蜂群";

pub(crate) fn agent_swarm_send_example() -> Value {
    json!({
        "action": "send",
        "agentName": "worker_a",
        "message": "请完成指定任务。"
    })
}

pub(crate) fn agent_swarm_batch_send_example() -> Value {
    json!({
        "action": "batch_send",
        "tasks": [
            {
                "agentName": "worker_a",
                "message": "请完成任务 A。"
            },
            {
                "agentName": "worker_b",
                "message": "请完成任务 B。"
            }
        ]
    })
}

pub(crate) fn agent_swarm_wait_example() -> Value {
    json!({
        "action": "wait",
        "runIds": ["run_demo_1"]
    })
}

pub(crate) fn agent_swarm_spawn_example() -> Value {
    json!({
        "action": "spawn",
        "agentName": "worker_a",
        "task": "请完成指定任务。"
    })
}

pub(crate) fn build_agent_swarm_args_failure(
    action: &str,
    code: &str,
    summary: impl Into<String>,
    hint: impl Into<String>,
    missing_fields: &[&str],
    example: Value,
    received_args: &Value,
    extra: Value,
) -> Value {
    let summary = summary.into();
    let hint = hint.into();
    let mut data = Map::new();
    data.insert(
        "tool".to_string(),
        Value::String(AGENT_SWARM_TOOL_LABEL.to_string()),
    );
    data.insert(
        "tool_name".to_string(),
        Value::String(AGENT_SWARM_TOOL_NAME.to_string()),
    );
    data.insert("action".to_string(), Value::String(action.to_string()));
    data.insert(
        "phase".to_string(),
        Value::String("args_validation".to_string()),
    );
    data.insert(
        "failure_summary".to_string(),
        Value::String(summary.clone()),
    );
    data.insert("next_step_hint".to_string(), Value::String(hint.clone()));
    if !missing_fields.is_empty() {
        data.insert(
            "missing_fields".to_string(),
            Value::Array(
                missing_fields
                    .iter()
                    .map(|field| Value::String((*field).to_string()))
                    .collect(),
            ),
        );
    }
    data.insert("example".to_string(), example);
    data.insert(
        "received_args".to_string(),
        compact_received_args(received_args),
    );
    if let Some(extra_obj) = extra.as_object() {
        for (key, value) in extra_obj {
            data.insert(key.clone(), value.clone());
        }
    }

    build_failed_tool_result(
        summary,
        Value::Object(data),
        ToolErrorMeta::new(code, Some(hint), false, None),
        false,
    )
}

fn compact_received_args(value: &Value) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };
    let mut compacted = map.clone();
    if let Some(tasks) = map.get("tasks").and_then(Value::as_array) {
        let preview = tasks.iter().take(2).cloned().collect::<Vec<_>>();
        compacted.insert("tasks".to_string(), Value::Array(preview));
        if tasks.len() > 2 {
            compacted.insert(
                "omitted_task_count".to_string(),
                json!(tasks.len().saturating_sub(2)),
            );
        }
    }
    Value::Object(compacted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_agent_swarm_args_failure_keeps_example_and_received_preview() {
        let payload = build_agent_swarm_args_failure(
            "batch_send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm batch_send task[0] requires message",
            "请为每个 task 提供非空 message。",
            &["tasks[0].message"],
            agent_swarm_batch_send_example(),
            &json!({
                "action": "batch_send",
                "tasks": [{}, {}, {}]
            }),
            json!({
                "task_index": 0
            }),
        );
        assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            payload.pointer("/error_meta/code").and_then(Value::as_str),
            Some("TOOL_ARGS_MISSING_FIELD")
        );
        assert_eq!(
            payload.pointer("/data/task_index").and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            payload
                .pointer("/data/received_args/omitted_task_count")
                .and_then(Value::as_u64),
            Some(1)
        );
    }
}
