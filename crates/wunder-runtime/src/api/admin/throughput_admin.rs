use super::error_response;
use crate::{
    state::AppState,
    throughput::{
        ThroughputConfig, ThroughputReport, ThroughputSnapshot, ThroughputStatusResponse,
    },
};
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/throughput/start", post(start))
        .route("/wunder/admin/throughput/stop", post(stop))
        .route("/wunder/admin/throughput/status", get(status))
        .route("/wunder/admin/throughput/report", get(report))
        .route("/wunder/admin/throughput/ticket", post(ticket))
        // Browser WebSockets cannot attach admin HTTP headers. A single-use ticket
        // issued by the guarded admin route authorizes this read-only snapshot feed.
        .route("/wunder/throughput/ws", get(watch))
}

async fn start(
    State(state): State<Arc<AppState>>,
    Json(config): Json<ThroughputConfig>,
) -> Result<Json<ThroughputSnapshot>, Response> {
    let model = config
        .resolve(&state.config_store.get().await)
        .map_err(|error| error_response(StatusCode::BAD_REQUEST, error))?;
    state
        .throughput
        .start(config, model)
        .await
        .map(Json)
        .map_err(|error| error_response(StatusCode::CONFLICT, error))
}

async fn stop(State(state): State<Arc<AppState>>) -> Result<Json<ThroughputSnapshot>, Response> {
    state
        .throughput
        .stop()
        .await
        .map(Json)
        .map_err(|error| error_response(StatusCode::BAD_REQUEST, error))
}

async fn status(State(state): State<Arc<AppState>>) -> Json<ThroughputStatusResponse> {
    Json(state.throughput.status().await)
}

#[derive(Deserialize)]
struct ReportQuery {
    run_id: Option<String>,
}

async fn report(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReportQuery>,
) -> Result<Json<ThroughputReport>, Response> {
    state
        .throughput
        .report(query.run_id.as_deref())
        .await
        .map(Json)
        .map_err(|error| error_response(StatusCode::NOT_FOUND, error))
}

async fn ticket(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({ "ticket": state.throughput.issue_ticket(), "expires_in_s": 30 }))
}

async fn watch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let ticket = headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .find_map(|item| item.trim().strip_prefix("ticket."))
        });
    if !ticket.is_some_and(|ticket| state.throughput.consume_ticket(ticket)) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Invalid or expired benchmark ticket".into(),
        );
    }
    upgrade.max_message_size(4096).max_frame_size(4096).protocols(["wunder-throughput"]).on_upgrade(move |mut socket| async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sequence = 0u64;
        let lifetime = tokio::time::sleep(Duration::from_secs(900));
        tokio::pin!(lifetime);
        loop {
            tokio::select! {
                _ = &mut lifetime => break,
                message = socket.recv() => {
                    if matches!(message, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) { break; }
                }
                _ = interval.tick() => {
                    sequence += 1;
                    let snapshot = state.throughput.status().await;
                    let payload = json!({ "event": "snapshot", "sequence": sequence, "data": snapshot });
                    if !matches!(tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Text(payload.to_string().into()))).await, Ok(Ok(()))) { break; }
                }
            }
        }
    }).into_response()
}
