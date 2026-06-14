use super::{
    build_model_tool_success, build_model_tool_success_with_hint, catalog::yaml_to_json,
    context::ToolContext,
};
use crate::a2a_store::A2aTask;
use crate::config::{A2aServiceConfig, Config};
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use uuid::Uuid;

#[derive(Clone)]
struct A2aTaskSnapshot {
    task_id: String,
    context_id: Option<String>,
    status: Option<String>,
    endpoint: Option<String>,
    service_name: Option<String>,
    answer: Option<String>,
    updated_time: Option<String>,
    refresh_error: Option<String>,
}

impl A2aTaskSnapshot {
    fn to_value(&self) -> Value {
        json!({
            "task_id": self.task_id,
            "context_id": self.context_id,
            "status": self.status,
            "endpoint": self.endpoint,
            "service_name": self.service_name,
            "answer": self.answer,
            "updated_time": self.updated_time,
            "refresh_error": self.refresh_error,
        })
    }

    fn is_done(&self) -> bool {
        self.status
            .as_deref()
            .map(is_a2a_task_finished)
            .unwrap_or(false)
    }
}

struct A2aTaskInfo {
    id: String,
    context_id: Option<String>,
    status: Option<String>,
    answer: Option<String>,
}

struct A2aObserveSnapshot {
    tasks: Vec<A2aTaskSnapshot>,
    pending: Vec<A2aTaskSnapshot>,
}

fn build_a2a_snapshot_success(
    action: &str,
    snapshot: &A2aObserveSnapshot,
    elapsed_s: Option<f64>,
    timed_out: bool,
) -> Value {
    let done = snapshot.pending.is_empty();
    let total = snapshot.tasks.len();
    let pending = snapshot.pending.len();
    let mut data = json!({
        "tasks": snapshot.tasks.iter().map(A2aTaskSnapshot::to_value).collect::<Vec<_>>(),
        "pending": snapshot.pending.iter().map(A2aTaskSnapshot::to_value).collect::<Vec<_>>(),
        "done": done,
        "total": total,
        "pending_total": pending,
    });
    if let Some(map) = data.as_object_mut() {
        if let Some(elapsed_s) = elapsed_s {
            map.insert("elapsed_s".to_string(), json!(elapsed_s));
        }
        if action == "a2a_wait" {
            map.insert("timeout".to_string(), Value::Bool(timed_out));
        }
    }
    let summary = if action == "a2a_wait" {
        if done {
            format!("All {total} A2A tasks finished.")
        } else if timed_out {
            format!("{pending} of {total} A2A tasks are still pending after waiting.")
        } else {
            format!("{pending} of {total} A2A tasks are still pending.")
        }
    } else if done {
        format!("Observed {total} A2A tasks; all finished.")
    } else {
        format!("Observed {total} A2A tasks; {pending} still pending.")
    };
    build_model_tool_success_with_hint(
        action,
        if done { "completed" } else { "running" },
        summary,
        data,
        (!done).then(|| {
            "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
                .to_string()
        }),
    )
}

pub(crate) fn is_a2a_service_tool(name: &str) -> bool {
    name.starts_with("a2a@") && name.len() > "a2a@".len()
}

