use super::*;
use crate::state::{AppState, AppStateInitOptions};

#[tokio::test]
async fn virtual_provider_enforces_capabilities_and_completes_tool_exchange() {
    use wunder_core::virtual_model::VirtualModelOptions;
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
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
            None,
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
            None,
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
            Some(&tools),
            None,
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
            Some(&tools),
            None,
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
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
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
    use wunder_core::virtual_model::VirtualModelOptions;
    let model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        // Disable simulated faults so this test stays fully deterministic.
        simulation: Some(VirtualModelOptions {
            error_rate: 0.0,
            disconnect_rate: 0.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    state
        .monitor
        .register("session_1", &user.user_id, "agent_1", "input", false);
    let emitter = EventEmitter::new(
        "session_1".into(),
        user.user_id.clone(),
        None,
        None,
        state.monitor.clone(),
        false,
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
            None,
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
    assert_eq!(emitter.accumulated_model_requests(), 0);
    assert_eq!(
        state.monitor.get_detail("session_1").unwrap()["session"]["quota_used"],
        json!(0)
    );
}

/// 模拟模型输出必须与真实模型走同一条持久化链路：assistant 文本块与
/// thread item 全部落库，会话历史里可见。
#[tokio::test]
async fn random_simulation_persists_records_like_real_models() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state =
        AppState::new_with_options(store, config, AppStateInitOptions::cli_default()).unwrap();
    let user = state
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
    state
        .monitor
        .register("session_1", &user.user_id, "agent_1", "input", false);
    let committer = state.kernel.orchestrator.committer.clone();
    let emitter = EventEmitter::new(
        "session_1".into(),
        user.user_id.clone(),
        None,
        None,
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(committer.clone());
    let accepted = committer
        .accept_turn(
            &user.user_id,
            "session_1",
            &json!({"role":"user","content":"你好","client_message_id":"cm_random_sim_1"}),
        )
        .await
        .unwrap();
    let turn_id = accepted["turn_id"].as_str().unwrap().to_string();
    let user_round = accepted["user_turn_index"].as_i64().unwrap_or(1);
    committer
        .freeze_session(
            &user.user_id,
            "session_1",
            "frozen fixture instructions",
            "fixture-workspace",
        )
        .await
        .unwrap();
    committer
        .begin_turn(&user.user_id, "session_1", &turn_id, "virtual_replay")
        .await
        .unwrap();
    emitter.bind_turn(&turn_id, user_round);
    use wunder_core::virtual_model::VirtualModelOptions;
    let model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        // Disable simulated faults so this test stays fully deterministic.
        simulation: Some(VirtualModelOptions {
            error_rate: 0.0,
            disconnect_rate: 0.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = state
        .kernel
        .orchestrator
        .call_llm(
            &model,
            &[json!({"role":"user","content":"你好"})],
            &user.user_id,
            false,
            &emitter,
            "session_1",
            true,
            RoundInfo::new(user_round, 1),
            true,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!result.0.trim().is_empty());

    // The completed semantic item contains all text needed to reopen history.
    let item_id = format!("{turn_id}:text-1");
    let item = state
        .storage
        .get_thread_item(&user.user_id, "session_1", &item_id, false)
        .unwrap()
        .unwrap();
    assert!(!item["payload"]["content"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .is_empty());
    let context_items = state
        .storage
        .load_thread_context_items(&user.user_id, "session_1", 20, false)
        .unwrap();
    assert!(
        context_items
            .iter()
            .any(|item| item["kind"] == json!("assistant_message")),
        "assistant_message item must appear in thread context"
    );
}

/// 虚拟模型的轮次约定必须由本回合权威的人类输入推导，而不是从（可能被压缩改写
/// 的）请求消息里回捞。历史里没有数字时，只有显式传来的 `user_question` 能决定
/// 本次计划轮数，否则同一回合跑到中途会换计划，轮次数不再等于用户给的数字。
#[tokio::test]
async fn user_question_drives_virtual_round_plan_after_history_rewrite() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
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
        None,
    );
    use wunder_core::virtual_model::VirtualModelOptions;
    let model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        simulation: Some(VirtualModelOptions {
            error_rate: 0.0,
            disconnect_rate: 0.0,
            ..Default::default()
        }),
        max_context: Some(8192),
        max_output: Some(1024),
        ..Default::default()
    };
    let tools =
        [json!({"type":"function","function":{"name":"读取文件","parameters":{"type":"object"}}})];
    // 压缩后的历史：最新一条“用户”文本是摘要，不含任何数字。
    let compacted = [
        json!({"role":"system","content":"system"}),
        json!({"role":"user","content":"以下是之前对话的结构化摘要，可忽略。"}),
    ];
    let orchestrator = &state.kernel.orchestrator;
    let turn = orchestrator
        .call_llm(
            &model,
            &compacted,
            "user_1",
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            false,
            false,
            Some(&tools),
            None,
            Some("4"),
        )
        .await
        .unwrap();
    assert!(
        turn.3.is_some(),
        "plan says 4 rounds, so round 1 must emit tool calls"
    );
    // 不传权威输入、只靠被改写的历史时，退化成随机计划（不保证有工具调用）；
    // 这里只断言不会崩，语义由上面的断言保证。
    let _ = orchestrator
        .call_llm(
            &model,
            &compacted,
            "user_1",
            true,
            &emitter,
            "session_1",
            false,
            RoundInfo::new(1, 1),
            false,
            false,
            Some(&tools),
            None,
            None,
        )
        .await
        .unwrap();
}
