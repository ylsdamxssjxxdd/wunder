use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::goal::{self, execute_goal_command, goal_payload, GoalCommand, GoalService};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/chat/sessions/{session_id}/goal",
        get(get_session_goal)
            .put(upsert_session_goal)
            .delete(delete_session_goal),
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoalUpsertPayload {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default, alias = "maxGoalRounds", alias = "max_goal_rounds")]
    pub max_goal_rounds: Option<i64>,
    /// Optional explicit transition: pause | resume. Without it an objective
    /// payload creates or edits the goal.
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GoalMutationOutcome {
    pub goal: Option<Value>,
    pub user_round: Option<i64>,
}

async fn get_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    let view = goal_service(&state)
        .get_view(&state.storage, &resolved.user.user_id, &session_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "data": {
            "goal": view.as_ref().map(goal_payload)
        }
    })))
}

async fn upsert_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<GoalUpsertPayload>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    // The same PUT endpoint is used by the first `/goal <objective>` command
    // and by later edits from the goal bar.  Resolve the current state before
    // choosing the command; treating every objective as `Edit` makes the
    // initial goal fail with GOAL_NOT_FOUND and prevents the driver from
    // starting a model round.
    let has_current_goal = goal_service(&state)
        .get_view(&state.storage, &resolved.user.user_id, &session_id)
        .await
        .map_err(bad_request)?
        .is_some();
    let command = goal_command_from_payload(payload, has_current_goal)?;
    let outcome = native_session_goal_outcome(&state, &resolved.user, &session_id, command)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "data": {
            "goal": outcome.goal,
            "user_round": outcome.user_round,
        }
    }))
    .into_response())
}

async fn delete_session_goal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = normalize_session_id(session_id)?;
    ensure_session_owner(&state, &resolved.user.user_id, &session_id)?;
    native_session_goal_outcome(&state, &resolved.user, &session_id, GoalCommand::Clear)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "data": { "deleted": true, "goal": null } })))
}

fn goal_service(state: &AppState) -> Arc<GoalService> {
    state.kernel.orchestrator.goal_handle()
}