/// 调用 A2A 服务执行任务，并将结果写入任务存储。
pub(crate) async fn execute_a2a_service_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let service_name = name.trim_start_matches("a2a@");
    let service = resolve_a2a_service(context.config, service_name, "")
        .ok_or_else(|| anyhow!("A2A 服务不存在: {service_name}"))?;
    if !service.enabled {
        return Err(anyhow!("A2A 服务已禁用: {service_name}"));
    }
    let content = extract_text_arg(args, &["content", "task", "message", "text"])
        .ok_or_else(|| anyhow!("A2A 任务内容不能为空"))?;
    let session_id = extract_text_arg(args, &["session_id", "context_id", "task_id"]);
    let user_id = service
        .user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(context.user_id);
    let mut message = json!({
        "parts": [
            { "text": content }
        ]
    });
    if let Some(session_id) = session_id.as_ref() {
        message["taskId"] = Value::String(session_id.clone());
        message["contextId"] = Value::String(session_id.clone());
    }
    let mut params = json!({ "message": message });
    if !user_id.trim().is_empty() {
        params["userId"] = Value::String(user_id.to_string());
    }
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "SendMessage",
        "params": params
    });
    let headers = build_a2a_headers(context.config, service)?;
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);
    let response = send_a2a_request(
        context.http,
        &service.endpoint,
        headers,
        &payload,
        timeout_s,
    )
    .await?;
    let info = parse_a2a_task_info(&response).ok_or_else(|| anyhow!("A2A 返回缺少任务信息"))?;
    let now = Utc::now();
    context.a2a_store.insert(A2aTask {
        id: info.id.clone(),
        user_id: context.user_id.to_string(),
        status: info.status.clone().unwrap_or_default(),
        context_id: info.context_id.clone(),
        endpoint: Some(service.endpoint.clone()),
        service_name: Some(service.name.clone()),
        method: Some("SendMessage".to_string()),
        created_time: now,
        updated_time: now,
        answer: info.answer.clone().unwrap_or_default(),
    });
    Ok(build_model_tool_success(
        "a2a_send",
        "accepted",
        format!(
            "Submitted task {} to A2A service {}.",
            info.id, service.name
        ),
        json!({
            "endpoint": service.endpoint,
            "service_name": service.name,
            "task_id": info.id,
            "context_id": info.context_id,
            "status": info.status,
            "answer": info.answer,
        }),
    ))
}

/// 观察 A2A 任务状态并返回快照。
pub(crate) async fn a2a_observe(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let snapshot = a2a_observe_snapshot(context, args).await?;
    Ok(build_a2a_snapshot_success(
        "a2a_observe",
        &snapshot,
        None,
        false,
    ))
}

/// 等待 A2A 任务完成或达到超时时间。
pub(crate) async fn a2a_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let timeout_s = args
        .get("wait_s")
        .and_then(Value::as_f64)
        .or_else(|| args.get("timeout_s").and_then(Value::as_f64))
        .unwrap_or(30.0)
        .max(0.0);
    let poll_interval_s = args
        .get("poll_interval_s")
        .and_then(Value::as_f64)
        .unwrap_or(1.5)
        .max(0.2);
    let start = Instant::now();
    let mut last_snapshot = a2a_observe_snapshot(context, args).await?;
    loop {
        if last_snapshot.pending.is_empty() {
            break;
        }
        if timeout_s > 0.0 && start.elapsed().as_secs_f64() >= timeout_s {
            break;
        }
        let remaining = if timeout_s > 0.0 {
            (timeout_s - start.elapsed().as_secs_f64()).max(0.0)
        } else {
            poll_interval_s
        };
        let delay = poll_interval_s.min(remaining.max(0.0));
        if delay <= 0.0 {
            break;
        }
        sleep(Duration::from_secs_f64(delay)).await;
        last_snapshot = a2a_observe_snapshot(context, args).await?;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let elapsed_s = (elapsed * 1000.0).round() / 1000.0;
    let timed_out = !last_snapshot.pending.is_empty() && timeout_s > 0.0 && elapsed >= timeout_s;
    Ok(build_a2a_snapshot_success(
        "a2a_wait",
        &last_snapshot,
        Some(elapsed_s),
        timed_out,
    ))
}

