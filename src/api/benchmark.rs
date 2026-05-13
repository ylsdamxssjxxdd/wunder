use crate::benchmark::loader::{default_tasks_dir, load_task_specs};
use crate::benchmark::profiles::available_profiles;
use crate::benchmark::spec::BenchmarkTaskSpec;
use crate::benchmark::BenchmarkStartRequest;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/wunderbench/profiles",
            get(benchmark_profiles),
        )
        .route("/wunder/admin/wunderbench/suites", get(benchmark_suites))
        .route("/wunder/admin/wunderbench/tasks", get(benchmark_tasks))
        .route("/wunder/admin/wunderbench/start", post(benchmark_start))
        .route("/wunder/admin/wunderbench/runs", get(benchmark_runs))
        .route(
            "/wunder/admin/wunderbench/runs/{run_id}",
            get(benchmark_detail).delete(benchmark_delete),
        )
        .route(
            "/wunder/admin/wunderbench/runs/{run_id}/export",
            get(benchmark_export),
        )
        .route(
            "/wunder/admin/wunderbench/runs/{run_id}/cancel",
            post(benchmark_cancel),
        )
        .route(
            "/wunder/admin/wunderbench/runs/{run_id}/stream",
            get(benchmark_stream),
        )
        .route("/wunder/admin/benchmark/profiles", get(benchmark_profiles))
        .route("/wunder/admin/benchmark/suites", get(benchmark_suites))
        .route("/wunder/admin/benchmark/tasks", get(benchmark_tasks))
        .route(
            "/wunder/admin/benchmark/start",
            post(benchmark_start_legacy),
        )
        .route("/wunder/admin/benchmark/runs", get(benchmark_runs))
        .route(
            "/wunder/admin/benchmark/runs/{run_id}",
            get(benchmark_detail).delete(benchmark_delete),
        )
        .route(
            "/wunder/admin/benchmark/runs/{run_id}/export",
            get(benchmark_export),
        )
        .route(
            "/wunder/admin/benchmark/runs/{run_id}/cancel",
            post(benchmark_cancel),
        )
        .route(
            "/wunder/admin/benchmark/runs/{run_id}/stream",
            get(benchmark_stream),
        )
}

async fn benchmark_profiles(State(_state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let tasks = load_task_specs(default_tasks_dir().as_path())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "benchmark": "wunderbench",
        "profiles": available_profiles(&tasks),
    })))
}

async fn benchmark_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BenchmarkStartRequest>,
) -> Result<Json<Value>, Response> {
    benchmark_start_with_state(state, payload).await
}

async fn benchmark_start_legacy(
    State(state): State<Arc<AppState>>,
    Json(mut payload): Json<BenchmarkStartRequest>,
) -> Result<Json<Value>, Response> {
    if is_empty(payload.profile.as_deref())
        && payload.suite_ids.iter().all(|value| is_empty(Some(value)))
        && payload.task_ids.iter().all(|value| is_empty(Some(value)))
    {
        // Preserve the old benchmark default: no explicit filter meant the full suite.
        payload.profile = Some("full".to_string());
    }
    benchmark_start_with_state(state, payload).await
}

async fn benchmark_start_with_state(
    state: Arc<AppState>,
    payload: BenchmarkStartRequest,
) -> Result<Json<Value>, Response> {
    let result = state
        .benchmark
        .start(payload)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(result))
}

async fn benchmark_cancel(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let result = state
        .benchmark
        .cancel(&run_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct BenchmarkRunsQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    since_time: Option<f64>,
    #[serde(default)]
    until_time: Option<f64>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn benchmark_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BenchmarkRunsQuery>,
) -> Result<Json<Value>, Response> {
    let runs = state
        .benchmark
        .load_runs(
            query.user_id.as_deref(),
            query.status.as_deref(),
            query.model_name.as_deref(),
            query.since_time,
            query.until_time,
            query.limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "runs": runs })))
}

async fn benchmark_detail(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let run = state
        .benchmark
        .load_run(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(run) = run else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "run not found".to_string(),
        ));
    };
    let tasks = state
        .benchmark
        .load_task_aggregates(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let attempts = state
        .benchmark
        .load_attempts(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "run": run, "tasks": tasks, "attempts": attempts }),
    ))
}

async fn benchmark_export(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Response, Response> {
    let payload = build_benchmark_export_payload(&state, &run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if payload.is_null() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "run not found".to_string(),
        ));
    }
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filename = format!(
        "wunderbench-{}-export.json",
        sanitize_export_filename_component(&run_id)
    );
    Ok(download_bytes_response(
        bytes,
        &filename,
        "application/json; charset=utf-8",
    ))
}

async fn benchmark_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let deleted = state
        .benchmark
        .delete_run(&run_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(deleted) = deleted else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "run not found".to_string(),
        ));
    };
    Ok(Json(json!({
        "ok": true,
        "run_id": run_id,
        "deleted": deleted,
    })))
}

