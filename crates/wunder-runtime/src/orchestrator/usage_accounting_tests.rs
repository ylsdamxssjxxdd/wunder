use super::*;
use crate::orchestrator::execute_support::build_round_usage_payload;
use crate::state::{AppState, AppStateInitOptions};
use axum::{routing::post, Json, Router};
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
    user.token_balance = 10000;
    user.last_token_grant_date = Some(UserStore::today_string());
    state.user_store.update_user(&user).unwrap();
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
    for (round, visible) in [(1, true), (2, false)] {
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
        (requests.load(Ordering::SeqCst), after.token_balance),
        (4, 9440)
    );
    let detail = state.monitor.get_detail("session_1").unwrap();
    assert_eq!(detail["session"]["consumed_tokens"], json!(560));
    server.abort();
}
