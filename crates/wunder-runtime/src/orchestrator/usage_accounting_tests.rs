use super::*;
use crate::orchestrator::execute_support::build_round_usage_payload;
use crate::state::{AppState, AppStateInitOptions};
use axum::{http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn usage_accounting_includes_rejected_calls_empty_responses_and_compaction() {
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let index = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if index == 8 {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"unavailable"})),
                    )
                        .into_response();
                }
                if (5..=7).contains(&index) {
                    let message = if index < 7 {
                        "reasoning_effort unsupported"
                    } else {
                        "stream_options unsupported"
                    };
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":{"message":message}})),
                    )
                        .into_response();
                }
                let message = match index {
                    0 => {
                        json!({"role":"assistant", "tool_calls":[{"id":"call_1", "type":"function",
                    "function":{"name":"tool_1", "arguments":"{"}}]})
                    }
                    2 => json!({"role":"assistant", "content":""}),
                    _ => json!({"role":"assistant", "content":"ok"}),
                };
                Json(
                    json!({"choices":[{"message":message,"finish_reason":"stop"}],
                "usage":{"prompt_tokens":100,"completion_tokens":40,"total_tokens":140,
                    "completion_tokens_details":{"reasoning_tokens":30}}}),
                )
                .into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root.path().join("state.db").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state =
        AppState::new_with_options(store, config, AppStateInitOptions::cli_default()).unwrap();
    let mut user = state
        .user_store
        .create_user(
            "user_1",
            None,
            "test-password",
            None,
            None,
            vec!["user".into()],
            "active",
            false,
        )
        .unwrap();
    user.quota_balance = 10000;
    user.last_quota_grant_date = Some(UserStore::today_string());
    state.storage.upsert_user_account(&user).unwrap();
    state
        .monitor
        .register("session_1", &user.user_id, "agent_1", "input", true, false);
    let emitter = EventEmitter::new(
        "session_1".into(),
        user.user_id.clone(),
        None,
        None,
        state.monitor.clone(),
        false,
        0,
        None,
    );
    let model = LlmModelConfig {
        provider: Some("openai".into()),
        model: Some("model_1".into()),
        base_url: Some(format!("http://{address}/v1")),
        ..Default::default()
    };
    let tools = [json!({"type":"function","function":{"name":"tool_1",
        "parameters":{"type":"object","properties":{}}}})];
    // A completed empty output is accounted once and returned to the turn guard.
    // The next explicit call represents its bounded recovery, not a hidden retry.
    for (round, visible) in [(1, true), (2, false), (3, true), (4, true)] {
        state
            .kernel
            .orchestrator
            .call_llm(
                &model,
                &[json!({"role":"user","content":"input"})],
                &user.user_id,
                false,
                &emitter,
                "session_1",
                false,
                RoundInfo::new(1, round),
                visible,
                true,
                false,
                Some(&tools),
                None,
            )
            .await
            .unwrap();
    }
    let cumulative = TokenUsage {
        input: 400,
        output: 40,
        total: 560,
        reasoning: Some(120),
        estimated: false,
    };
    assert_eq!(emitter.accumulated_usage(), cumulative);
    emitter
        .emit(
            "round_usage",
            build_round_usage_payload(&cumulative, 140, RoundInfo::new(1, 2)),
        )
        .await;
    let after = state
        .user_store
        .get_user_by_id(&user.user_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        (requests.load(Ordering::SeqCst), after.quota_balance),
        (4, 9996)
    );
    let detail = state.monitor.get_detail("session_1").unwrap();
    assert_eq!(detail["session"]["consumed_tokens"], json!(560));
    assert_eq!(detail["session"]["quota_used"], json!(4));
    // The last credit is usable; the following request must fail before HTTP dispatch.
    state
        .storage
        .set_user_quota_balance(&user.user_id, &UserStore::today_string(), 1000, 1)
        .unwrap();
    for (round, should_succeed) in [(3, true), (4, false)] {
        let result = state
            .kernel
            .orchestrator
            .call_llm(
                &model,
                &[json!({"role":"user","content":"input"})],
                &user.user_id,
                false,
                &emitter,
                "session_1",
                false,
                RoundInfo::new(1, round),
                true,
                true,
                false,
                Some(&tools),
                None,
            )
            .await;
        if should_succeed {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err().code(), "USER_QUOTA_INSUFFICIENT");
        }
    }
    let after = state
        .storage
        .get_user_account(&user.user_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            requests.load(Ordering::SeqCst),
            after.quota_balance,
            after.quota_used_total
        ),
        (5, 0, 5)
    );
    assert_eq!(
        state.monitor.session_usage_summaries(&["session_1".into()])["session_1"].2,
        Some(5)
    );
    // Internal reasoning and streaming fallbacks must also debit before dispatch.
    for (balance, stream, effort, expected_requests) in [
        (2, false, Some("xhigh".to_string()), 7),
        (1, true, None, 8),
        (1, true, None, 9),
    ] {
        state
            .storage
            .set_user_quota_balance(&user.user_id, &UserStore::today_string(), 1000, balance)
            .unwrap();
        let mut fallback_model = model.clone();
        fallback_model.reasoning_effort = effort;
        let error = state
            .kernel
            .orchestrator
            .call_llm(
                &fallback_model,
                &[json!({"role":"user","content":"input"})],
                &user.user_id,
                false,
                &emitter,
                "session_1",
                stream,
                RoundInfo::new(2, 1),
                true,
                true,
                false,
                None,
                None,
            )
            .await
            .unwrap_err();
        let after = state
            .storage
            .get_user_account(&user.user_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                error.code(),
                requests.load(Ordering::SeqCst),
                after.quota_balance,
                after.quota_used_total
            ),
            (
                "USER_QUOTA_INSUFFICIENT",
                expected_requests,
                0,
                expected_requests as i64
            )
        );
    }
    server.abort();
}

