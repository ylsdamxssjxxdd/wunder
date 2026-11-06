use super::*;

#[cfg(windows)]
#[test]
fn decode_command_output_recovers_gbk_chinese() {
    let (encoded, _, _) = encoding_rs::GBK.encode("中文输出正常");
    assert_eq!(decode_command_output(&encoded), "中文输出正常");
    // Valid UTF-8 must stay untouched.
    assert_eq!(decode_command_output("中文输出正常".as_bytes()), "中文输出正常");
}

#[cfg(feature = "sqlite-storage")]
use crate::{
    lsp::LspManager,
    services::tools::command_sessions::CommandSessionBroker,
    skills::SkillRegistry,
    storage::{SqliteStorage, StorageBackend},
    workspace::WorkspaceManager,
};

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn local_command_timeout_preserves_side_effect_and_requires_verification() {
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path()
            .join("state.sqlite3")
            .to_string_lossy()
            .into_owned(),
    ));
    storage.ensure_initialized().unwrap();
    let workspace = Arc::new(WorkspaceManager::new(
        root.path().join("workspace").to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.server.mode = "cli".into();
    config.security.allow_commands = vec!["*".into()];
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "fixture-user",
        session_id: "fixture-thread",
        workspace_id: "fixture-user",
        agent_id: None,
        user_round: None,
        model_round: None,
        is_admin: true,
        storage,
        orchestrator: None,
        monitor: None,
        workspace: workspace.clone(),
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
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command \"Add-Content -LiteralPath probe.txt -Value once; Start-Sleep -Seconds 5\""
    } else {
        "printf 'once\\n' >> probe.txt; sleep 5"
    };
    let result = execute_command(
        &context,
        &json!({"content":command,"timeout_s":2,"run_in_background":false}),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(result["error_meta"]["code"], "TOOL_EXEC_TIMEOUT");
    assert_eq!(result["error_meta"]["retryable"], false);
    assert_eq!(result["data"]["execution_outcome"], "unknown");
    let content =
        std::fs::read_to_string(workspace.workspace_root("fixture-user").join("probe.txt"))
            .unwrap();
    assert_eq!(content.lines().collect::<Vec<_>>(), vec!["once"]);
    let remote = build_sandbox_command_result(json!({"timed_out":true,"exit_code":-1}));
    assert_eq!(remote["error_meta"]["retryable"], false);
    assert_eq!(remote["data"]["execution_outcome"], "unknown");
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn foreground_command_can_be_terminated_from_the_task_panel() {
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path()
            .join("state.sqlite3")
            .to_string_lossy()
            .into_owned(),
    ));
    storage.ensure_initialized().unwrap();
    let workspace = Arc::new(WorkspaceManager::new(
        root.path().join("workspace").to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.server.mode = "cli".into();
    config.security.allow_commands = vec!["*".into()];
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let broker = Arc::new(CommandSessionBroker::new());
    let context = ToolContext {
        user_id: "fixture-user",
        session_id: "fixture-thread",
        workspace_id: "fixture-user",
        agent_id: None,
        user_round: None,
        model_round: None,
        is_admin: true,
        storage,
        orchestrator: None,
        monitor: None,
        workspace: workspace.clone(),
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
        "powershell -NoProfile -Command \"Start-Sleep -Seconds 30\""
    } else {
        "sh -c 'sleep 30'"
    };
    let canceller = async {
        // 等到面板（running_tasks）里出现这条命令，再走与终止按钮相同的路径。
        // 会话登记早于取消令牌登记，窄窗口内终止会落空，重试直到生效。
        for _ in 0..500 {
            let tasks = broker.running_tasks("fixture-user", "fixture-thread");
            if let Some(task) = tasks.first() {
                if broker.terminate_task("fixture-user", "fixture-thread", &task.id) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("command session never appeared in running tasks");
    };
    let args = json!({"content": command, "timeout_s": 60, "run_in_background": false});
    let runner = execute_command(&context, &args);
    let (result, _) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(runner, canceller)
    })
    .await
    .expect("cancellation must not hang");
    let result = result.unwrap();
    assert_eq!(result["ok"], false, "{result}");
    let stderr = result["data"]["stderr"].as_str().unwrap_or("");
    assert!(stderr.contains("command cancelled"), "{result}");
    // 会话已收尾，面板不再列出这条任务。
    assert!(
        broker
            .running_tasks("fixture-user", "fixture-thread")
            .is_empty()
    );
}

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

/// shell 已退出但孙进程仍持有 stdout 管道写端：历史实现在 join EOF 时
/// 无限等待（孙进程活多久挂多久），是 Windows 下 pwsh「挂死」并冻结整条
/// 智能体线程的直接根源。树终止 + EOF 宽限后，调用必须在有限时间内返回，
/// 且已产出的输出不能丢。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(feature = "sqlite-storage")]
async fn pipe_holding_grandchild_cannot_stall_the_command_result() {
    if cfg!(windows) {
        return; // 命令形态按 unix 书写；Windows 由同构的 job 终止路径覆盖。
    }
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path()
            .join("state.sqlite3")
            .to_string_lossy()
            .into_owned(),
    ));
    storage.ensure_initialized().unwrap();
    let workspace = Arc::new(WorkspaceManager::new(
        root.path().join("workspace").to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.server.mode = "cli".into();
    config.security.allow_commands = vec!["*".into()];
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "fixture-user",
        session_id: "fixture-thread",
        workspace_id: "fixture-user",
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
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };
    let started = std::time::Instant::now();
    let result = execute_command(&context, &json!({"content": "echo ready; sleep 300 &"}))
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(result["ok"], true, "{result}");
    let stdout = result["data"]["results"][0]["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("ready"), "{result}");
    assert!(
        elapsed < Duration::from_secs(30),
        "pipe tail must not stall the call, took {elapsed:?}"
    );
}

/// 超时终止必须收掉整棵树：杀掉 shell 后孙进程不能再持有管道把收尾拖住，
/// 超时结果要按时返回。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(feature = "sqlite-storage")]
async fn timeout_kills_the_whole_tree_and_returns_promptly() {
    if cfg!(windows) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path()
            .join("state.sqlite3")
            .to_string_lossy()
            .into_owned(),
    ));
    storage.ensure_initialized().unwrap();
    let workspace = Arc::new(WorkspaceManager::new(
        root.path().join("workspace").to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.server.mode = "cli".into();
    config.security.allow_commands = vec!["*".into()];
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "fixture-user",
        session_id: "fixture-thread",
        workspace_id: "fixture-user",
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
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };
    let started = std::time::Instant::now();
    let result = execute_command(
        &context,
        &json!({"content": "echo started; sleep 300", "timeout_s": 1}),
    )
    .await
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_meta"]["code"], "TOOL_EXEC_TIMEOUT");
    assert_eq!(
        result["data"]["diagnostics"][0]["output"],
        json!("timeout after 1s"),
        "{result}"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "timeout must return promptly after tree kill, took {elapsed:?}"
    );
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