async fn a2a_observe_snapshot(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<A2aObserveSnapshot> {
    let explicit_task_ids = parse_string_list(
        args.get("task_ids")
            .or_else(|| args.get("task_id"))
            .or_else(|| args.get("taskId")),
    );
    let explicit_endpoint = args
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .unwrap_or_default();
    let explicit_service = args
        .get("service_name")
        .or_else(|| args.get("service"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(true);
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);

    let mut tasks = Vec::new();
    let mut seen = HashSet::new();

    for task in context.a2a_store.list_by_user(context.user_id) {
        if !explicit_task_ids.is_empty() && !explicit_task_ids.contains(&task.id) {
            continue;
        }
        if !explicit_service.is_empty()
            && task
                .service_name
                .as_deref()
                .map(|name| name != explicit_service)
                .unwrap_or(true)
        {
            continue;
        }
        if !explicit_endpoint.is_empty()
            && task
                .endpoint
                .as_deref()
                .map(|value| normalize_a2a_endpoint(value) != explicit_endpoint)
                .unwrap_or(true)
        {
            continue;
        }
        let snapshot = build_snapshot_from_task(&task);
        seen.insert(task.id.clone());
        tasks.push(snapshot);
    }

    if let Some(entries) = args.get("tasks").and_then(Value::as_array) {
        for entry in entries {
            if let Some(snapshot) =
                build_snapshot_from_value(entry, &explicit_endpoint, &explicit_service)
            {
                if seen.insert(snapshot.task_id.clone()) {
                    tasks.push(snapshot);
                }
            }
        }
    }

    for task_id in explicit_task_ids {
        if seen.contains(&task_id) {
            continue;
        }
        tasks.push(A2aTaskSnapshot {
            task_id,
            context_id: None,
            status: None,
            endpoint: if explicit_endpoint.is_empty() {
                None
            } else {
                Some(explicit_endpoint.clone())
            },
            service_name: if explicit_service.is_empty() {
                None
            } else {
                Some(explicit_service.clone())
            },
            answer: None,
            updated_time: None,
            refresh_error: None,
        });
    }

    if refresh {
        for item in tasks.iter_mut() {
            if let Err(err) = refresh_a2a_task(context, item, timeout_s).await {
                item.refresh_error = Some(err.to_string());
            }
        }
    }

    let pending = tasks
        .iter()
        .filter(|&item| !item.is_done())
        .cloned()
        .collect::<Vec<_>>();
    Ok(A2aObserveSnapshot { tasks, pending })
}

fn build_snapshot_from_task(task: &A2aTask) -> A2aTaskSnapshot {
    A2aTaskSnapshot {
        task_id: task.id.clone(),
        context_id: task.context_id.clone(),
        status: Some(task.status.clone()),
        endpoint: task.endpoint.clone(),
        service_name: task.service_name.clone(),
        answer: if task.answer.is_empty() {
            None
        } else {
            Some(task.answer.clone())
        },
        updated_time: Some(task.updated_time.with_timezone(&Local).to_rfc3339()),
        refresh_error: None,
    }
}

fn build_snapshot_from_value(
    value: &Value,
    default_endpoint: &str,
    default_service: &str,
) -> Option<A2aTaskSnapshot> {
    let obj = value.as_object()?;
    let task_id = obj
        .get("task_id")
        .or_else(|| obj.get("taskId"))
        .or_else(|| obj.get("id"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if task_id.is_empty() {
        return None;
    }
    let endpoint = obj
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_endpoint.is_empty() {
                None
            } else {
                Some(default_endpoint.to_string())
            }
        });
    let service_name = obj
        .get("service_name")
        .or_else(|| obj.get("service"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_service.is_empty() {
                None
            } else {
                Some(default_service.to_string())
            }
        });
    Some(A2aTaskSnapshot {
        task_id,
        context_id: obj
            .get("context_id")
            .or_else(|| obj.get("contextId"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        status: obj
            .get("status")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        endpoint,
        service_name,
        answer: obj
            .get("answer")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        updated_time: obj
            .get("updated_time")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        refresh_error: None,
    })
}

fn parse_string_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|text| text.trim().to_string()))
            .filter(|text| !text.is_empty())
            .collect(),
        Value::String(text) => text
            .split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_text_arg(args: &Value, keys: &[&str]) -> Option<String> {
    let obj = args.as_object()?;
    for key in keys {
        if let Some(Value::String(text)) = obj.get(*key) {
            let value = text.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn refresh_a2a_task(
    context: &ToolContext<'_>,
    snapshot: &mut A2aTaskSnapshot,
    timeout_s: u64,
) -> Result<()> {
    if snapshot.task_id.trim().is_empty() {
        return Ok(());
    }
    let endpoint = match snapshot.endpoint.clone() {
        Some(endpoint) if !endpoint.is_empty() => endpoint,
        _ => {
            let service_name = snapshot.service_name.as_deref().unwrap_or("");
            if let Some(service) = resolve_a2a_service(context.config, service_name, "") {
                snapshot.endpoint = Some(service.endpoint.clone());
                snapshot.service_name = Some(service.name.clone());
                service.endpoint.clone()
            } else {
                return Ok(());
            }
        }
    };
    let service_name = snapshot.service_name.clone().unwrap_or_default();
    let service = resolve_a2a_service(context.config, &service_name, &endpoint);
    let headers = match service {
        Some(service) => build_a2a_headers(context.config, service)?,
        None => build_a2a_headers_for_endpoint(context.config, &endpoint)?,
    };
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "GetTask",
        "params": { "name": format!("tasks/{}", snapshot.task_id) }
    });
    let response = send_a2a_request(context.http, &endpoint, headers, &payload, timeout_s).await?;
    if let Some(info) = parse_a2a_task_info(&response) {
        snapshot.context_id = info.context_id.clone();
        snapshot.status = info.status.clone();
        snapshot.answer = info.answer.clone();
        snapshot.updated_time = Some(Local::now().to_rfc3339());
        snapshot.refresh_error = None;
        context.a2a_store.update(&info.id, |task| {
            task.context_id = info.context_id.clone();
            task.status = info.status.clone().unwrap_or_default();
            task.answer = info.answer.clone().unwrap_or_default();
            task.updated_time = Utc::now();
        });
    }
    Ok(())
}

fn resolve_a2a_service<'a>(
    config: &'a Config,
    service_name: &str,
    endpoint: &str,
) -> Option<&'a A2aServiceConfig> {
    let normalized_endpoint = normalize_a2a_endpoint(endpoint);
    config.a2a.services.iter().find(|service| {
        if !service_name.is_empty() && service.name == service_name {
            return true;
        }
        if !normalized_endpoint.is_empty() {
            return normalize_a2a_endpoint(&service.endpoint) == normalized_endpoint;
        }
        false
    })
}

fn normalize_a2a_endpoint(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn build_a2a_headers(config: &Config, service: &A2aServiceConfig) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    for (key, value) in &service.headers {
        let name = HeaderName::from_bytes(key.as_bytes())?;
        let value = HeaderValue::from_str(value)?;
        header_map.insert(name, value);
    }
    if let Some(auth) = &service.auth {
        let auth_json = yaml_to_json(auth);
        if let Value::Object(map) = auth_json {
            if let Some(Value::String(token)) = map.get("bearer_token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("api_key") {
                let header = HeaderValue::from_str(token)?;
                header_map.insert(HeaderName::from_static("x-api-key"), header);
            }
        }
    }
    let has_auth = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("authorization"));
    let has_api_key = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("x-api-key"));
    if should_attach_a2a_api_key(config, service) && !has_auth && !has_api_key {
        if let Some(api_key) = config.api_key() {
            header_map.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(&api_key)?,
            );
        }
    }
    Ok(header_map)
}