/// In-process command entry used by the desktop façade and the CLI slash
/// surface. Persists the command echo as a durable user round and re-arms
/// the driver for create/resume.
pub async fn native_session_goal(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<Option<crate::storage::SessionGoalRecord>> {
    let outcome = native_session_goal_outcome(state, user, session_id, command).await?;
    match outcome.goal {
        Some(value) => {
            let goal_id = value
                .get("goal_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(goal_id) = goal_id {
                let storage = state.storage.clone();
                let user_id = user.user_id.clone();
                let session_id = session_id.trim().to_string();
                return crate::core::blocking::run_db("goal.load_record", move || {
                    Ok(storage
                        .get_session_goal(&user_id, &session_id)?
                        .filter(|record| record.goal_id == goal_id))
                })
                .await;
            }
            Ok(None)
        }
        None => Ok(None),
    }
}

/// WebSocket command face: execute one parsed goal command and return the
/// goal projection payload (snake_case) after the change.
pub async fn apply_goal_command(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<Option<Value>> {
    let outcome = native_session_goal_outcome(state, user, session_id, command).await?;
    Ok(outcome.goal)
}

async fn native_session_goal_outcome(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    command: GoalCommand,
) -> anyhow::Result<GoalMutationOutcome> {
    let user_id = user.user_id.trim();
    let session_id = session_id.trim();
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .map_err(|error| anyhow::anyhow!("load session failed: {error}"))?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let echo = match &command {
        GoalCommand::Create { objective } => Some(format!("/goal {objective}")),
        GoalCommand::Edit { objective } => Some(format!("/goal edit {objective}")),
        GoalCommand::Pause => Some("/goal pause".to_string()),
        GoalCommand::Resume => Some("/goal resume".to_string()),
        GoalCommand::Clear => Some("/goal clear".to_string()),
        GoalCommand::Show | GoalCommand::InvalidEdit => None,
    };
    // Persist the /goal command as a durable completed user round so the
    // bubble survives reloads. Finish admission before create/resume can wake
    // the driver, and do not register a running monitor for this control turn.
    let mut user_round = None;
    if let Some(echo) = echo {
        let input = json!({
            "role": "user",
            "content": echo,
            "meta": {"type": "goal_command", "goal_command": true},
        });
        let accepted = state
            .kernel
            .orchestrator
            .committer
            .accept_turn(user_id, session_id, &input)
            .await?;
        let accepted_round = accepted["user_turn_index"].as_i64().unwrap_or(1);
        user_round = Some(accepted_round);
        let storage = state.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        if let Some(turn) =
            crate::core::blocking::run_db("thread_log.goal_command.find_turn", move || {
                storage.find_thread_turn_id(&owner, &thread, accepted_round)
            })
            .await?
        {
            state
                .kernel
                .orchestrator
                .committer
                .update_turn(
                    user_id,
                    session_id,
                    &turn,
                    "completed",
                    "",
                    &json!({"stop_reason": "goal_command"}),
                )
                .await?;
        }
    }
    let (_, goal_value) = execute_goal_command(
        goal_service(state).as_ref(),
        state.storage.clone(),
        user_id,
        session_id,
        command,
    )
    .await?;
    // Create/resume arm the goal: let the driver re-evaluate immediately.
    if goal_value.is_some() {
        goal_service(state)
            .driver()
            .notify_goal_changed(user_id, session_id);
    }
    Ok(GoalMutationOutcome {
        goal: goal_value,
        user_round,
    })
}

fn goal_command_from_payload(
    payload: GoalUpsertPayload,
    has_current_goal: bool,
) -> Result<GoalCommand, Response> {
    if let Some(action) = payload
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match action {
            "pause" => Ok(GoalCommand::Pause),
            "resume" => Ok(GoalCommand::Resume),
            other => Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("unknown goal action: {other}"),
            )),
        };
    }
    let objective = payload
        .objective
        .as_deref()
        .map(goal::validate_objective)
        .transpose()
        .map_err(bad_request)?;
    let Some(objective) = objective else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    };
    Ok(if has_current_goal {
        GoalCommand::Edit { objective }
    } else {
        GoalCommand::Create { objective }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config_store::ConfigStore;
    use crate::services::goal::{GoalDriverHost, GoalRoundTag, GoalView};
    use crate::state::AppStateInitOptions;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use futures::future::BoxFuture;
    use std::sync::Mutex;
    use tower::ServiceExt;

    struct GoalHost {
        state: Arc<AppState>,
        submitted: Mutex<Vec<GoalRoundTag>>,
    }

    impl GoalDriverHost for GoalHost {
        fn host_session_available(&self, user: &str, session: &str) -> BoxFuture<'_, bool> {
            let runtime = self.state.kernel.thread_runtime.clone();
            let user = user.to_string();
            let session = session.to_string();
            Box::pin(async move {
                runtime
                    .is_session_available_for_submit(&user, &session)
                    .await
            })
        }

        fn host_submit_goal_round(
            &self,
            _: &str,
            _: &str,
            tag: GoalRoundTag,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async move {
                self.submitted.lock().unwrap().push(tag);
                Ok(())
            })
        }

        fn host_load_goal_view(
            &self,
            user: &str,
            session: &str,
        ) -> BoxFuture<'_, anyhow::Result<Option<GoalView>>> {
            let state = self.state.clone();
            let user = user.to_string();
            let session = session.to_string();
            Box::pin(async move {
                goal_service(&state)
                    .get_view(&state.storage, &user, &session)
                    .await
            })
        }

        fn host_pause_goal(
            &self,
            _: &str,
            _: &str,
            _: goal::GoalRef,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async { unreachable!("creation must not pause the goal") })
        }

        fn host_block_goal(
            &self,
            _: &str,
            _: &str,
            _: goal::GoalRef,
            _: goal::GoalBlockReason,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async { unreachable!("creation must not block the goal") })
        }
    }

    #[test]
    fn objective_put_creates_persists_and_drives_then_edits_existing_goal() {
        // Match the real runner tests: Windows debug futures need additional
        // stack on both the block_on thread and its runtime workers.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_stack_size(16 * 1024 * 1024)
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(exercise_objective_put_and_runtime());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    async fn exercise_objective_put_and_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = dir.path().join("fixture.db").to_string_lossy().into();
        config.workspace.root = dir.path().join("workspace").to_string_lossy().into();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(dir.path().join("fixture.yaml"));
        let saved = config.clone();
        config_store.update(|c| *c = saved.clone()).await.unwrap();
        let state = Arc::new(
            AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
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
        let token = state
            .user_store
            .create_session_token(&user.user_id)
            .unwrap()
            .token;
        let session = "fixture-thread";
        state
            .storage
            .upsert_chat_session(&crate::storage::ChatSessionRecord {
                session_id: session.into(),
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
        let host = Arc::new(GoalHost {
            state: state.clone(),
            submitted: Mutex::new(vec![]),
        });
        goal_service(&state).set_driver_host(host.clone());
        let app = router().with_state(state.clone());
        let mut goal_id = String::new();
        for (index, objective) in ["Complete the example task", "Revise the example task"]
            .iter()
            .enumerate()
        {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/wunder/chat/sessions/{session}/goal"))
                        .header("authorization", format!("Bearer {token}"))
                        .header("content-type", "application/json")
                        .body(Body::from(json!({"objective": objective}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let value = &body["data"]["goal"];
            assert_eq!(value["objective"], *objective);
            assert_eq!(value["phase"], "active");
            assert_eq!(value["activation"], "armed");
            assert_eq!(value["revision"], index + 1);
            assert_eq!(body["data"]["user_round"], index + 1);
            if index == 0 {
                goal_id = value["goal_id"].as_str().unwrap().into();
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    while host.submitted.lock().unwrap().is_empty() {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                })
                .await
                .expect("new goal schedules a model round");
                assert_eq!(host.submitted.lock().unwrap()[0].goal_id, goal_id);
                let turns = state
                    .storage
                    .list_thread_turns(&user.user_id, session, None, 1)
                    .unwrap();
                assert_eq!(turns[0]["summary"], format!("/goal {objective}"));
            } else {
                assert_eq!(value["goal_id"], goal_id);
            }
        }

        // Exercise the real runtime after the command-only host: two bounded
        // model rounds must complete without another human request or wake.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mock = Router::new().route("/v1/chat/completions", axum::routing::post(
            |Json(payload): Json<Value>| async move {
                let value = if payload["stream"] == true {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Example progress.\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_string()
                } else {
                    json!({"choices":[{"message":{"role":"assistant","content":"Example progress."},"finish_reason":"stop"}]}).to_string()
                };
                axum::http::Response::builder()
                    .header("content-type", if payload["stream"] == true { "text/event-stream" } else { "application/json" })
                    .body(Body::from(value)).unwrap()
            },
        ));
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        state
            .config_store
            .update(|config| {
                config.llm.default = "fixture-model".into();
                config.llm.models.clear();
                config.llm.models.insert(
                    "fixture-model".into(),
                    crate::config::LlmModelConfig {
                        enable: Some(true),
                        provider: Some("openai_compatible".into()),
                        base_url: Some(format!("http://{address}/v1")),
                        api_key: Some("fixture-key".into()),
                        model: Some("fixture-model".into()),
                        max_context: Some(16384),
                        max_output: Some(64),
                        stream: Some(true),
                        tool_call_mode: Some("tool_call".into()),
                        ..Default::default()
                    },
                );
                config.agent_queue.enabled = false;
            })
            .await
            .unwrap();
        let service = goal_service(&state);
        let view = service
            .get_view(&state.storage, &user.user_id, session)
            .await
            .unwrap()
            .unwrap();
        service
            .edit(
                state.storage.clone(),
                &user.user_id,
                session,
                &goal::GoalRef {
                    goal_id: view.record.goal_id,
                    revision: view.record.revision,
                },
                "Complete the example task",
                Some(2),
            )
            .await
            .unwrap();
        service.driver().reset_session(session);
        service.set_driver_host(state.kernel.thread_runtime.clone());
        service.driver().notify_goal_changed(&user.user_id, session);
        let completed = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let turns = state
                    .storage
                    .list_thread_turns(&user.user_id, session, None, 8)
                    .unwrap();
                if turns.len() == 4 && turns.iter().all(|turn| turn["status"] == "completed") {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await;
        server.abort();
        assert!(
            completed.is_ok(),
            "real runtime must finish two goal rounds: {:?}",
            state
                .storage
                .list_thread_turns(&user.user_id, session, None, 8)
                .unwrap()
        );
        assert_eq!(
            service
                .get_view(&state.storage, &user.user_id, session)
                .await
                .unwrap()
                .unwrap()
                .record
                .rounds_started,
            2
        );
    }

    #[test]
    fn objective_upsert_creates_when_session_has_no_goal() {
        let command = goal_command_from_payload(
            GoalUpsertPayload {
                objective: Some("finish the task".into()),
                max_goal_rounds: None,
                action: None,
            },
            false,
        )
        .expect("valid objective");
        assert_eq!(
            command,
            GoalCommand::Create {
                objective: "finish the task".into()
            }
        );
    }

    #[test]
    fn objective_upsert_edits_when_session_has_goal() {
        let command = goal_command_from_payload(
            GoalUpsertPayload {
                objective: Some("revise the task".into()),
                max_goal_rounds: None,
                action: None,
            },
            true,
        )
        .expect("valid objective");
        assert_eq!(
            command,
            GoalCommand::Edit {
                objective: "revise the task".into()
            }
        );
    }
}

fn ensure_session_owner(
    state: &AppState,
    user_id: &str,
    session_id: &str,
) -> Result<crate::storage::ChatSessionRecord, Response> {
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .map_err(bad_request)?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))
}

fn normalize_session_id(session_id: String) -> Result<String, Response> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    Ok(session_id)
}

fn bad_request(err: impl ToString) -> Response {
    error_response(StatusCode::BAD_REQUEST, err.to_string())
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
