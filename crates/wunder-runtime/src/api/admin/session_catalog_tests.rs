use super::*;
use crate::config_store::ConfigStore;
use crate::state::AppStateInitOptions;
use crate::storage::{ChatSessionRecord, StorageBackend};
use axum::body::{to_bytes, Body};
use axum::http::Request;
use tower::ServiceExt;

fn session(id: &str, user_id: &str, status: &str) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: id.into(),
        user_id: user_id.into(),
        title: "thread".into(),
        status: status.into(),
        created_at: 1.0,
        updated_at: 1.0,
        last_message_at: 1.0,
        agent_id: None,
        tool_overrides: vec![],
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    }
}

fn verify_catalog_storage(storage: &dyn StorageBackend) {
    let prefix = uuid::Uuid::new_v4().simple().to_string();
    let active_id = format!("{prefix}-active");
    let archived_id = format!("{prefix}-archived");
    let other_id = format!("{prefix}-other");
    let mut ids = (0..220)
        .map(|i| format!("{prefix}-{i}"))
        .collect::<Vec<_>>();
    ids.extend([active_id.clone(), archived_id.clone(), other_id.clone()]);
    for record in [
        session(&active_id, "user-a", "active"),
        session(&archived_id, "user-a", "archived"),
        session(&other_id, "user-b", "active"),
    ] {
        storage.upsert_chat_session(&record).unwrap();
    }
    assert_eq!(
        storage
            .list_active_chat_session_ids("user-a", &ids)
            .unwrap(),
        vec![active_id.clone()]
    );
    assert_eq!(
        storage.get_chat_session_owner(&active_id).unwrap(),
        Some("user-a".into())
    );
    assert_eq!(
        storage.delete_chat_session("user-b", &active_id).unwrap(),
        0
    );
    assert_eq!(
        storage.delete_chat_session("user-a", &active_id).unwrap(),
        1
    );
    assert_eq!(storage.get_chat_session_owner(&active_id).unwrap(), None);
    assert_eq!(
        storage
            .list_active_chat_session_ids("user-a", &ids)
            .unwrap(),
        Vec::<String>::new()
    );
    storage.delete_chat_session("user-a", &archived_id).unwrap();
    storage.delete_chat_session("user-b", &other_id).unwrap();
}

#[test]
fn session_catalog_sqlite_ownership_and_batched_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let storage =
        crate::storage::SqliteStorage::new(dir.path().join("catalog.db").to_string_lossy().into());
    verify_catalog_storage(&storage);
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires isolated PostgreSQL via WUNDER_CATALOG_TEST_DSN"]
fn session_catalog_postgres_ownership_and_batched_reconciliation() {
    let storage = crate::storage::PostgresStorage::new(
        std::env::var("WUNDER_CATALOG_TEST_DSN").unwrap(),
        5,
        8,
    )
    .unwrap();
    verify_catalog_storage(&storage);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_catalog_admin_delete_without_monitor_reconciles_across_pages() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = dir.path().join("catalog.db").to_string_lossy().into();
    config.workspace.root = dir.path().join("workspace").to_string_lossy().into();
    let config_store = ConfigStore::new(dir.path().join("config.yaml"));
    config_store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state = Arc::new(
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .unwrap(),
    );
    let user = state
        .user_store
        .create_user(
            "user-a",
            None,
            "test-password",
            Some("A"),
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
    for id in ["session-a", "session-b", "session-c"] {
        state
            .storage
            .upsert_chat_session(&session(id, &user.user_id, "active"))
            .unwrap();
    }
    assert!(state.monitor.get_record("session-a").is_none());
    let deleted = admin_monitor_delete(State(state.clone()), AxumPath("session-a".into()))
        .await
        .unwrap();
    assert_eq!(deleted.0.get("ok"), Some(&json!(true)));
    assert_eq!(
        state.storage.get_chat_session_owner("session-a").unwrap(),
        None
    );
    let app = crate::api::chat::router().with_state(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri(
                    "/wunder/chat/sessions?limit=1&known_session_ids=session-a,session-b,session-c",
                )
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(body["data"]["total"], json!(2));
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["data"]["unavailable_session_ids"],
        json!(["session-a"])
    );
}