fn build_a2a_headers_for_endpoint(config: &Config, endpoint: &str) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    if let Some(api_key) = config.api_key() {
        if let Ok(parsed) = url::Url::parse(endpoint) {
            let path = parsed.path().trim_end_matches('/');
            if path.ends_with("/a2a") {
                header_map.insert(
                    HeaderName::from_static("x-api-key"),
                    HeaderValue::from_str(&api_key)?,
                );
            }
        }
    }
    Ok(header_map)
}

fn should_attach_a2a_api_key(config: &Config, service: &A2aServiceConfig) -> bool {
    if config.api_key().is_none() {
        return false;
    }
    if service.name.eq_ignore_ascii_case("wunder") {
        return true;
    }
    if let Ok(parsed) = url::Url::parse(&service.endpoint) {
        let path = parsed.path().trim_end_matches('/');
        return path.ends_with("/a2a");
    }
    false
}

async fn send_a2a_request(
    client: &reqwest::Client,
    endpoint: &str,
    headers: HeaderMap,
    payload: &Value,
    timeout_s: u64,
) -> Result<Value> {
    let mut request = client.post(endpoint).headers(headers).json(payload);
    if timeout_s > 0 {
        request = request.timeout(Duration::from_secs(timeout_s));
    }
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;
    let body: Value =
        serde_json::from_str(&text).map_err(|_| anyhow!("A2A 响应非 JSON: {text}"))?;
    if !status.is_success() {
        return Err(anyhow!("A2A 请求失败: {status}"));
    }
    if body.get("error").is_some() {
        return Err(anyhow!("A2A 返回错误: {body}"));
    }
    Ok(body)
}

