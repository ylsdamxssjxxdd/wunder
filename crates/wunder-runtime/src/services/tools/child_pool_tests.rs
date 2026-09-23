use super::*;
use crate::a2a_store::A2aStore;
use crate::config::Config;
use crate::config::LlmModelConfig;
use crate::config_store::ConfigStore;
use crate::state::{AppState, AppStateInitOptions};
use axum::{routing::post, Json, Router};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_pool_parent_stop_then_resume_and_reassign_same_session() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let counter = calls.clone();
    let capture = observed.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        capture.lock().unwrap().push(body);
        async move {
            if index == 0 { tokio::time::sleep(Duration::from_secs(30)).await; }
            Json(json!({"choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":10,"completion_tokens":1,"total_tokens":11}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = dir.path().join("test.db").to_string_lossy().into();
    config.workspace.root = dir.path().join("workspace").to_string_lossy().into();
    config.llm.default = "test".into();
    config.llm.models.clear();
    config.llm.models.insert(
        "test".into(),
        LlmModelConfig {
            provider: Some("openai".into()),
            model: Some("test".into()),
            base_url: Some(format!("http://{address}/v1")),
            stream: Some(false),
            timeout_s: Some(60),
            max_rounds: Some(2),
            ..Default::default()
        },
    );
    let store = ConfigStore::new(dir.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state =
        AppState::new_with_options(store, config.clone(), AppStateInitOptions::cli_default())
            .unwrap();
    let user = state
        .user_store
        .create_user(
            "user",
            None,
            "test-password",
            None,
            None,
            vec!["user".into()],
            "active",
            false,
        )
        .unwrap();
    let a2a = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let mut context = ToolContext {
        user_id: &user.user_id,
        session_id: "parent",
        workspace_id: &user.user_id,
        agent_id: None,
        user_round: Some(1),
        model_round: Some(1),
        is_admin: true,
        storage: state.storage.clone(),
        orchestrator: Some(state.kernel.orchestrator.clone()),
        monitor: Some(state.monitor.clone()),
        beeroom_realtime: None,
        workspace: state.workspace.clone(),
        lsp_manager: state.lsp_manager.clone(),
        config: &config,
        a2a_store: &a2a,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };
    for (id, parent) in [("parent", None), ("child", Some("parent"))] {
        state
            .storage
            .upsert_chat_session(&ChatSessionRecord {
                session_id: id.into(),
                user_id: user.user_id.clone(),
                title: "thread".into(),
                status: "active".into(),
                created_at: 1.0,
                updated_at: 1.0,
                last_message_at: 1.0,
                agent_id: None,
                tool_overrides: vec![],
                parent_session_id: parent.map(str::to_string),
                parent_message_id: None,
                spawn_label: None,
                spawned_by: parent.map(|_| "model".into()),
            })
            .unwrap();
    }
    state
        .monitor
        .register("parent", &user.user_id, "", "task", true, false);
    let accepted = subagent_control::execute(
        &context,
        &json!({"action":"send","session_id":"child","message":"first"}),
    )
    .await
    .unwrap();
    let run_id = accepted["data"]["run_id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    state
        .kernel
        .thread_runtime
        .cancel_session_activity(&user.user_id, "parent", "test")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let run = state.storage.get_session_run(run_id).unwrap().unwrap();
            if run.status == "cancelled" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        state
            .storage
            .get_chat_session(&user.user_id, "child")
            .unwrap()
            .unwrap()
            .status,
        "active"
    );
    let pool = subagent_control::execute(&context, &json!({"action":"list"}))
        .await
        .unwrap();
    assert_eq!(pool["data"]["items"][0]["status"], "cancelled");

    state
        .monitor
        .register("parent", &user.user_id, "", "next", true, false);
    context.user_round = Some(2);
    for (action, message) in [("resume", "second"), ("send", "third")] {
        let result = subagent_control::execute(
            &context,
            &json!({"action":action,"session_id":"child","message":message,"timeout_seconds":15}),
        )
        .await
        .unwrap();
        assert_eq!(
            (
                result["state"].as_str(),
                result["data"]["session_id"].as_str()
            ),
            (Some("completed"), Some("child"))
        );
        let run = state
            .storage
            .get_session_run(result["data"]["run_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(run.metadata.unwrap()["parent_user_round"], 2);
    }
    let captured = observed.lock().unwrap();
    assert_eq!(captured.len(), 3);
    let systems = captured
        .iter()
        .map(|body| {
            body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "system")
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(systems, vec![systems[0].clone(); 3]);
    assert!(captured[2]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["content"] == "second"));
    server.abort();
}
