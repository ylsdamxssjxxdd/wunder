use super::*;
use crate::state::{AppState, AppStateInitOptions};

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
    user.token_balance = 0;
    user.last_token_grant_date = Some(UserStore::today_string());
    state.user_store.update_user(&user).unwrap();
    let before = state
        .user_store
        .get_user_by_id(&user.user_id)
        .unwrap()
        .unwrap();
    let model = LlmModelConfig {
        provider: Some("virtual_replay".into()),
        ..Default::default()
    };
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
    assert_eq!(after.token_balance, before.token_balance);
    assert_eq!(after.token_granted_total, before.token_granted_total);
    assert_eq!(after.token_used_total, before.token_used_total);
    assert_eq!(after.last_token_grant_date, before.last_token_grant_date);
}