fn parse_a2a_task_info(value: &Value) -> Option<A2aTaskInfo> {
    let result = value.get("result").unwrap_or(value);
    let task = result.get("task").unwrap_or(result);
    let task_obj = task.as_object()?;
    let id = task_obj
        .get("id")
        .or_else(|| task_obj.get("task_id"))
        .or_else(|| task_obj.get("taskId"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if id.is_empty() {
        return None;
    }
    let context_id = task_obj
        .get("contextId")
        .or_else(|| task_obj.get("context_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let status = match task_obj.get("status") {
        Some(Value::Object(status_obj)) => status_obj
            .get("state")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    };
    let answer = extract_a2a_answer(task);
    Some(A2aTaskInfo {
        id,
        context_id,
        status,
        answer: if answer.is_empty() {
            None
        } else {
            Some(answer)
        },
    })
}

fn extract_a2a_answer(task: &Value) -> String {
    if let Some(answer) = task.get("answer").and_then(Value::as_str) {
        return answer.to_string();
    }
    let mut parts = Vec::new();
    if let Some(artifacts) = task.get("artifacts").and_then(Value::as_array) {
        for artifact in artifacts {
            if let Some(items) = artifact.get("parts").and_then(Value::as_array) {
                for part in items {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
    }
    parts.join("\n")
}

fn is_a2a_task_finished(status: &str) -> bool {
    matches!(
        status.to_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "rejected"
    )
}

#[cfg(test)]
mod tests {
    use super::{build_a2a_snapshot_success, A2aObserveSnapshot, A2aTaskSnapshot};
    use serde_json::{json, Value};

    #[test]
    fn build_a2a_snapshot_success_marks_running_and_includes_hint() {
        let snapshot = A2aObserveSnapshot {
            tasks: vec![
                A2aTaskSnapshot {
                    task_id: "task-1".to_string(),
                    context_id: Some("ctx-1".to_string()),
                    status: Some("running".to_string()),
                    endpoint: Some("http://a2a.local".to_string()),
                    service_name: Some("helper".to_string()),
                    answer: None,
                    updated_time: Some("2026-01-01T00:00:00+08:00".to_string()),
                    refresh_error: None,
                },
                A2aTaskSnapshot {
                    task_id: "task-2".to_string(),
                    context_id: Some("ctx-2".to_string()),
                    status: Some("completed".to_string()),
                    endpoint: Some("http://a2a.local".to_string()),
                    service_name: Some("helper".to_string()),
                    answer: Some("done".to_string()),
                    updated_time: Some("2026-01-01T00:00:01+08:00".to_string()),
                    refresh_error: None,
                },
            ],
            pending: vec![A2aTaskSnapshot {
                task_id: "task-1".to_string(),
                context_id: Some("ctx-1".to_string()),
                status: Some("running".to_string()),
                endpoint: Some("http://a2a.local".to_string()),
                service_name: Some("helper".to_string()),
                answer: None,
                updated_time: Some("2026-01-01T00:00:00+08:00".to_string()),
                refresh_error: None,
            }],
        };

        let value = build_a2a_snapshot_success("a2a_wait", &snapshot, Some(1.25), true);

        assert_eq!(value.get("state").and_then(Value::as_str), Some("running"));
        assert_eq!(
            value.pointer("/data/pending_total").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            value.pointer("/data/timeout").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value.get("next_step_hint").and_then(Value::as_str),
            Some(
                "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
            )
        );
    }

    #[test]
    fn parse_a2a_task_info_collects_artifact_text_answer() {
        let value = json!({
            "result": {
                "task": {
                    "id": "task-1",
                    "contextId": "ctx-1",
                    "status": { "state": "completed" },
                    "artifacts": [
                        { "parts": [ { "text": "first" }, { "text": "second" } ] }
                    ]
                }
            }
        });
        let info = super::parse_a2a_task_info(&value).expect("task info");
        assert_eq!(info.id, "task-1");
        assert_eq!(info.context_id.as_deref(), Some("ctx-1"));
        assert_eq!(info.status.as_deref(), Some("completed"));
        assert_eq!(info.answer.as_deref(), Some("first\nsecond"));
    }
}
