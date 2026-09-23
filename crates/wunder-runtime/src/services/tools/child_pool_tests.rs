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
    run_child_pool_scenario(false, None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_pool_running_messages_reports_and_frozen_prompt() {
    run_child_pool_scenario(true, None).await;
}

#[cfg(feature = "postgres-storage")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated PostgreSQL via WUNDER_SUBAGENT_TEST_DSN"]
async fn child_pool_postgres_messages() {
    run_child_pool_scenario(
        true,
        Some(std::env::var("WUNDER_SUBAGENT_TEST_DSN").unwrap()),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "release integration pressure experiment"]
async fn child_pool_pressure() {
    for concurrency in [1, 2, 4, 8] {
        let started = std::time::Instant::now();
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..concurrency {
            workers.spawn(run_child_pool_scenario(true, None));
        }
        while let Some(result) = workers.join_next().await {
            result.unwrap();
        }
        println!(
            "CHILD_POOL_PRESSURE concurrency={concurrency} elapsed_ms={:.1}",
            started.elapsed().as_secs_f64() * 1000.
        );
    }
}

async fn run_child_pool_scenario(messaging: bool, postgres: Option<String>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let counter = calls.clone();
    let capture = observed.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let gate = release.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        capture.lock().unwrap().push(body);
        let gate = gate.clone();
        async move {
            if index == 0 { gate.notified().await; }
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
    if let Some(dsn) = postgres {
        config.storage.backend = "postgres".into();
        config.storage.postgres.dsn = dsn;
    }
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
    std::fs::write(
        dir.path().join("config.yaml"),
        serde_yaml::to_string(&config).unwrap(),
    )
    .unwrap();
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
            &format!("user_{}", uuid::Uuid::new_v4().simple()),
            None,
            "test-password",
            None,
            None,
            vec!["user".into()],
            "active",
            false,
        )
        .unwrap();
    let parent_id = format!("parent_{}", uuid::Uuid::new_v4().simple());
    let child_id = format!("child_{}", uuid::Uuid::new_v4().simple());
    let a2a = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let mut context = ToolContext {
        user_id: &user.user_id,
        session_id: &parent_id,
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
    for (id, parent) in [
        (parent_id.as_str(), None),
        (child_id.as_str(), Some(parent_id.as_str())),
    ] {
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
        .register(&parent_id, &user.user_id, "", "task", true, false);
    let accepted = subagent_control::execute(
        &context,
        &json!({"action":"send","session_id":child_id,"message":"first"}),
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
    if messaging {
        let guide = json!({"action":"send","session_id":child_id,"message":"guidance","message_id":"guide_1"});
        for _ in 0..2 {
            let receipt = subagent_control::execute(&context, &guide).await.unwrap();
            assert_eq!(receipt["data"]["delivery"], "queued_current_turn");
        }
        let parent_inbox = state
            .monitor
            .mailboxes
            .open(&user.user_id, &parent_id)
            .unwrap();
        context.session_id = &child_id;
        let receipt = subagent_control::execute(
            &context,
            &json!({"action":"report","message":"partial","message_id":"report_1"}),
        )
        .await
        .unwrap();
        assert_eq!(receipt["data"]["delivery"], "queued_current_turn");
        context.session_id = &parent_id;
        let interrupted_wait = subagent_control::execute(
            &context,
            &json!({"action":"wait","run_id":run_id,"wait_seconds":10}),
        )
        .await
        .unwrap();
        assert_eq!(
            interrupted_wait["data"]["completed_reason"],
            "message_received"
        );
        assert_eq!(interrupted_wait["data"]["all_finished"], false);
        assert_eq!(
            parent_inbox
                .take(false)
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>(),
            vec!["partial"]
        );
        drop(parent_inbox);
        context.session_id = &parent_id;
        assert!(subagent_control::execute(
            &context,
            &json!({"action":"report","message":"invalid"})
        )
        .await
        .is_err());
        release.notify_one();
        let result = subagent_control::execute(
            &context,
            &json!({"action":"wait","run_id":run_id,"wait_seconds":45,"poll_interval_seconds":5}),
        )
        .await
        .unwrap();
        assert_eq!(result["state"], "completed");
        assert!(result["data"]["elapsed_s"].as_f64().unwrap_or(45.) < 45.);
        assert!(state.workspace.flush_writes_async().await);
        {
            let captured = observed.lock().unwrap();
            assert_eq!(captured.len(), 2);
            assert_eq!(
                captured[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["content"].as_str().is_some_and(|s| s.contains("guide_1")))
                    .count(),
                1
            );
            let systems: Vec<_> = captured
                .iter()
                .map(|body| {
                    body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|m| m["role"] == "system")
                        .unwrap()
                })
                .collect();
            assert_eq!(systems[0], systems[1]);
        }
        let history = state
            .storage
            .load_model_context_entries(&user.user_id, &child_id, None)
            .unwrap();
        assert_eq!(
            history
                .iter()
                .filter(|m| m["content"].as_str().is_some_and(|s| s.contains("guide_1")))
                .count(),
            1
        );
        // Hold the parent busy while duplicate durable reports are admitted.
        // A delayed completion already consumed by wait must not cancel its parent.
        let stale_request = crate::services::subagents::build_parent_auto_wake_request(
            state.storage.as_ref(),
            &user.user_id,
            &parent_id,
            None,
            json!({"dispatch":{"run_id":run_id}}),
        )
        .unwrap();
        let stale_message = crate::services::runtime::thread::mailbox::AgentMessage {
            id: "stale_completion".into(),
            source: child_id.clone(),
            kind: "completion".into(),
            text: stale_request.question.clone(),
            cancellation: None,
        };
        let stale_id = state
            .kernel
            .thread_runtime
            .submit_agent_message(stale_request, &stale_message)
            .await
            .unwrap();
        context.session_id = &child_id;
        let report = json!({"action":"report","message":"next input","message_id":"report_2"});
        let queued = subagent_control::execute(&context, &report).await.unwrap();
        let duplicate = subagent_control::execute(&context, &report).await.unwrap();
        assert_eq!(queued["data"], duplicate["data"]);
        let queue_id = queued["data"]["queue_id"].as_str().unwrap();
        assert_eq!(
            state
                .storage
                .get_agent_task(queue_id)
                .unwrap()
                .unwrap()
                .status,
            "pending"
        );
        state.monitor.mark_finished(&parent_id);
        state.kernel.thread_runtime.wake().await;
        // PostgreSQL admission and checkpoint contention can exceed the normal
        // local smoke window; this is a correctness test, not a latency gate.
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let task = state.storage.get_agent_task(queue_id).unwrap().unwrap();
                if task.status == "success"
                    && state
                        .storage
                        .get_agent_task(&stale_id)
                        .unwrap()
                        .unwrap()
                        .status
                        == "cancelled"
                {
                    break;
                }
                assert!(
                    !matches!(task.status.as_str(), "failed" | "cancelled" | "dead"),
                    "{task:?}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            state
                .storage
                .get_agent_task(&stale_id)
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        assert!(!state.monitor.is_cancelled(&parent_id));
        let duplicate = subagent_control::execute(&context, &report).await.unwrap();
        assert_eq!(queued["data"], duplicate["data"]);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        state
            .monitor
            .register(&parent_id, &user.user_id, "", "held", true, false);
        let cancelled = subagent_control::execute(
            &context,
            &json!({"action":"report","message":"pending","message_id":"report_cancel"}),
        )
        .await
        .unwrap();
        let cancelled_id = cancelled["data"]["queue_id"].as_str().unwrap();
        state
            .kernel
            .thread_runtime
            .cancel_session_activity(&user.user_id, &parent_id, "test")
            .await
            .unwrap();
        assert_eq!(
            state
                .storage
                .get_agent_task(cancelled_id)
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        assert!(
            subagent_control::execute(&context, &json!({"action":"report","message":"late"}))
                .await
                .is_err()
        );
        server.abort();
        return;
    }
    state
        .kernel
        .thread_runtime
        .cancel_session_activity(&user.user_id, &parent_id, "test")
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
            .get_chat_session(&user.user_id, &child_id)
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
        .register(&parent_id, &user.user_id, "", "next", true, false);
    context.user_round = Some(2);
    for (action, message) in [("resume", "second"), ("send", "third")] {
        let result = subagent_control::execute(
            &context,
            &json!({"action":action,"session_id":child_id,"message":message,"timeout_seconds":15}),
        )
        .await
        .unwrap();
        assert_eq!(
            (
                result["state"].as_str(),
                result["data"]["session_id"].as_str()
            ),
            (Some("completed"), Some(child_id.as_str()))
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
