use super::*;
use crate::orchestrator::execute_support::PlannedToolCall;
use crate::state::{AppState, AppStateInitOptions};

#[cfg(windows)]
const LONG_RUNNING_COMMAND: &str = "ping -n 10 127.0.0.1";
#[cfg(not(windows))]
const LONG_RUNNING_COMMAND: &str = "sleep 8 && echo done";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_metrics_preserve_parallel_success_failure_and_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root.path().join("state.db").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    // Run commands through the local blocking path instead of the sandbox HTTP client.
    config.server.mode = "desktop".to_string();
    config.security.allow_commands = vec!["*".to_string()];
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state = AppState::new_with_options(
        store,
        config.clone(),
        AppStateInitOptions::cli_default().with_start_thread_runtime(false),
    )
    .unwrap();
    let orchestrator = &state.kernel.orchestrator;
    let skills = SkillRegistry::default();
    let context = ToolContext {
        user_id: "user_1",
        session_id: "session_1",
        workspace_id: "user_1",
        agent_id: None,
        user_round: Some(1),
        model_round: Some(1),
        is_admin: true,
        storage: state.storage.clone(),
        orchestrator: None,
        monitor: Some(state.monitor.clone()),
        workspace: orchestrator.workspace.clone(),
        lsp_manager: orchestrator.lsp_manager.clone(),
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
        http: &orchestrator.http,
    };
    let name = crate::tools::resolve_tool_name("execute_command");
    let allowed = HashSet::from([name.clone()]);
    let planned = |id: &str, content: &str| PlannedToolCall {
        call: tool_calls::ToolCall {
            id: Some(id.into()),
            name: name.clone(),
            arguments: json!({"content": content}),
            function_name: Some("execute_command".into()),
        },
        name: name.clone(),
        function_name: "execute_command".into(),
    };
    state
        .monitor
        .register("session_1", "user_1", "", "input", true);
    let (tx, mut rx) = mpsc::channel(64);
    let emitter = EventEmitter::new(
        "session_1".into(),
        "user_1".into(),
        Some(tx),
        None,
        state.monitor.clone(),
        true,
        None,
    );
    let outcomes = orchestrator
        .execute_tool_calls_parallel(
            vec![planned("call_1", "echo ok"), planned("call_2", "exit 3")],
            &context,
            &allowed,
            "session_1",
            "turn_1",
            &emitter,
            None,
            RoundInfo::new(1, 1),
        )
        .await
        .unwrap();
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| (
                outcome.result.ok,
                outcome.result.meta.as_ref().unwrap()["duration_ms"]
                    .as_u64()
                    .is_some()
            ))
            .collect::<Vec<_>>(),
        vec![(true, true), (false, true)]
    );
    let work = orchestrator.execute_tool_calls_parallel(
        vec![
            planned("call_3", "echo ok"),
            planned("call_4", LONG_RUNNING_COMMAND),
        ],
        &context,
        &allowed,
        "session_1",
        "turn_1",
        &emitter,
        None,
        RoundInfo::new(1, 2),
    );
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        state.monitor.cancel("session_1");
    };
    let (result, _) = tokio::join!(work, cancel);
    assert!(result.is_err());
    let mut metrics = Vec::new();
    while let Ok(StreamSignal::Event(event)) = rx.try_recv() {
        if event.event == "tool_result" {
            let data = &event.data["data"];
            metrics.push((
                data["tool_call_id"].as_str().unwrap().to_string(),
                data["ok"].as_bool().unwrap(),
                data["meta"]["duration_ms"].as_u64().is_some(),
            ));
        }
    }
    metrics.sort();
    assert_eq!(
        metrics,
        vec![
            ("call_3".into(), true, true),
            ("call_4".into(), false, true)
        ]
    );
}
