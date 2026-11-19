use super::*;

#[test]
fn closed_heredoc_passes_the_guard() {
    let script = "cat > /tmp/patch.py <<'EOF'\nprint(1)\nEOF\npython3 /tmp/patch.py";
    assert_eq!(unterminated_heredoc(script), None);
    // `<<-` 允许用制表符缩进的定界符收尾。
    let tabbed = "cat <<-EOF\n\techo hi\n\tEOF\n";
    assert_eq!(unterminated_heredoc(tabbed), None);
}

#[test]
fn truncated_heredoc_is_reported_with_its_delimiter() {
    let truncated = "cat > /tmp/patch.py <<'EOF'\nprint(1)\nprint(2)";
    assert_eq!(unterminated_heredoc(truncated), Some("EOF".to_string()));
    let quoted = "python3 <<\"PY\"\nprint(1)\n";
    assert_eq!(unterminated_heredoc(quoted), Some("PY".to_string()));
}

#[test]
fn shift_operators_and_herestrings_are_not_heredocs() {
    // 移位运算出现在脚本正文里，不该被当成 heredoc 开头。
    let script = "python3 -c \"x = a << b\"\necho done";
    assert_eq!(unterminated_heredoc(script), None);
    let herestring = "grep foo <<< \"bar baz\"";
    assert_eq!(unterminated_heredoc(herestring), None);
    // 正文里的 `<<` 行属于已闭合 heredoc 的内容，不应再开新 heredoc。
    let nested = "cat > /tmp/a.txt <<'EOF'\nx << y\nEOF\n";
    assert_eq!(unterminated_heredoc(nested), None);
}

#[test]
fn background_command_plan_keeps_multiline_script_as_one_job() {
    let script = "printf 'a\\n'\nprintf 'b\\n'";
    assert_eq!(
        build_command_plan(script, false, true),
        vec![script.to_string()]
    );
    assert_eq!(
        build_command_plan(script, true, false),
        vec![script.to_string()]
    );
    assert_eq!(
        build_command_plan(script, false, false),
        vec!["printf 'a\\n'".to_string(), "printf 'b\\n'".to_string()]
    );
}

#[cfg(windows)]
#[test]
fn decode_command_output_recovers_gbk_chinese() {
    let (encoded, _, _) = encoding_rs::GBK.encode("中文输出正常");
    assert_eq!(decode_command_output(&encoded), "中文输出正常");
    // Valid UTF-8 must stay untouched.
    assert_eq!(
        decode_command_output("中文输出正常".as_bytes()),
        "中文输出正常"
    );
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
    let result = execute_command(&context, &json!({"content":command,"timeout_s":2}))
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
    // 超时前没有任何输出 = 真的无从判断，保持 unknown。
    let blank = build_sandbox_command_result(json!({
        "timed_out": true,
        "exit_code": -1,
        "stdout": "  \n"
    }));
    assert_eq!(blank["data"]["execution_outcome"], "unknown");
    // 超时前只有执行层补的 `timeout after 2s` 标记，不算命令产出。
    let note_only = build_sandbox_command_result(json!({
        "timed_out": true,
        "exit_code": -1,
        "stderr": "timeout after 2s"
    }));
    assert_eq!(note_only["data"]["execution_outcome"], "unknown");
    // 真输出 + 标记同时存在时，仍要认出“命令确实跑过”。
    let mixed = build_sandbox_command_result(json!({
        "timed_out": true,
        "exit_code": -1,
        "stdout": "step 1 done\n",
        "stderr": "timeout after 2s"
    }));
    assert_eq!(mixed["data"]["execution_outcome"], "likely_applied");
    // 超时前已经有输出 = 命令确实跑过，副作用可能已落盘，不能再让模型当没执行。
    let progressed = build_sandbox_command_result(json!({
        "timed_out": true,
        "exit_code": -1,
        "stdout": "step 1 done\n"
    }));
    assert_eq!(progressed["data"]["execution_outcome"], "likely_applied");
    assert!(progressed["data"]["next_step_hint"]
        .as_str()
        .unwrap_or_default()
        .contains("副作用"));
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
    let args = json!({"content": command, "timeout_s": 60});
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
    assert!(broker
        .running_tasks("fixture-user", "fixture-thread")
        .is_empty());
}