#[tokio::test]
async fn administrator_requests_track_thread_quota_without_debiting_account() {
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async {
                Json(json!({
                    "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 8, "completion_tokens": 2, "total_tokens": 10}
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root.path().join("state.db").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state =
        AppState::new_with_options(store, config, AppStateInitOptions::cli_default()).unwrap();
    let mut user = state
        .user_store
        .create_user(
            "user_1",
            None,
            "test-password",
            None,
            None,
            vec!["user".into()],
            "active",
            false,
        )
        .unwrap();
    user.quota_balance = 0;
    user.last_quota_grant_date = Some(UserStore::today_string());
    state.storage.upsert_user_account(&user).unwrap();
    let before = state
        .storage
        .get_user_account(&user.user_id)
        .unwrap()
        .unwrap();
    state
        .monitor
        .register("session_1", &user.user_id, "agent_1", "input", true, false);
    let emitter = EventEmitter::new(
        "session_1".into(),
        user.user_id.clone(),
        None,
        None,
        state.monitor.clone(),
        true,
        0,
        None,
    );
    let model = LlmModelConfig {
        provider: Some("openai".into()),
        model: Some("model_1".into()),
        base_url: Some(format!("http://{address}/v1")),
        ..Default::default()
    };

    state
        .kernel
        .orchestrator
        .call_llm(
            &model,
            &[json!({"role": "user", "content": "input"})],
            &user.user_id,
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            true,
            true,
            false,
            None,
            None,
        )
        .await
        .unwrap();

    let after = state
        .storage
        .get_user_account(&user.user_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            after.quota_balance,
            after.quota_granted_total,
            after.quota_used_total
        ),
        (
            before.quota_balance,
            before.quota_granted_total,
            before.quota_used_total
        )
    );
    let detail = state.monitor.get_detail("session_1").unwrap();
    assert_eq!(detail["session"]["quota_used"], json!(1));
    assert!(detail["events"].as_array().is_some_and(|events| {
        events.iter().any(|event| {
            event["type"] == "model_request_usage"
                && event["data"]["request_count"] == json!(1)
                && event["data"]["billable"] == json!(false)
        })
    }));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    server.abort();
}
