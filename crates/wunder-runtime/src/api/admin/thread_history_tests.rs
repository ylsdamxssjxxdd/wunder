use super::*;
use crate::config_store::ConfigStore;
use crate::state::AppStateInitOptions;
use crate::storage::{ChatSessionRecord, StressThreadScenario, StressThreadSpec};
use axum::body::{to_bytes, Body};
use axum::http::Request;
use tower::ServiceExt;
use wunder_core::thread_store::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_thread_details_read_canonical_body_with_thread_identity() {
    verify_admin_thread_details(None).await;
}

#[cfg(feature = "postgres-storage")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
async fn postgres_admin_thread_details_and_preview_snapshot() {
    verify_admin_thread_details(Some(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap())).await;
}

async fn verify_admin_thread_details(dsn: Option<String>) {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    let postgres = dsn.is_some();
    if let Some(dsn) = dsn {
        config.storage.backend = "postgres".into();
        config.storage.postgres.dsn = dsn;
    }
    #[cfg(feature = "postgres-storage")]
    if postgres {
        use crate::storage::UserAccountStore;
        let storage =
            crate::storage::PostgresStorage::new(config.storage.postgres.dsn.clone(), 5, 4)
                .unwrap();
        storage
            .upsert_org_unit(&crate::storage::OrgUnitRecord {
                unit_id: "sample-audit-unit".into(),
                parent_id: None,
                name: "sample unit".into(),
                level: 1,
                path: "/sample-audit-unit".into(),
                path_name: "sample unit".into(),
                sort_order: 0,
                leader_ids: vec![],
                created_at: 1.0,
                updated_at: 1.0,
            })
            .unwrap();
    }
    config.storage.data_root = root.path().join("data").to_string_lossy().into();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let options = if postgres {
        AppStateInitOptions::server_default().with_start_thread_runtime(false)
    } else {
        AppStateInitOptions::cli_default()
    };
    let state = Arc::new(AppState::new_with_options(store, config, options).unwrap());
    let owner = "sample-owner";
    for name in ["sample-thread", "sample-other"] {
        state
            .thread_store
            .generate_stress_thread(
                owner,
                &StressThreadSpec {
                    session_id: name.into(),
                    title: "sample history".into(),
                    workspace_id: None,
                    user_rounds: 1,
                    model_rounds_per_turn: 2,
                    seed: Some(7),
                    scenario: StressThreadScenario::Complex,
                },
                |_, _| {},
            )
            .await
            .unwrap();
        state
            .storage
            .upsert_chat_session(&ChatSessionRecord {
                session_id: name.into(),
                user_id: owner.into(),
                title: "sample history".into(),
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
            })
            .unwrap();
    }
    let thread = ThreadId::new("sample-thread").unwrap();
    let turn = state
        .storage
        .list_thread_turns(owner, thread.as_str(), None, 1)
        .unwrap()[0]["turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let content = "sample complete reply 中文🙂 ".repeat(4000);
    state
        .thread_store
        .append_canonical(
            &thread,
            vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
                CanonicalItem {
                    item_id: ItemId::new("sample-body").unwrap(),
                    turn_id: TurnId::new(turn).unwrap(),
                    root_turn_id: None,
                    created_at_ms: 20,
                    kind: CanonicalItemKind::AssistantMessage,
                    payload: json!({"content":content}),
                    display_output: None,
                    display_output_truncated: false,
                },
            ))],
        )
        .await
        .unwrap();
    let app = router().with_state(state.clone());
    let item_path = "/wunder/admin/monitor/sample-thread/thread-log/items/sample-body";
    assert!(crate::auth::is_admin_path(item_path));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(item_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["data"]["payload"]["content"], content);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/wunder/admin/monitor/sample-thread/thread-log/snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let item = body["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == "sample-body")
        .unwrap();
    if postgres {
        assert_eq!(item["preview_only"], true);
        assert!(item["payload"]["content"].as_str().unwrap().len() < content.len());
    } else {
        assert_eq!(item["payload"]["content"], content);
    }
    for path in [
        "/wunder/admin/monitor/sample-other/thread-log/items/sample-body",
        "/wunder/admin/monitor/sample-missing/thread-log/items/sample-body",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    for name in ["sample-thread", "sample-other"] {
        state
            .storage
            .delete_thread_log_by_session(owner, name)
            .unwrap();
        state.storage.delete_chat_session(owner, name).unwrap();
    }
    if postgres {
        state.storage.delete_org_unit("sample-audit-unit").unwrap();
    }
}
