use super::*;
#[cfg(feature = "sqlite-storage")]
use crate::{
    lsp::LspManager,
    services::tools::command_sessions::CommandSessionBroker,
    skills::SkillRegistry,
    storage::{SqliteStorage, StorageBackend},
    workspace::WorkspaceManager,
};

#[test]
fn model_sandbox_poll_contains_each_output_once() {
    let result = compact_sandbox_poll_result(json!({
        "status": "running", "seq": 3, "stdout": "old\nnew\n", "stderr": "warning\n",
        "deltas": [
            {"seq": 2, "stream": "stdout", "delta": "new\n"},
            {"seq": 3, "stream": "stderr", "delta": "warning\n"}
        ]
    }));
    assert_eq!(result["stdout"], "new\n");
    assert_eq!(result["stderr"], "warning\n");
    assert!(result.get("deltas").is_none());
    assert_eq!(
        build_command_session_result(json!({
            "status": "exited", "exit_code": 2, "stderr": "failed"
        }))["ok"],
        false
    );
}

/// Exercise real sandbox pipes + HTTP + runtime observer + model exit wait.
/// The model does not poll to drive output delivery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(feature = "sqlite-storage")]
async fn sandbox_background_output_streams_independently_and_polls_incrementally() {
    struct RestoreEndpoint(Option<std::ffi::OsString>);
    impl Drop for RestoreEndpoint {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("WUNDER_SANDBOX_ENDPOINT", value),
                None => std::env::remove_var("WUNDER_SANDBOX_ENDPOINT"),
            }
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, crate::sandbox::server::build_router())
            .await
            .unwrap();
    });
    let _restore = RestoreEndpoint(std::env::var_os("WUNDER_SANDBOX_ENDPOINT"));
    std::env::set_var("WUNDER_SANDBOX_ENDPOINT", format!("http://{address}"));
    let dir = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        dir.path().join("test.db").to_string_lossy().to_string(),
    ));
    let workspace = Arc::new(WorkspaceManager::new(
        dir.path().to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let broker = Arc::new(CommandSessionBroker::new());
    let context = ToolContext {
        user_id: "user",
        session_id: "thread",
        workspace_id: "workspace",
        agent_id: None,
        user_round: None,
        model_round: None,
        is_admin: true,
        storage,
        orchestrator: None,
        monitor: None,
        workspace,
        lsp_manager,
        config: &config,
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
        command_sessions: Some(broker.clone()),
        event_emitter: None,
        http: &http,
    };
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command \"Write-Output 'first'; Start-Sleep -Milliseconds 350; Write-Output 'second'\""
    } else {
        "printf 'first\\n'; sleep 0.35; printf 'second\\n'"
    };
    let launch: Value = http
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user", "session_id": "thread", "workspace_root": "/",
            "container_root": "/", "allow_commands": ["*"],
            "args": {"content": command, "yield_time_ms": 50, "timeout_s": 10}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(launch["ok"], true, "{launch}");
    let id = launch["data"]["command_session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let tracker = CommandSessionTracker::start_with_id(
        &context,
        Some(id.clone()),
        "stream test",
        "",
        0,
        None,
        CommandSessionLaunchMode::Shell,
        false,
        false,
    )
    .unwrap();
    let handle = Arc::new(CommandProcessHandle::new(None));
    assert!(broker.register_process(&id, handle.clone()));
    let mut events = broker.subscribe();
    let observer = tokio::spawn(observe_sandbox_command(
        config.clone(),
        "user".into(),
        "thread".into(),
        tracker,
        broker.clone(),
        handle.cancellation_token(),
    ));
    let first = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.event_type, "command_session_delta");
    assert!(first.data["delta"].as_str().unwrap().contains("first"));
    let result = command_session_control(&context, &json!({"command_session_id": id}))
        .await
        .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["status"], "exited");
    let stdout = result["data"]["stdout"].as_str().unwrap();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["first", "second"]);
    let next = command_session_control(
        &context,
        &json!({"command_session_id": id, "yield_time_ms": 0}),
    )
    .await
    .unwrap();
    assert_eq!(next["data"]["stdout"], "");
    assert!(broker.snapshot(&id).unwrap().stdout_tail.contains("first"));
    let foreground = await_sandbox_command_exit(&context, &id, 10.0, None)
        .await
        .unwrap();
    assert_eq!(
        foreground["stdout"]
            .as_str()
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(foreground.get("deltas").is_none());
    assert_eq!(foreground["output_truncated"], false);
    tokio::time::timeout(Duration::from_secs(2), observer)
        .await
        .unwrap()
        .unwrap();
    server.abort();
}

#[test]
fn compact_command_result_for_model_passes_stderr_warning_through() {
    let with_warning = compact_command_result_for_model(&json!({
        "command": "echo ok | sort",
        "command_index": 0,
        "returncode": 0,
        "stdout": "ok\n",
        "stderr": "sort: open failed: missing\n",
        "warning": "退出码为 0 但 stderr 非空"
    }));
    assert_eq!(with_warning["warning"], json!("退出码为 0 但 stderr 非空"));

    let without_warning = compact_command_result_for_model(&json!({
        "command": "echo ok",
        "command_index": 0,
        "returncode": 0,
        "stdout": "ok\n",
        "stderr": ""
    }));
    assert!(without_warning.get("warning").is_none());
}
