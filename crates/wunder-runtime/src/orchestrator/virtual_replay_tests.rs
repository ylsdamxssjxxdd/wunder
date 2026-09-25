use super::*;
use crate::state::{AppState, AppStateInitOptions};

#[tokio::test]
async fn virtual_provider_enforces_capabilities_and_completes_tool_exchange() {
    use wunder_core::virtual_model::VirtualModelOptions;
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
    let emitter = EventEmitter::new(
        "session_1".into(),
        "user_1".into(),
        None,
        None,
        state.monitor.clone(),
        false,
        0,
        None,
    );
    let mut model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        max_context: Some(16),
        max_output: Some(16),
        ..Default::default()
    };
    let messages = [json!({"role":"user","content":"abcd"})];
    let orchestrator = &state.kernel.orchestrator;
    let error = orchestrator
        .call_llm(
            &model,
            &messages,
            "user_1",
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            false,
            false,
            false,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONTEXT_WINDOW_EXCEEDED");
    model.max_context = Some(4096);
    model.max_output = Some(256);
    let image = [
        json!({"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]}),
    ];
    let error = orchestrator
        .call_llm(
            &model,
            &image,
            "user_1",
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            false,
            false,
            false,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (error.code(), error.retryable()),
        ("INVALID_REQUEST", false)
    );
    assert!(error.message().contains("unsupported_image"));
    model.simulation = Some(VirtualModelOptions {
        support_reasoning: false,
        ..Default::default()
    });
    let replay = [
        json!({"role":"user","content":"test"}),
        json!({"role":"assistant","content":"<tool_call>{\"name\":\"test_tool\",\"arguments\":{}}</tool_call>"}),
        json!({"role":"assistant","content":"Recorded final reply."}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let mut replay_config = state.config_store.get().await;
    replay_config.llm.virtual_replay.logs_root =
        root.path().join("replay").to_string_lossy().into_owned();
    let (updated, log) = crate::services::virtual_llm::store_uploaded_log(
        replay_config,
        crate::services::virtual_llm::VirtualLlmUpload {
            name: "test".into(),
            content: replay,
        },
    )
    .await
    .unwrap();
    state
        .config_store
        .update(|config| *config = updated.clone())
        .await
        .unwrap();
    model.model = Some(log.id);
    state
        .workspace
        .save_session_frozen_tool_call_mode("user_1", "session_1", "function_call");
    model.tool_call_mode = Some("tool_call".into());
    let tools =
        [json!({"type":"function","function":{"name":"test_tool","parameters":{"type":"object"}}})];
    let first = orchestrator
        .call_llm(
            &model,
            &messages,
            "user_1",
            true,
            &emitter,
            "session_1",
            true,
            RoundInfo::new(1, 1),
            false,
            false,
            false,
            Some(&tools),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        first.3,
        Some(
            json!([{"id":"replay_1_1_0","type":"function","function":{"name":"test_tool","arguments":"{}"}}])
        )
    );
    let history = [
        messages[0].clone(),
        json!({"role":"assistant","content":"","tool_calls":first.3}),
        json!({"role":"tool","tool_call_id":"replay_1_1_0","content":"{\"error\":\"test_error\"}"}),
    ];
    let result = orchestrator
        .call_llm(
            &model,
            &history,
            "user_1",
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 2),
            false,
            false,
            false,
            Some(&tools),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        (result.0, result.1, result.3),
        ("Recorded final reply.".into(), String::new(), None)
    );
}

#[tokio::test]
async fn virtual_replay_works_at_zero_balance_without_spending_or_granting_tokens() {
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
        .user_store
        .get_user_by_id(&user.user_id)
        .unwrap()
        .unwrap();
    let model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        ..Default::default()
    };
    state
        .monitor
        .register("session_1", &user.user_id, "agent_1", "input", false, false);
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
    let result = state
        .kernel
        .orchestrator
        .call_llm(
            &model,
            &[json!({"role":"user","content":"A"})],
            &user.user_id,
            false,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            false,
            false,
            false,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!result.0.is_empty());
    assert!(result.2.total > 0);
    let after = state
        .user_store
        .get_user_by_id(&user.user_id)
        .unwrap()
        .unwrap();
    assert_eq!(after.quota_balance, before.quota_balance);
    assert_eq!(after.quota_granted_total, before.quota_granted_total);
    assert_eq!(after.quota_used_total, before.quota_used_total);
    assert_eq!(after.last_quota_grant_date, before.last_quota_grant_date);
    assert_eq!(emitter.accumulated_quota_consumption(), 0);
    assert_eq!(
        state.monitor.get_detail("session_1").unwrap()["session"]["quota_used"],
        json!(0)
    );
}
