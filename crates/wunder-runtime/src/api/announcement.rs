// 系统消息：管理员维护一条面向全体用户的公告，用户上线时拉取并展示。
use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::i18n;
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const ANNOUNCEMENT_META_KEY: &str = "system_announcement:v1";
const MAX_TITLE_CHARS: usize = 100;
const MAX_CONTENT_CHARS: usize = 4000;

/// 单条系统消息。`revision` 在内容变化时更换，客户端据此判断是否已经确认过。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AnnouncementRecord {
    #[serde(default)]
    revision: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    updated_at: String,
}

impl AnnouncementRecord {
    fn public_view(&self) -> Option<Value> {
        if !self.enabled || self.content.trim().is_empty() {
            return None;
        }
        Some(json!({
            "revision": self.revision,
            "title": self.title,
            "content": self.content,
            "updated_at": self.updated_at,
        }))
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/announcement", get(get_announcement))
}

pub fn admin_router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/admin/announcement",
        get(admin_get_announcement).post(admin_update_announcement),
    )
}

async fn get_announcement(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(response) = resolve_user(&state, &headers, None).await {
        return response;
    }
    let record = match load_record(&state).await {
        Ok(record) => record,
        Err(response) => return response,
    };
    Json(json!({ "announcement": record.public_view() })).into_response()
}

async fn admin_get_announcement(State(state): State<Arc<AppState>>) -> Response {
    match load_record(&state).await {
        Ok(record) => Json(json!({ "announcement": record })).into_response(),
        Err(response) => response,
    }
}

#[derive(Debug, Deserialize)]
struct AnnouncementUpdateRequest {
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    enabled: bool,
}

async fn admin_update_announcement(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AnnouncementUpdateRequest>,
) -> Response {
    let title = payload.title.trim().to_string();
    let content = payload.content.trim().to_string();
    if let Some(response) = validate_length(&title, MAX_TITLE_CHARS) {
        return response;
    }
    if let Some(response) = validate_length(&content, MAX_CONTENT_CHARS) {
        return response;
    }
    if payload.enabled && content.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, i18n::t("error.content_required"));
    }

    let mut record = match load_record(&state).await {
        Ok(record) => record,
        Err(response) => return response,
    };
    let content_changed = record.title != title || record.content != content;
    record.title = title;
    record.content = content;
    record.enabled = payload.enabled;
    if content_changed {
        record.revision = uuid::Uuid::new_v4().simple().to_string();
        record.updated_at = Utc::now().to_rfc3339();
    }

    if let Err(response) = save_record(&state, &record).await {
        return response;
    }
    Json(json!({ "announcement": record })).into_response()
}

fn validate_length(value: &str, max_chars: usize) -> Option<Response> {
    let actual_chars = value.chars().count();
    if actual_chars <= max_chars {
        return None;
    }
    let message = i18n::t_with_params(
        "error.user_input_too_long",
        &HashMap::from([
            ("max_chars".to_string(), max_chars.to_string()),
            ("actual_chars".to_string(), actual_chars.to_string()),
        ]),
    );
    Some(error_response(StatusCode::BAD_REQUEST, message))
}

async fn load_record(state: &AppState) -> Result<AnnouncementRecord, Response> {
    let storage = state.storage.clone();
    let raw = blocking::run_db("api.announcement.get", move || {
        storage.get_meta(ANNOUNCEMENT_META_KEY)
    })
    .await
    .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(raw
        .as_deref()
        .and_then(|text| serde_json::from_str::<AnnouncementRecord>(text).ok())
        .unwrap_or_default())
}

async fn save_record(state: &AppState, record: &AnnouncementRecord) -> Result<(), Response> {
    let stored = serde_json::to_string(record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let storage = state.storage.clone();
    blocking::run_db("api.announcement.set", move || {
        storage.set_meta(ANNOUNCEMENT_META_KEY, &stored)
    })
    .await
    .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_view_hides_disabled_or_empty_messages() {
        let mut record = AnnouncementRecord {
            revision: "r1".into(),
            title: "维护通知".into(),
            content: "今晚升级".into(),
            enabled: true,
            updated_at: "2026-11-13T00:00:00Z".into(),
        };
        let view = record.public_view().expect("enabled message is visible");
        assert_eq!(view["content"], "今晚升级");

        record.enabled = false;
        assert!(record.public_view().is_none());

        record.enabled = true;
        record.content = "   ".into();
        assert!(record.public_view().is_none());
    }

    #[test]
    fn validate_length_rejects_oversized_fields() {
        assert!(validate_length("ok", 4).is_none());
        assert!(validate_length("超出", 1).is_some());
    }
}
