use crate::evaluation::{default_cases_dir, load_case_files, EvaluationDimension};
use crate::evaluation_runner::EvaluationStartRequest;
use crate::i18n;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/evaluation/start", post(eval_start))
        .route(
            "/wunder/admin/evaluation/{run_id}/cancel",
            post(eval_cancel),
        )
        .route("/wunder/admin/evaluation/runs", get(eval_runs))
        .route("/wunder/admin/evaluation/cases", get(eval_cases))
        .route("/wunder/admin/evaluation/stream/{run_id}", get(eval_stream))
        .route(
            "/wunder/admin/evaluation/{run_id}",
            get(eval_detail).delete(eval_delete),
        )
}

async fn eval_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EvaluationStartRequest>,
) -> Result<Json<Value>, Response> {
    let run = state
        .evaluation
        .start(payload)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(run))
}

async fn eval_cancel(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let result = state
        .evaluation
        .cancel(&run_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct EvaluationRunsQuery {
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

async fn eval_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EvaluationRunsQuery>,
) -> Result<Json<Value>, Response> {
    let runs = state
        .evaluation
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

async fn eval_detail(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let run = state
        .evaluation
        .load_run(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(run) = run else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "run not found".to_string(),
        ));
    };
    let items = state
        .evaluation
        .load_items(&run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "run": run, "items": items })))
}

async fn eval_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let deleted = state
        .evaluation
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
        "message": i18n::t("message.deleted"),
    })))
}

async fn eval_cases(State(_state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let case_files = load_case_files(&default_cases_dir())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut summaries = Vec::new();
    for file in case_files {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for case in &file.cases {
            let key = dimension_label(case.dimension).to_string();
            *counts.entry(key).or_insert(0) += 1;
        }
        summaries.push(json!({
            "case_set": file.case_set,
            "language": file.language,
            "version": file.version,
            "case_count": file.cases.len(),
            "dimensions": counts,
        }));
    }
    summaries.sort_by(|a, b| {
        let left_set = a.get("case_set").and_then(Value::as_str).unwrap_or("");
        let right_set = b.get("case_set").and_then(Value::as_str).unwrap_or("");
        let left_lang = a.get("language").and_then(Value::as_str).unwrap_or("");
        let right_lang = b.get("language").and_then(Value::as_str).unwrap_or("");
        (left_set, left_lang).cmp(&(right_set, right_lang))
    });
    Ok(Json(json!({ "case_sets": summaries })))
}

async fn eval_stream(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Response, Response> {
    let receiver = state
        .evaluation
        .subscribe(&run_id)
        .await
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "run not found".to_string()))?;
    let stream = BroadcastStream::new(receiver).filter_map(|item| match item {
        Ok(event) => {
            let event_payload = Event::default()
                .event(event.event)
                .data(event.data.to_string());
            Some(Ok::<Event, std::convert::Infallible>(event_payload))
        }
        Err(_) => None,
    });
    let sse =
        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
    Ok(sse.into_response())
}

fn dimension_label(dimension: EvaluationDimension) -> &'static str {
    match dimension {
        EvaluationDimension::Tool => "tool",
        EvaluationDimension::Logic => "logic",
        EvaluationDimension::Common => "common",
        EvaluationDimension::Complex => "complex",
    }
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
