//! 线程渲染压测 API：为登录用户在后台生成模拟线程，供蜂巢/蜂窝/舵机做
//! 消息渲染与性能验证。生成走任务服务，批量写入与 CLI/蜂窝入口共用同一实现。

use super::error_response;
use crate::api::user_context::resolve_user;
use crate::services::stress_thread::{self, StartStressJobRequest};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/stress-threads",
            post(start_stress_thread).get(list_stress_threads),
        )
        .route(
            "/wunder/chat/stress-threads/{job_id}",
            get(get_stress_thread),
        )
}

#[derive(Debug, Deserialize)]
struct StartStressThreadRequest {
    #[serde(alias = "userRounds")]
    user_rounds: i64,
    #[serde(alias = "modelRounds")]
    model_rounds: i64,
    #[serde(default)]
    title: Option<String>,
}

async fn start_stress_thread(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<StartStressThreadRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let config = state.config_store.get().await;
    let target = stress_thread::StressStorageTarget::from_config(&config.storage)
        .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    stress_thread::start_stress_thread_job(StartStressJobRequest {
        target,
        user_id: resolved.user.user_id.clone(),
        user_rounds: payload.user_rounds,
        model_rounds_per_turn: payload.model_rounds,
        title: payload.title,
    })
    .map(Json)
    .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))
}

async fn get_stress_thread(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    stress_thread::stress_job_status(&resolved.user.user_id, job_id.trim())
        .map(Json)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, format!("任务不存在: {job_id}")))
}

async fn list_stress_threads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    Ok(Json(serde_json::json!({
        "jobs": stress_thread::list_stress_jobs(&resolved.user.user_id),
    })))
}
