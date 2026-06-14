use crate::services::browser::{browser_service, BrowserSessionScope};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/browser/health", get(browser_health))
        .route("/wunder/browser/status", get(browser_status))
        .route("/wunder/browser/profiles", get(browser_profiles))
        .route("/wunder/browser/session/start", post(browser_start))
        .route("/wunder/browser/session/stop", post(browser_stop))
        .route("/wunder/browser/tabs", get(browser_tabs))
        .route("/wunder/browser/tabs/open", post(browser_open_tab))
        .route("/wunder/browser/tabs/focus", post(browser_focus_tab))
        .route("/wunder/browser/tabs/close", post(browser_close_tab))
        .route("/wunder/browser/navigate", post(browser_navigate))
        .route("/wunder/browser/snapshot", post(browser_snapshot))
        .route("/wunder/browser/act", post(browser_act))
        .route("/wunder/browser/screenshot", post(browser_screenshot))
        .route("/wunder/browser/read_page", post(browser_read_page))
}

async fn browser_health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config_store.get().await;
    Json(browser_service(&config).health())
}

async fn browser_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config_store.get().await;
    Json(browser_service(&config).status())
}

async fn browser_profiles(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let result = browser_service(&config)
        .profiles()
        .await
        .map_err(api_error)?;
    Ok(Json(result))
}

async fn browser_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "start").await
}

async fn browser_stop(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "stop").await
}

async fn browser_tabs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BrowserScopeQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let scope = query.into_scope();
    let result = browser_service(&config)
        .execute(&scope, "tabs", &json!({}))
        .await
        .map_err(api_error)?;
    Ok(Json(result))
}

async fn browser_open_tab(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "open").await
}

async fn browser_focus_tab(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "focus").await
}

async fn browser_close_tab(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "close").await
}

async fn browser_navigate(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "navigate").await
}

async fn browser_snapshot(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "snapshot").await
}

async fn browser_act(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "act").await
}

async fn browser_screenshot(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "screenshot").await
}

async fn browser_read_page(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    execute_browser_action(state, payload, "read_page").await
}

async fn execute_browser_action(
    state: Arc<AppState>,
    payload: Value,
    action: &str,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let scope = scope_from_value(&payload);
    let args = strip_scope_fields(payload);
    let result = browser_service(&config)
        .execute(&scope, action, &args)
        .await
        .map_err(api_error)?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct BrowserScopeQuery {
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    browser_session_id: Option<String>,
}

impl BrowserScopeQuery {
    fn into_scope(self) -> BrowserSessionScope {
        BrowserSessionScope {
            user_id: self.user_id,
            session_id: self.session_id,
            agent_id: self.agent_id,
            profile: self.profile,
            browser_session_id: self.browser_session_id,
        }
    }
}

fn scope_from_value(payload: &Value) -> BrowserSessionScope {
    BrowserSessionScope {
        user_id: value_string(payload, "user_id"),
        session_id: value_string(payload, "session_id"),
        agent_id: optional_value_string(payload, "agent_id"),
        profile: optional_value_string(payload, "profile"),
        browser_session_id: optional_value_string(payload, "browser_session_id"),
    }
}

fn strip_scope_fields(payload: Value) -> Value {
    let Some(mut object) = payload.as_object().cloned() else {
        return payload;
    };
    for key in [
        "user_id",
        "session_id",
        "agent_id",
        "profile",
        "browser_session_id",
    ] {
        object.remove(key);
    }
    Value::Object(object)
}

fn value_string(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn optional_value_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn api_error(err: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "ok": false,
            "error": err.to_string(),
        })),
    )
        .into_response()
}
