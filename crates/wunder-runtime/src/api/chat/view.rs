//! One presentation bootstrap. No transcript, event replay or snapshot barrier.
use super::{error_response, format_ts};
use crate::{api::user_context::resolve_user, core::blocking, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wunder_core::thread_view::{ThreadViewPage, CHAT_PAGE_SIZE, TRAJECTORY_PAGE_SIZE};

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions/{session_id}/view/bootstrap",
            get(bootstrap),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/view/chat",
            get(chat_page),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/view/trajectory",
            get(trajectory_page),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/view/changes",
            get(changes),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/view/search",
            get(search),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/bodies/{body_id}",
            get(body),
        )
}
#[derive(Default, Deserialize)]
struct ViewQuery {
    q: Option<String>,
    kind: Option<String>,
    before: Option<i64>,
    root_turn_id: Option<String>,
    limit: Option<usize>,
    after: Option<i64>,
    revision: Option<i64>,
    cursor: Option<i64>,
}

async fn read(
    state: Arc<AppState>,
    headers: HeaderMap,
    session: String,
    query: ViewQuery,
    kind: &str,
    include_metadata: bool,
) -> Result<Json<Value>, Response> {
    if query.before.is_some() && query.after.is_some() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "before and after are mutually exclusive".into(),
        ));
    }
    let user = resolve_user(&state, &headers, None).await?.user.user_id;
    let storage = state.storage.clone();
    let config = state.config_store.get().await;
    let workspace = state.workspace.clone();
    let goal_service = state.kernel.orchestrator.goal_handle();
    let kind = kind.to_string();
    let fallback_session = session.clone();
    let query_session = session.clone();
    // Resolve ownership and metadata in the ordinary DB lane. A slow display
    // projection then degrades independently without discarding the session
    // record needed by the client to finish navigation.
    let record = blocking::run_db("api.chat.view.session", {
        let storage = storage.clone();
        let user = user.clone();
        let session = query_session.clone();
        move || {
            storage
                .get_chat_session(&user, &session)?
                .ok_or_else(|| anyhow::anyhow!("SESSION_NOT_FOUND"))
        }
    })
    .await
    .map_err(read_error)?;
    let view_storage = storage.clone();
    let view_user = user.clone();
    let view_session = query_session.clone();
    let page = match blocking::run_view("api.chat.view.bootstrap", move || {
        let result = view_storage.thread_view_window(
            &view_user,
            &view_session,
            &kind,
            query.before,
            query.after,
            query.root_turn_id.as_deref(),
            query.limit.unwrap_or(if kind == "trajectory" {
                TRAJECTORY_PAGE_SIZE
            } else {
                CHAT_PAGE_SIZE
            }),
        );
        result.or_else(|error| {
            if is_view_timeout(&error) {
                Ok(degraded_page(&view_session))
            } else {
                Err(error)
            }
        })
    })
    .await
    {
        Ok(page) => page,
        Err(error) => {
            tracing::warn!(session_id = %fallback_session, error = %error, "chat thread display read degraded");
            degraded_page_with_diagnostic(
                &fallback_session,
                if is_view_timeout(&error) {
                    "view_timeout"
                } else {
                    "view_unavailable"
                },
            )
        }
    };
    // Paging endpoints only need the bounded projection.  Re-running model,
    // agent, context and goal hydration for every scroll request coupled a
    // cheap indexed page to the ordinary DB lane and made large legacy
    // threads appear to fail after their first paint.
    if !include_metadata {
        return Ok(Json(
            json!({"data": {"session": {"id": record.session_id}, "page": page}}),
        ));
    }
    let mut result=match blocking::run_db("api.chat.view.metadata",move || {
        let model_override=workspace.load_session_model_override(&user,&query_session);
        let agent=record.agent_id.as_ref().map(|id|storage.get_user_agent(&user,id)).transpose()?.flatten();
        let mut metadata=super::sessions::session_model_view(&config,agent.as_ref(),model_override.as_deref());
        let context=workspace.load_session_context_tokens(&user,&query_session).max(0);
        metadata["context_tokens"]=json!(context);metadata["context_occupancy_tokens"]=json!(context);
        metadata["reasoning_effort"]=json!(workspace.load_session_reasoning_effort(&user,&query_session));
        metadata["session_model_name"]=json!(model_override);
        metadata["goal"]=storage.get_session_goal(&user,&query_session)?.map(|record|{
            let mut goal=crate::services::goal::goal_payload(&crate::services::goal::GoalView{record,activation:goal_service.activation(&query_session)});
            for (field,budget) in [("objective",2048),("blocked_message",256)]{if let Some(value)=goal[field].as_str(){goal[field]=json!(wunder_core::thread_view::preview(value,budget));}}goal
        }).unwrap_or(Value::Null);
        let result=json!({"session":{"id":record.session_id,"title":wunder_core::thread_view::preview(&record.title,256),
            "agent_id":record.agent_id,"created_at":format_ts(record.created_at),"updated_at":format_ts(record.updated_at),
            "last_message_at":format_ts(record.last_message_at),"tool_overrides":record.tool_overrides.into_iter().take(32).map(|v|wunder_core::thread_view::preview(&v,96)).collect::<Vec<_>>()}});
        let mut session=result["session"].as_object().cloned().unwrap_or_default();
        session.extend(metadata.as_object().unwrap().clone());
        Ok::<_,anyhow::Error>(Value::Object(session))
    }).await {
        Ok(session) => json!({"session":session}),
        Err(error) if error.to_string() != "SESSION_NOT_FOUND" => {
            tracing::warn!(
                session_id = %fallback_session,
                error = %error,
                "chat session metadata hydration degraded"
            );
            json!({
                "session": {"id": fallback_session}
            })
        }
        Err(error) => return Err(read_error(error)),
    };
    result["page"] = json!(page);
    Ok(Json(json!({"data":result})))
}
fn is_view_timeout(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("timeout") || text.contains("timed out") || text.contains("deadline")
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;

    #[test]
    fn view_timeout_is_reported_as_gateway_timeout() {
        assert_eq!(
            read_error(anyhow::anyhow!("view deadline exceeded")).status(),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn missing_session_is_reported_as_not_found() {
        assert_eq!(
            read_error(anyhow::anyhow!("SESSION_NOT_FOUND")).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn degraded_page_keeps_session_identity_without_rows() {
        let page = degraded_page_with_diagnostic("fixture-session", "view_unavailable");
        assert_eq!(page.session_id, "fixture-session");
        assert!(page.degraded);
        assert_eq!(page.projection_state, "degraded");
        assert!(page.chat.is_empty());
    }
}

fn degraded_page(session: &str) -> ThreadViewPage {
    degraded_page_with_diagnostic(session, "view_timeout")
}

fn degraded_page_with_diagnostic(session: &str, diagnostic: &str) -> ThreadViewPage {
    ThreadViewPage {
        session_id: session.to_owned(),
        projection_state: "degraded".into(),
        thread_empty: None,
        degraded: true,
        diagnostic: Some(diagnostic.into()),
        ..Default::default()
    }
}
fn read_error(e: anyhow::Error) -> Response {
    let message = e.to_string();
    let status = if message == "SESSION_NOT_FOUND" {
        StatusCode::NOT_FOUND
    } else if is_view_timeout(&e) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    error_response(status, message)
}
async fn bootstrap(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    let kind = q.kind.clone().unwrap_or_else(|| "chat".into());
    if !matches!(kind.as_str(), "chat" | "trajectory") {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid view kind".into(),
        ));
    }
    read(s, h, id, q, &kind, true).await
}
async fn chat_page(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    read(s, h, id, q, "chat", false).await
}
async fn trajectory_page(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    read(s, h, id, q, "trajectory", false).await
}
async fn body(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((id, body)): Path<(String, String)>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    let user = resolve_user(&s, &h, None).await?.user.user_id;
    let storage = s.storage.clone();
    let page = blocking::run_view("api.chat.view.body", move || {
        anyhow::ensure!(
            storage.get_chat_session(&user, &id)?.is_some(),
            "SESSION_NOT_FOUND"
        );
        storage.thread_body_page(
            &user,
            &id,
            &body,
            q.revision.unwrap_or(0),
            q.cursor.unwrap_or(0),
        )
    })
    .await
    .map_err(read_error)?;
    Ok(Json(json!({"data":page})))
}
async fn changes(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    let user = resolve_user(&s, &h, None).await?.user.user_id;
    let storage = s.storage.clone();
    let page = blocking::run_view("api.chat.view.changes", move || {
        anyhow::ensure!(
            storage.get_chat_session(&user, &id)?.is_some(),
            "SESSION_NOT_FOUND"
        );
        storage.thread_view_changes(&user, &id, q.after.unwrap_or(0))
    })
    .await
    .map_err(read_error)?;
    Ok(Json(json!({"data":page})))
}

async fn search(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Result<Json<Value>, Response> {
    let query = q.q.unwrap_or_default();
    if query.len() > 256 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "search query too long".into(),
        ));
    }
    let user = resolve_user(&s, &h, None).await?.user.user_id;
    let storage = s.storage.clone();
    let page = blocking::run_view("api.chat.view.search", move || {
        anyhow::ensure!(
            storage.get_chat_session(&user, &id)?.is_some(),
            "SESSION_NOT_FOUND"
        );
        storage.thread_view_search(&user, &id, &query)
    })
    .await
    .map_err(read_error)?;
    Ok(Json(json!({"data":page})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config, config_store::ConfigStore, state::AppStateInitOptions,
        storage::ChatSessionRecord,
    };
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn thread_view_routes_authorize_and_return_bounded_projection() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = dir.path().join("data").to_string_lossy().into();
        config.workspace.root = dir
            .path()
            .join("fixture-workspace")
            .to_string_lossy()
            .into();
        config.skills.enabled.clear();
        let store = ConfigStore::new(dir.path().join("fixture.yaml"));
        let saved = config.clone();
        store.update(|c| *c = saved.clone()).await.unwrap();
        let state = Arc::new(
            AppState::new_with_options(
                store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        let user = state
            .user_store
            .create_user(
                "fixture-user",
                None,
                "fixture-password",
                None,
                None,
                vec!["user".into()],
                "active",
                false,
            )
            .unwrap();
        let other = state
            .user_store
            .create_user(
                "fixture-other",
                None,
                "fixture-password",
                None,
                None,
                vec!["user".into()],
                "active",
                false,
            )
            .unwrap();
        let token = state
            .user_store
            .create_session_token(&user.user_id)
            .unwrap()
            .token;
        let other_token = state
            .user_store
            .create_session_token(&other.user_id)
            .unwrap()
            .token;
        state
            .storage
            .upsert_chat_session(&ChatSessionRecord {
                session_id: "fixture-thread".into(),
                user_id: user.user_id.clone(),
                title: "fixture".into(),
                status: "active".into(),
                created_at: 1.,
                updated_at: 1.,
                last_message_at: 1.,
                agent_id: None,
                workspace_id: None,
                tool_overrides: vec![],
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
        let turn = state
            .storage
            .accept_thread_turn(
                &user.user_id,
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        state
            .kernel
            .orchestrator
            .committer
            .freeze_session(
                &user.user_id,
                "fixture-thread",
                "frozen fixture instructions",
                "fixture-workspace",
            )
            .await
            .unwrap();
        let turn_id = turn["turn_id"].as_str().unwrap();
        state.storage.append_thread_item(&user.user_id,&json!({"session_id":"fixture-thread","turn_id":turn_id,"item_id":format!("{turn_id}:text-1"),"kind":"assistant_message","content":"fixture answer ".repeat(4000),"status":"completed"})).unwrap();
        let app = router().with_state(state);
        for (path, status) in [
            (
                "/wunder/chat/sessions/fixture-thread/view/bootstrap?kind=chat",
                StatusCode::OK,
            ),
            (
                "/wunder/chat/sessions/fixture-thread/view/bootstrap?kind=trajectory",
                StatusCode::OK,
            ),
            (
                "/wunder/chat/sessions/fixture-thread/view/chat?before=100&limit=20",
                StatusCode::OK,
            ),
            (
                "/wunder/chat/sessions/fixture-thread/view/trajectory?limit=20",
                StatusCode::OK,
            ),
            (
                "/wunder/chat/sessions/fixture-thread/view/search?q=fixture",
                StatusCode::OK,
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
            assert!(bytes.len() < 192 * 1024);
            let payload: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(payload["data"].is_object());
            if path.contains("/view/chat?") || path.contains("/view/trajectory?") {
                assert_eq!(payload["data"]["session"]["id"], "fixture-thread");
                assert_eq!(payload["data"]["session"].as_object().unwrap().len(), 1);
                assert!(payload["data"]["page"].is_object());
            }
            let denied = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("authorization", format!("Bearer {other_token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        }
    }
}