async fn benchmark_stream(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Response, Response> {
    let receiver = state
        .benchmark
        .subscribe(&run_id)
        .await
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "run not found".to_string()))?;
    let stream = BroadcastStream::new(receiver).filter_map(|item| match item {
        Ok(event) => Some(Ok::<Event, std::convert::Infallible>(
            Event::default()
                .event(event.event)
                .data(event.data.to_string()),
        )),
        Err(_) => None,
    });
    let sse =
        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
    Ok(sse.into_response())
}

#[derive(Debug, Deserialize)]
struct BenchmarkTasksQuery {
    #[serde(default)]
    suite: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    grading_type: Option<String>,
}

async fn benchmark_suites(State(_state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let task_dir = default_tasks_dir();
    let tasks = load_task_specs(task_dir.as_path())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut suites: BTreeMap<String, Value> = BTreeMap::new();
    for task in &tasks {
        let suite_id = task.frontmatter.suite.clone();
        let entry = suites.entry(suite_id.clone()).or_insert_with(|| {
            json!({
                "suite_id": suite_id,
                "task_count": 0,
                "categories": {},
                "grading_types": {},
                "recommended_runs": 0,
            })
        });
        if let Some(value) = entry.get_mut("task_count") {
            *value = json!(value.as_u64().unwrap_or(0) + 1);
        }
        bump_map(entry, "categories", &task.frontmatter.category);
        bump_map(entry, "grading_types", grading_type_name(task));
        if let Some(value) = entry.get_mut("recommended_runs") {
            *value = json!(value
                .as_u64()
                .unwrap_or(0)
                .max(task.frontmatter.runs_recommended as u64));
        }
    }
    Ok(Json(json!({
        "benchmark": "wunderbench",
        "profiles": available_profiles(&tasks),
        "suites": suites.into_values().collect::<Vec<_>>()
    })))
}

async fn benchmark_tasks(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<BenchmarkTasksQuery>,
) -> Result<Json<Value>, Response> {
    let task_dir = default_tasks_dir();
    let mut tasks = load_task_specs(task_dir.as_path())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    tasks.retain(|task| filter_task(task, &query));
    let items = tasks.into_iter().map(task_to_summary).collect::<Vec<_>>();
    Ok(Json(json!({ "benchmark": "wunderbench", "tasks": items })))
}

fn filter_task(task: &BenchmarkTaskSpec, query: &BenchmarkTasksQuery) -> bool {
    if let Some(suite) = query.suite.as_deref() {
        if !suite.trim().is_empty() && task.frontmatter.suite != suite.trim() {
            return false;
        }
    }
    if let Some(category) = query.category.as_deref() {
        if !category.trim().is_empty() && task.frontmatter.category != category.trim() {
            return false;
        }
    }
    if let Some(grading_type) = query.grading_type.as_deref() {
        if !grading_type.trim().is_empty() && grading_type_name(task) != grading_type.trim() {
            return false;
        }
    }
    true
}

fn task_to_summary(task: BenchmarkTaskSpec) -> Value {
    let grading_type = grading_type_name(&task).to_string();
    json!({
        "id": task.frontmatter.id,
        "name": task.frontmatter.name,
        "suite": task.frontmatter.suite,
        "category": task.frontmatter.category,
        "grading_type": grading_type,
        "timeout_seconds": task.frontmatter.timeout_seconds,
        "runs_recommended": task.frontmatter.runs_recommended,
        "difficulty": task.frontmatter.difficulty,
        "required_tools": task.frontmatter.required_tools,
        "tags": task.frontmatter.tags,
        "languages": task.frontmatter.languages,
        "criteria_count": task.grading_criteria.len(),
        "has_automated_checks": task.has_automated_checks(),
        "has_judge_rubric": task.has_judge_rubric(),
        "coverage": {
            "tool_use": !task.frontmatter.required_tools.is_empty(),
            "workspace": task.frontmatter.required_tools.iter().any(|tool| {
                matches!(
                    tool.as_str(),
                    "read_file" | "write_file" | "edit_file" | "list_files" | "execute_command"
                )
            }),
            "judge": task.has_judge_rubric(),
        },
        "prompt": task.prompt,
        "expected_behavior": task.expected_behavior,
    })
}

fn grading_type_name(task: &BenchmarkTaskSpec) -> &str {
    match task.frontmatter.grading_type {
        crate::benchmark::spec::BenchmarkGradingType::Automated => "automated",
        crate::benchmark::spec::BenchmarkGradingType::LlmJudge => "llm_judge",
        crate::benchmark::spec::BenchmarkGradingType::Hybrid => "hybrid",
    }
}

fn bump_map(target: &mut Value, key: &str, value: &str) {
    let Some(map) = target.as_object_mut() else {
        return;
    };
    let entry = map.entry(key.to_string()).or_insert_with(|| json!({}));
    let Some(counts) = entry.as_object_mut() else {
        return;
    };
    let current = counts.get(value).and_then(Value::as_u64).unwrap_or(0);
    counts.insert(value.to_string(), json!(current + 1));
}

fn build_benchmark_export_payload(state: &AppState, run_id: &str) -> anyhow::Result<Value> {
    let cleaned_run_id = run_id.trim();
    if cleaned_run_id.is_empty() {
        return Ok(Value::Null);
    }
    let Some(run) = state.benchmark.load_run(cleaned_run_id)? else {
        return Ok(Value::Null);
    };
    let tasks = state.benchmark.load_task_aggregates(cleaned_run_id)?;
    let attempts = state.benchmark.load_attempts(cleaned_run_id)?;
    let task_specs = load_task_specs(default_tasks_dir().as_path()).unwrap_or_default();
    let task_spec_map = task_specs
        .into_iter()
        .map(|task| {
            let id = task.frontmatter.id.clone();
            (id, task_to_export_spec(task))
        })
        .collect::<BTreeMap<_, _>>();
    let attempt_logs = attempts
        .iter()
        .map(|attempt| build_attempt_log_export(state, cleaned_run_id, attempt))
        .collect::<Vec<_>>();
    let missing_logs = attempt_logs
        .iter()
        .filter(|item| {
            item.get("monitor_record")
                .map(Value::is_null)
                .unwrap_or(true)
        })
        .count();
    Ok(json!({
        "export_schema_version": 1,
        "export_type": "wunderbench_run",
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "run_id": cleaned_run_id,
        "run": run,
        "task_aggregates": tasks,
        "attempts": attempts,
        "task_specs": task_spec_map,
        "attempt_logs": attempt_logs,
        "diagnostics": {
            "attempt_count": attempt_logs.len(),
            "missing_monitor_records": missing_logs,
            "notes": [
                "attempt.transcript is captured from the benchmark stream.",
                "attempt_logs.monitor_record contains the raw persisted monitor events for model/tool/runtime analysis.",
                "For historical runs created before debug logging was enabled, llm_request payloads may be summarized by monitor policy."
            ]
        }
    }))
}

fn build_attempt_log_export(state: &AppState, run_id: &str, attempt: &Value) -> Value {
    let task_id = attempt
        .get("task_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let attempt_no = attempt
        .get("attempt_no")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let session_id = format!("bench-{run_id}-{task_id}-{attempt_no}");
    let judge_session_id = format!("{session_id}-judge");
    let monitor_record = state.monitor.get_record(&session_id).unwrap_or(Value::Null);
    let monitor_detail = state.monitor.get_detail(&session_id).unwrap_or(Value::Null);
    let judge_monitor_record = state
        .monitor
        .get_record(&judge_session_id)
        .unwrap_or(Value::Null);
    let judge_monitor_detail = state
        .monitor
        .get_detail(&judge_session_id)
        .unwrap_or(Value::Null);
    json!({
        "task_id": task_id,
        "attempt_no": attempt_no,
        "session_id": session_id,
        "judge_session_id": judge_session_id,
        "attempt_summary": attempt,
        "monitor_record": monitor_record,
        "monitor_detail": monitor_detail,
        "judge_monitor_record": judge_monitor_record,
        "judge_monitor_detail": judge_monitor_detail,
    })
}

fn task_to_export_spec(task: BenchmarkTaskSpec) -> Value {
    json!({
        "id": task.frontmatter.id,
        "name": task.frontmatter.name,
        "suite": task.frontmatter.suite,
        "category": task.frontmatter.category,
        "grading_type": grading_type_name(&task),
        "timeout_seconds": task.frontmatter.timeout_seconds,
        "runs_recommended": task.frontmatter.runs_recommended,
        "difficulty": task.frontmatter.difficulty,
        "required_tools": task.frontmatter.required_tools,
        "tags": task.frontmatter.tags,
        "languages": task.frontmatter.languages,
        "prompt": task.prompt,
        "expected_behavior": task.expected_behavior,
        "grading_criteria": task.grading_criteria,
        "automated_checks": task.automated_checks,
        "llm_judge_rubric": task.llm_judge_rubric,
        "file_path": task.file_path,
    })
}

fn download_bytes_response(bytes: Vec<u8>, filename: &str, content_type: &'static str) -> Response {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(&build_content_disposition(filename)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn build_content_disposition(filename: &str) -> String {
    let ascii_name = sanitize_export_filename_component(filename);
    if ascii_name == filename {
        return format!("attachment; filename=\"{ascii_name}\"");
    }
    format!("attachment; filename=\"{ascii_name}\"")
}

fn sanitize_export_filename_component(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "wunderbench".to_string()
    } else {
        output
    }
}

fn is_empty(value: Option<&str>) -> bool {
    value.map(str::trim).unwrap_or("").is_empty()
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[cfg(test)]
mod tests {
    use super::sanitize_export_filename_component;

    #[test]
    fn sanitize_export_filename_component_keeps_safe_run_ids() {
        assert_eq!(
            sanitize_export_filename_component("abc123-task_1.json"),
            "abc123-task_1.json"
        );
    }

    #[test]
    fn sanitize_export_filename_component_replaces_unsafe_chars() {
        assert_eq!(
            sanitize_export_filename_component("run/with\\unsafe:*?"),
            "run_with_unsafe___"
        );
    }
}