/// 后台会话的 stdin 是真管道：喂进去的内容命令能收到；会话退出后再喂，
/// 不能变成硬错误把最终输出一起丢掉。
#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn background_session_takes_stdin_and_still_returns_final_output_after_exit() {
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
        "powershell -NoProfile -Command \"$line = [Console]::In.ReadLine(); Write-Output ('GOT:' + $line)\""
    } else {
        "read -r line; printf 'GOT:%s\\n' \"$line\""
    };
    let started = execute_command(&context, &json!({"cmd": command, "yield_time_ms": 100}))
        .await
        .unwrap();
    assert_eq!(started["ok"], true, "{started}");
    let id = started["data"]["command_session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("command session id missing: {started}"))
        .to_string();
    // 写入后一有输出就返回，不为了等待退出拖住这次调用。
    let answered = command_session_control(
        &context,
        &json!({"session_id": id, "chars": "hello\n", "yield_time_ms": 5000}),
    )
    .await
    .unwrap();
    assert_eq!(answered["ok"], true, "{answered}");
    assert!(
        answered["data"]["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("GOT:hello"),
        "{answered}"
    );
    // 空 chars 复用同一会话；没有新增输出时等待进程退出。
    let finished = command_session_control(
        &context,
        &json!({"session_id": id, "chars": "", "yield_time_ms": 5000}),
    )
    .await
    .unwrap();
    assert_eq!(finished["data"]["status"], "exited", "{finished}");
    assert_eq!(finished["data"]["exit_code"], 0, "{finished}");
    // 已经退出再喂：不算失败，照样返回最终输出，并说明输入没有送进去。
    let after_exit = command_session_control(
        &context,
        &json!({"command_session_id": id, "input": "again\n", "yield_time_ms": 0}),
    )
    .await
    .unwrap();
    assert_eq!(after_exit["ok"], true, "{after_exit}");
    assert_eq!(after_exit["data"]["status"], "exited", "{after_exit}");
    assert!(
        after_exit["data"]["stdin_note"].as_str().is_some(),
        "{after_exit}"
    );
    assert_eq!(after_exit["data"]["exit_code"], 0, "{after_exit}");
}

/// 模型主动发 Ctrl-C 是正常操作，不是工具故障：要返回成功并说明已中断。
#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn interrupt_input_cancels_the_session_without_reporting_failure() {
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
    let started = execute_command(
        &context,
        &json!({"content": "echo alive; sleep 30", "timeout_s": 60, "yield_time_ms": 100}),
    )
    .await
    .unwrap();
    assert_eq!(started["ok"], true, "{started}");
    let id = started["data"]["command_session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("command session id missing: {started}"))
        .to_string();
    let interrupted = command_session_control(
        &context,
        &json!({"command_session_id": id, "input": "", "yield_time_ms": 3000}),
    )
    .await
    .unwrap();
    assert_eq!(interrupted["ok"], true, "{interrupted}");
    assert_eq!(interrupted["data"]["interrupted"], true, "{interrupted}");
    assert_eq!(interrupted["data"]["status"], "exited", "{interrupted}");
    let polled = command_session_control(
        &context,
        &json!({"command_session_id": id, "chars": "", "yield_time_ms": 0}),
    )
    .await
    .unwrap();
    assert_eq!(polled["ok"], true, "{polled}");
    assert_eq!(polled["data"]["status"], "exited", "{polled}");
    assert_eq!(polled["data"]["interrupted"], true, "{polled}");
    assert!(polled["data"]["error"].is_null(), "{polled}");
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
    // 非零退出是命令语义：会话正常完成，退出码与提示随数据返回。
    let exited = build_command_session_result(json!({
        "status": "exited", "exit_code": 2, "stderr": "failed"
    }));
    assert_eq!(exited["ok"], true);
    assert_eq!(exited["data"]["exit_code"], 2);
    assert!(exited["data"]["exit_note"].as_str().is_some());
}

#[test]
fn dropped_sandbox_poll_preserves_bounded_snapshot_instead_of_rebuilding_a_partial_head() {
    let result = compact_sandbox_poll_result(json!({
        "status": "exited", "dropped": true,
        "stdout": "PREFIX300|START_MARK_ASCII|\n...(truncated)...\n|TAIL_MARK\n",
        "stderr": "",
        "deltas": [{"seq": 400, "stream": "stdout", "delta": "|TAIL_MARK\n"}]
    }));
    assert!(result["stdout"]
        .as_str()
        .unwrap()
        .starts_with("PREFIX300|START_MARK_ASCII|"));
    assert_eq!(result["output_truncated"], true);
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
        None,
        handle.cancellation_token(),
        0,
    ));
    let first = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.event_type, "command_session_delta");
    assert!(first.data["delta"].as_str().unwrap().contains("first"));
    // 默认是「读一眼」：先确认能拿到增量，再显式阻塞到退出收齐剩余输出。
    let quick = command_session_control(&context, &json!({"command_session_id": id}))
        .await
        .unwrap();
    assert_eq!(quick["ok"], true, "{quick}");
    let result = command_session_control(
        &context,
        &json!({"command_session_id": id, "wait_for": "exit", "yield_time_ms": 10000}),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["status"], "exited");
    let stdout = result["data"]["stdout"].as_str().unwrap();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["second"]);
    let next = command_session_control(
        &context,
        &json!({"command_session_id": id, "yield_time_ms": 0}),
    )
    .await
    .unwrap();
    assert_eq!(next["data"]["stdout"], "");
    assert!(broker.snapshot(&id).unwrap().stdout_tail.contains("first"));

    let short_command = if cfg!(windows) {
        "ping 127.0.0.1 -n 30 > NUL"
    } else {
        "printf 'fixture\\n'; sleep 5"
    };
    let short_launch = http
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user", "session_id": "thread", "workspace_root": "/",
            "container_root": "/", "allow_commands": ["*"],
            "args": {"content": short_command, "yield_time_ms": 50}
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let short_id = short_launch["data"]["command_session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let began = std::time::Instant::now();
    let (short, _) = await_sandbox_command_exit(&context, &short_id, 0.1, None)
        .await
        .unwrap();
    assert_eq!(short["status"], "running", "{short}");
    assert!(began.elapsed() < Duration::from_secs(2), "{short}");
    let short_tracker = CommandSessionTracker::start_with_id(
        &context,
        Some(short_id.clone()),
        short_command,
        "",
        0,
        Some("sandbox".into()),
        CommandSessionLaunchMode::Shell,
        false,
        false,
    )
    .unwrap();
    let short_handle = Arc::new(CommandProcessHandle::new(None));
    assert!(broker.register_process(&short_id, short_handle.clone()));
    let short_observer = tokio::spawn(observe_sandbox_command(
        config.clone(),
        "user".into(),
        "thread".into(),
        short_tracker,
        broker.clone(),
        None,
        short_handle.cancellation_token(),
        0,
    ));
    let interrupted = command_session_control(
        &context,
        &json!({"command_session_id": short_id, "chars": "\u{3}", "yield_time_ms": 1000}),
    )
    .await
    .unwrap();
    assert_eq!(interrupted["ok"], true, "{interrupted}");
    assert_eq!(interrupted["data"]["status"], "exited", "{interrupted}");
    assert_eq!(interrupted["data"]["interrupted"], true, "{interrupted}");
    assert!(interrupted["data"]
        .as_object()
        .unwrap()
        .contains_key("exit_code"));
    assert!(interrupted["data"]["exit_code"].is_null(), "{interrupted}");
    tokio::time::timeout(Duration::from_secs(2), short_observer)
        .await
        .unwrap()
        .unwrap();

    let eot_launch = http
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user", "session_id": "thread", "workspace_root": "/",
            "container_root": "/", "allow_commands": ["*"],
            "args": {
                "content": "env -u PYTHONHOME -u PYTHONPATH python3 -u -c \"import sys; print('ready', flush=True); data=sys.stdin.read(); print('eof='+str(len(data)), flush=True)\"",
                "yield_time_ms": 50
            }
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let eot_id = eot_launch["data"]["command_session_id"].as_str().unwrap();
    let eot = command_session_control(
        &context,
        &json!({"command_session_id": eot_id, "chars": "\\x04", "yield_time_ms": 1000}),
    )
    .await
    .unwrap();
    assert_eq!(eot["ok"], true, "{eot}");
    assert_eq!(eot["data"]["status"], "exited", "{eot}");
    assert_eq!(eot["data"]["exit_code"], 0, "{eot}");
    assert!(
        eot["data"]["stdout"].as_str().unwrap().contains("eof=0"),
        "{eot}"
    );

    let large_launch = http
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user", "session_id": "thread", "workspace_root": "/",
            "container_root": "/", "allow_commands": ["*"],
            "args": {
                "content": "env -u PYTHONHOME -u PYTHONPATH python3 -c \"print('PREFIX300|START_MARK_ASCII|' + 'x'*300000 + '|TAIL_MARK')\"",
                "yield_time_ms": 50
            }
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let large_id = large_launch["data"]["command_session_id"].as_str().unwrap();
    let (large, _) = await_sandbox_command_exit(&context, large_id, 5.0, None)
        .await
        .unwrap();
    let large_stdout = large["stdout"].as_str().unwrap();
    assert!(
        large_stdout.starts_with("PREFIX300|START_MARK_ASCII|"),
        "{:.80}",
        large_stdout
    );
    assert!(
        large_stdout.ends_with("|TAIL_MARK\n"),
        "{:.80}",
        large_stdout
    );
    assert!(large_stdout.contains("truncated command output"));
    assert_eq!(large["output_truncated"], true);

    let (foreground, _after_seq) = await_sandbox_command_exit(&context, &id, 10.0, None)
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
    let stdout = result["data"]["results"][0]["stdout"]
        .as_str()
        .unwrap_or("");
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

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn initial_wait_yields_partial_output_and_keeps_the_same_process_running() {
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
    let result = execute_command(
        &context,
        &json!({
            "content": "printf 'started\\n'; sleep 2; printf 'finished\\n' > escalated.txt",
            "yield_time_ms": 100
        }),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["state"], "running", "{result}");
    assert_eq!(result["data"]["yield_time_ms"], 100, "{result}");
    assert!(result["data"]["command_session_id"].as_str().is_some());
    assert!(
        result["data"]["results"][0]["returncode"].is_null(),
        "{result}"
    );
    assert_eq!(
        result["data"]["results"][0]["stdout"], "started\n",
        "{result}"
    );
    // 进程没有被杀，继续在后台跑完并落盘。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let output = workspace
        .workspace_root("fixture-user")
        .join("escalated.txt");
    while std::time::Instant::now() < deadline {
        if output.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(std::fs::read_to_string(output).unwrap().trim(), "finished");
    while !broker
        .running_tasks("fixture-user", "fixture-thread")
        .is_empty()
        && std::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn foreground_quick_command_with_broker_returns_without_waiting_for_timeout() {
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
        command_sessions: Some(broker),
        event_emitter: None,
        http: &http,
    };
    let started = std::time::Instant::now();
    let result = execute_command(
        &context,
        &json!({"content": "printf 'quick\\n'", "timeout_s": 60}),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["state"], "completed", "{result}");
    assert_eq!(
        result["data"]["results"][0]["stdout"],
        json!("quick\n"),
        "{result}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "quick command waited for timeout: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn sigpipe_exit_is_reported_as_truncated_output_not_failure() {
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
    let result = execute_command(
        &context,
        &json!({"content": "sh -c 'kill -s PIPE $$'", "timeout_s": 30}),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        result["data"]["results"][0]["output_truncated_by_pipe"],
        json!(true),
        "{result}"
    );
    assert!(result["data"]["results"][0]["warning"]
        .as_str()
        .unwrap_or_default()
        .contains("SIGPIPE"));
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

#[test]
fn compact_command_result_for_model_keeps_background_seq_for_after_seq_resume() {
    // 后台首包的 seq 是模型续读游标；不带 seq 的普通结果不应凭空多出字段。
    let running_entry = compact_command_result_for_model(&json!({
        "command": "make -j8",
        "command_index": 0,
        "command_session_id": "cmd_demo",
        "returncode": null,
        "stdout": "compiling\n",
        "stderr": "",
        "running": true,
        "seq": 7
    }));
    assert_eq!(running_entry["seq"], json!(7));
    assert_eq!(running_entry["command_session_id"], json!("cmd_demo"));

    let finished_entry = compact_command_result_for_model(&json!({
        "command": "echo ok",
        "command_index": 0,
        "returncode": 0,
        "stdout": "ok\n",
        "stderr": ""
    }));
    assert!(finished_entry.get("seq").is_none());
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn duplicate_command_reuses_the_running_session() {
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
    let payload = json!({"content": "printf 'go\\n'; sleep 5", "yield_time_ms": 100});
    let first = execute_command(&context, &payload).await.unwrap();
    assert_eq!(first["state"], "running", "{first}");
    let id = first["data"]["command_session_id"]
        .as_str()
        .expect("running command must expose a session id")
        .to_string();
    // 同一条命令再发一次：复用既有会话，不启动第二份。
    let second = execute_command(&context, &payload).await.unwrap();
    assert_eq!(second["state"], "running", "{second}");
    assert_eq!(second["data"]["reused"], true, "{second}");
    assert_eq!(second["data"]["command_session_id"], json!(id), "{second}");
    assert_eq!(
        broker.running_tasks("fixture-user", "fixture-thread").len(),
        1,
        "duplicate launch must not create a second process"
    );
    let _ = command_session_control(
        &context,
        &json!({"command_session_id": id, "chars": "\x03"}),
    )
    .await;
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn background_command_without_wake_chain_is_rejected() {
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
        command_sessions: Some(broker),
        event_emitter: None,
        http: &http,
    };
    // 没有线程唤醒链路就不能假装后台：明确拒绝，让模型回退前台执行。
    let result = execute_command(
        &context,
        &json!({"content": "echo hi", "run_in_background": true}),
    )
    .await
    .unwrap();
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(
        result["error_meta"]["code"], "TOOL_EXEC_BACKGROUND_UNAVAILABLE",
        "{result}"
    );
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn background_command_returns_running_session_before_exit() {
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
    let (stdout_policy, stderr_policy) = derive_capture_policies(None);
    let user_root = workspace.ensure_user_root("fixture-user").unwrap();
    let started = std::time::Instant::now();
    let run = run_command_streaming(
        &context,
        "printf 'bg\\n'; sleep 3; printf 'done\\n'",
        &user_root,
        Some(Duration::from_secs(30)),
        "execute_command",
        stdout_policy,
        stderr_policy,
        0,
        None,
        true,
        None,
        false,
    )
    .await
    .unwrap();
    // 后台模式不等待进程结束：立即交回会话 id。
    assert!(run.running);
    assert!(started.elapsed() < Duration::from_secs(1));
    let id = run.command_session_id.expect("session id");
    let data = broker
        .poll(
            "fixture-user",
            "fixture-thread",
            &id,
            None,
            Duration::from_secs(10),
            true,
        )
        .await
        .unwrap();
    assert_eq!(data["status"], "exited", "{data}");
    assert_eq!(data["exit_code"], 0, "{data}");
    let snapshot = broker.snapshot(&id).expect("snapshot");
    assert!(snapshot.stdout_tail.contains("done"), "{snapshot:?}");
    // 模型自己等到终态后，完成通知应被抑制。
    assert!(broker.is_model_observed(&id));
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn command_session_without_id_lists_thread_tasks() {
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
    let started = execute_command(
        &context,
        &json!({"content": "printf 'starting\\n'; sleep 5", "yield_time_ms": 100}),
    )
    .await
    .unwrap();
    let id = started["data"]["command_session_id"]
        .as_str()
        .expect("running command must expose a session id")
        .to_string();
    // 不传 id：只读列出本线程的命令任务，模型据此判断「到底跑没跑」。
    let listed = command_session_control(&context, &json!({})).await.unwrap();
    assert_eq!(listed["ok"], true, "{listed}");
    let sessions = listed["data"]["sessions"]
        .as_array()
        .expect("listed sessions");
    assert_eq!(sessions.len(), 1, "{listed}");
    assert_eq!(sessions[0]["command_session_id"], json!(id), "{listed}");
    assert_eq!(sessions[0]["running"], json!(true), "{listed}");
    assert_eq!(listed["data"]["running"], json!(1), "{listed}");
    assert!(sessions[0]["elapsed_ms"].as_i64().unwrap_or(-1) >= 0);
    // 列表是只读的：会话仍在跑。
    assert_eq!(
        broker.running_tasks("fixture-user", "fixture-thread").len(),
        1
    );
    let _ = command_session_control(
        &context,
        &json!({"command_session_id": id, "chars": "\x03"}),
    )
    .await;
}

#[tokio::test]
#[cfg(feature = "sqlite-storage")]
async fn aborted_tool_call_leaves_the_process_running_and_pollable() {
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
    // 看门狗打断：工具调用只活 50ms 就被丢弃，命令本身还在跑。
    let aborted = tokio::time::timeout(
        Duration::from_millis(50),
        execute_command(
            &context,
            &json!({"content": "printf 'go\\n'; sleep 2; printf 'done\\n' > aborted.txt"}),
        ),
    )
    .await;
    assert!(aborted.is_err(), "tool call should still be waiting");
    // 进程没有陪葬：会话仍在跑，并且能被列出来（旧实现会连树带进程一起杀掉）。
    let listed = command_session_control(&context, &json!({})).await.unwrap();
    let sessions = listed["data"]["sessions"]
        .as_array()
        .expect("listed sessions");
    assert_eq!(sessions.len(), 1, "{listed}");
    let id = sessions[0]["command_session_id"]
        .as_str()
        .expect("session id")
        .to_string();
    assert_eq!(sessions[0]["running"], json!(true), "{listed}");
    // 命令自然跑完后产物落盘，说明它确实继续执行到了最后。
    let output = workspace.workspace_root("fixture-user").join("aborted.txt");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if output.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(std::fs::read_to_string(output).unwrap().trim(), "done");
    // 会话最终收尾为 exited，不会留下永久 running 的僵尸条目。
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !broker
        .running_tasks("fixture-user", "fixture-thread")
        .is_empty()
        && std::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let snapshot = broker
        .snapshot_for_scope("fixture-user", "fixture-thread", &id)
        .expect("session snapshot");
    assert_ne!(snapshot.status, CommandSessionStatus::Running);
}
