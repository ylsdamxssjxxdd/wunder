//! Read-only context window endpoints for the cloud shell.
//!
//! The engine records the composition of each outgoing request at the request
//! gate (`ContextWindowSnapshot`, text kept in a bounded in-memory cache). These
//! two handlers expose that record to the cloud card: the snapshot itself and
//! one item's memory-only text. They never persist text, never log it, and
//! never echo the requesting user's identity back in the body.
//!
//! Ownership follows every other chat read: the resolved user must own the
//! session, otherwise the lookup returns `None` and the handler replies 404.
use super::error_response;
use crate::api::user_context::resolve_user;
use crate::context_window::ContextWindowSnapshot;
use crate::core::blocking;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions/{session_id}/context_window",
            get(context_window),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/context_window/text",
            get(context_window_text),
        )
}

#[derive(Debug, Deserialize)]
struct ContextTextQuery {
    /// 1-based palette slot, matching `PART_KINDS` order.
    slot: i32,
    tag: String,
    #[serde(default)]
    name: Option<String>,
    count: i64,
}

/// The composition of the thread's last outgoing request. An empty record
/// (thread that never sent a request) returns just the window capacity and an
/// empty parts list; the shell must not invent a zero breakdown.
async fn context_window(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("error.content_required"),
        ));
    }
    // Ownership is checked before any workspace read: a session owned by
    // another user is indistinguishable from a missing one here.
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                crate::i18n::t("error.session_not_found"),
            )
        })?;

    let db = state.clone();
    let owner = resolved.user.user_id.clone();
    let target = session_id.clone();
    let recorded = blocking::run_db("api.chat.context_window", move || {
        Ok(db.workspace.load_session_context_window(&owner, &target))
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    // The same capacity policy as the native shell: a recorded window wins,
    // otherwise the model/agent policy resolves the limit; still 0 means
    // "unknown" and the shell shows a dash instead of dividing by it.
    let capacity = super::native_chat_context_capacity(&state, &resolved.user.user_id, &session_id)
        .await
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .map(i64::from)
        .unwrap_or(0);

    let snapshot = match recorded {
        Some(mut recorded) => {
            if recorded.window <= 0 {
                recorded.window = capacity;
            }
            recorded
        }
        None => ContextWindowSnapshot {
            window: capacity,
            ..Default::default()
        },
    };

    Ok(Json(json!({ "data": snapshot })))
}

/// One item's full text, straight from the memory-only `ContextTextCache`.
/// The text lives only in this process: after a restart or cache eviction the
/// row is unreachable, which the endpoint reports as 404 rather than a stored
/// copy.
async fn context_window_text(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ContextTextQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("error.content_required"),
        ));
    }
    if query.slot < 1 || query.slot > 5 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid context part slot".to_string(),
        ));
    }
    let tag = query.tag.trim().to_string();
    if tag.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid context item tag".to_string(),
        ));
    }
    let name = query.name.unwrap_or_default();

    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                crate::i18n::t("error.session_not_found"),
            )
        })?;

    let db = state.clone();
    let owner = resolved.user.user_id.clone();
    let target = session_id.clone();
    let slot = query.slot;
    let tag_clone = tag.clone();
    let name_clone = name.clone();
    let count = query.count;
    let text = blocking::run_db("api.chat.context_text", move || {
        Ok(db
            .workspace
            .context_text(&owner, &target, slot, &tag_clone, &name_clone, count))
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    match text {
        Some(text) => Ok(Json(json!({ "data": { "text": text } }))),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "context text is not held in this process".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_store::ConfigStore;
    use crate::context_window::{ContextItem, ContextPart, ContextWindowSnapshot};
    use crate::state::AppStateInitOptions;
    use crate::storage::ChatSessionRecord;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn session_record(id: &str, user_id: &str) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: id.into(),
            user_id: user_id.into(),
            title: "thread".into(),
            status: "active".into(),
            created_at: 1.0,
            updated_at: 1.0,
            last_message_at: 1.0,
            agent_id: None,
            workspace_id: None,
            tool_overrides: vec![],
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    async fn build_state() -> Arc<AppState> {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = dir.path().join("data").to_string_lossy().into();
        config.workspace.root = dir.path().join("workspace").to_string_lossy().into();
        let store = ConfigStore::new(dir.path().join("config.yaml"));
        store
            .update(|current| *current = config.clone())
            .await
            .unwrap();
        Arc::new(
            AppState::new_with_options(store, config, AppStateInitOptions::cli_default()).unwrap(),
        )
    }

    async fn token_for(state: &AppState, username: &str) -> String {
        let user = state
            .user_store
            .create_user(
                username,
                None,
                "test-password",
                None,
                None,
                vec!["user".into()],
                "active",
                false,
            )
            .unwrap();
        state
            .user_store
            .create_session_token(&user.user_id)
            .unwrap()
            .token
    }

    fn five_part_snapshot() -> ContextWindowSnapshot {
        ContextWindowSnapshot {
            model: "test-model".into(),
            provider: "test-provider".into(),
            window: 128_000,
            tokens: 1_000,
            measured: true,
            turns: 1,
            parts: vec![
                ContextPart {
                    kind: "system".into(),
                    tokens: 300,
                    items: vec![ContextItem {
                        tag: "prompt".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 300,
                        order: 1,
                        text: "system prompt body".into(),
                    }],
                },
                ContextPart {
                    kind: "tools".into(),
                    tokens: 200,
                    items: vec![ContextItem {
                        tag: "tool".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 200,
                        order: 0,
                        text: "tool definition".into(),
                    }],
                },
                ContextPart {
                    kind: "files".into(),
                    tokens: 120,
                    items: vec![ContextItem {
                        tag: "image".into(),
                        name: String::new(),
                        count: 2,
                        tokens: 120,
                        order: 0,
                        text: String::new(),
                    }],
                },
                ContextPart {
                    kind: "results".into(),
                    tokens: 180,
                    items: vec![ContextItem {
                        tag: "result".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 180,
                        order: 6,
                        text: "file body".into(),
                    }],
                },
                ContextPart {
                    kind: "chat".into(),
                    tokens: 200,
                    items: vec![ContextItem {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 200,
                        order: 2,
                        text: "first question".into(),
                    }],
                },
            ],
        }
    }

    async fn get_json(
        state: Arc<AppState>,
        token: &str,
        uri: &str,
    ) -> (axum::http::StatusCode, Value) {
        let app = crate::api::chat::router().with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, value)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_snapshot_reports_window_only_without_fake_parts() {
        let state = build_state().await;
        let token = token_for(&state, "owner-empty").await;
        state
            .storage
            .upsert_chat_session(&session_record("empty-session", "owner-empty"))
            .unwrap();

        let (status, body) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/empty-session/context_window",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let data = &body["data"];
        assert!(data["parts"].as_array().unwrap().is_empty());
        assert_eq!(data["tokens"], json!(0));
        // No text field leaks from a persisted/empty record.
        assert!(data.get("text").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn five_part_snapshot_round_trips_its_recorded_fields() {
        let state = build_state().await;
        let token = token_for(&state, "owner-five").await;
        state
            .storage
            .upsert_chat_session(&session_record("five-session", "owner-five"))
            .unwrap();
        let snapshot = five_part_snapshot();
        state
            .workspace
            .remember_context_texts("owner-five", "five-session", &snapshot);
        state
            .workspace
            .save_session_context_window("owner-five", "five-session", &snapshot);

        let (status, body) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/five-session/context_window",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let data = &body["data"];
        assert_eq!(data["model"], json!("test-model"));
        assert_eq!(data["provider"], json!("test-provider"));
        assert_eq!(data["window"], json!(128_000));
        assert_eq!(data["tokens"], json!(1_000));
        assert_eq!(data["measured"], json!(true));
        assert_eq!(data["turns"], json!(1));
        let kinds: Vec<String> = data["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part["kind"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kinds, vec!["system", "tools", "files", "results", "chat"]);
        // Persisted snapshot never carries the item text.
        let serialized = serde_json::to_string(&data).unwrap();
        assert!(!serialized.contains("first question"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn context_text_hit_and_miss() {
        let state = build_state().await;
        let token = token_for(&state, "owner-text").await;
        state
            .storage
            .upsert_chat_session(&session_record("text-session", "owner-text"))
            .unwrap();
        let snapshot = five_part_snapshot();
        state
            .workspace
            .remember_context_texts("owner-text", "text-session", &snapshot);
        state
            .workspace
            .save_session_context_window("owner-text", "text-session", &snapshot);

        // Hit: chat turn, identified by the turn ordinal carried in `count`.
        let (hit_status, hit_body) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/text-session/context_window/text?slot=5&tag=turn&name=&count=1",
        )
        .await;
        assert_eq!(hit_status, StatusCode::OK);
        assert_eq!(hit_body["data"]["text"], json!("first question"));

        // Result row, named by the tool.
        let (result_status, result_body) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/text-session/context_window/text?slot=4&tag=result&name=read_file&count=1",
        )
        .await;
        assert_eq!(result_status, StatusCode::OK);
        assert_eq!(result_body["data"]["text"], json!("file body"));

        // Miss: an ordinal that does not exist in the memory cache.
        let (miss_status, _) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/text-session/context_window/text?slot=5&tag=turn&name=&count=99",
        )
        .await;
        assert_eq!(miss_status, StatusCode::NOT_FOUND);

        // Out-of-range slot is a bad request, not a silent miss.
        let (bad_status, _) = get_json(
            state.clone(),
            &token,
            "/wunder/chat/sessions/text-session/context_window/text?slot=9&tag=turn&name=&count=1",
        )
        .await;
        assert_eq!(bad_status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cross_user_cannot_read_another_user_session() {
        let state = build_state().await;
        let owner_token = token_for(&state, "owner-a").await;
        let other_token = token_for(&state, "owner-b").await;
        state
            .storage
            .upsert_chat_session(&session_record("owned-session", "owner-a"))
            .unwrap();
        let snapshot = five_part_snapshot();
        state
            .workspace
            .remember_context_texts("owner-a", "owned-session", &snapshot);
        state
            .workspace
            .save_session_context_window("owner-a", "owned-session", &snapshot);

        // The other user's token resolves but the session lookup by owner is
        // empty, so both read paths reject with 404.
        let (window_status, _) = get_json(
            state.clone(),
            &other_token,
            "/wunder/chat/sessions/owned-session/context_window",
        )
        .await;
        assert_eq!(window_status, StatusCode::NOT_FOUND);

        let (text_status, _) = get_json(
            state.clone(),
            &other_token,
            "/wunder/chat/sessions/owned-session/context_window/text?slot=5&tag=turn&name=&count=1",
        )
        .await;
        assert_eq!(text_status, StatusCode::NOT_FOUND);

        // The owner still reads its own session.
        let (owner_status, _) = get_json(
            state.clone(),
            &owner_token,
            "/wunder/chat/sessions/owned-session/context_window",
        )
        .await;
        assert_eq!(owner_status, StatusCode::OK);
    }

    #[test]
    fn default_snapshot_empty_parts_and_zero_window() {
        let snapshot = ContextWindowSnapshot::default();
        assert!(snapshot.parts.is_empty());
        assert_eq!(snapshot.window, 0);
    }
}
