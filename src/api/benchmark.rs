use crate::benchmark::loader::{default_tasks_dir, load_task_specs};
use crate::benchmark::spec::BenchmarkTaskSpec;
use crate::benchmark::BenchmarkStartRequest;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
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
        .route("/wunder/admin/benchmark/suites", get(benchmark_suites))
        .route("/wunder/admin/benchmark/tasks", get(benchmark_tasks))
        .route("/wunder/admin/benchmark/start", post(benchmark_start))
        .route("/wunder/admin/benchmark/runs", get(benchmark_runs))
        .route(
            "/wunder/admin/benchmark/runs/{run_id}",
            get(benchmark_detail).delete(benchmark_delete),
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

async fn benchmark_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BenchmarkStartRequest>,
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
    Ok(Json(
        json!({ "suites": suites.into_values().collect::<Vec<_>>() }),
    ))
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
    Ok(Json(json!({ "tasks": items })))
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

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
