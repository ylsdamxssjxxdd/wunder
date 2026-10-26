use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::time::{Duration, Instant};
use wunder_desktop::{args::DesktopArgs, native::NativeChatCommand, NativeDesktop};

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

fn isolated() -> anyhow::Result<(MutexGuard<'static, ()>, tempfile::TempDir, NativeDesktop)> {
    isolated_with_endpoint("http://127.0.0.1:9/v1")
}

fn isolated_with_endpoint(
    endpoint: &str,
) -> anyhow::Result<(MutexGuard<'static, ()>, tempfile::TempDir, NativeDesktop)> {
    let guard = RUNTIME_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(
        config.join("wunder.yaml"),
        serde_json::to_vec(&serde_json::json!({
            "llm": {"default": "fixture-model", "models": {
                "fixture-model": {"enable": true, "provider": "openai_compatible",
                    "model": "fixture-model", "base_url": endpoint, "api_key": "fixture-key",
                    "timeout_s": 30, "stream": true, "max_context": 16384, "max_output": 64}
            }},
            "skills": {"enabled": []}
        }))?,
    )?;
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "workspace_root": "", "desktop_token": "", "updated_at": 0,
            "lan_mesh": {"enabled": false}
        }))?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    std::env::set_var("WUNDER_HOME", directory.path().join("runtime"));
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    let desktop = NativeDesktop::start_with_args(args)?;
    Ok((guard, directory, desktop))
}

#[test]
fn goal_command_is_a_durable_completed_user_round() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
    let session = desktop.create_session(None)?;
    let command = NativeChatCommand::parse("/goal Fixture objective").unwrap();
    desktop.execute_chat_command(&session.id, command)?;
    let page = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        10,
    )?;
    assert_eq!(
        page.chat.len(),
        1,
        "a goal command must survive history reload"
    );
    assert_eq!(page.chat[0].user.content, "/goal Fixture objective");
    assert_eq!(page.chat[0].stop_reason.as_deref(), Some("goal_command"));
    let turns = desktop.load_chat_turns(&session.id)?;
    assert_eq!(
        turns.len(),
        1,
        "the full native projection must retain the durable goal command"
    );
    assert_eq!(turns[0].user.text, page.chat[0].user.content);
    desktop.cancel_chat(&session.id)?;
    Ok(())
}

#[test]
fn goal_pause_cancels_an_inflight_model_request() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let address = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    std::future::pending::<axum::Json<serde_json::Value>>().await
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Ok::<_, anyhow::Error>(address)
    })?;
    let (_guard, _directory, desktop) = isolated_with_endpoint(&format!("http://{address}/v1"))?;
    let session = desktop.create_session(None)?;
    desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::GoalSet("Fixture objective".into()),
    )?;
    desktop
        .state()
        .kernel
        .orchestrator
        .goal_handle()
        .driver()
        .notify_goal_changed(desktop.user_id(), &session.id);
    let deadline = Instant::now() + Duration::from_secs(15);
    while calls.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "the real goal executor must reach the fixture model"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let before = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        10,
    )?;
    let executing = before
        .chat
        .iter()
        .find(|row| row.status == "running")
        .expect("a goal execution must be running before pause");
    let service = desktop.state().kernel.orchestrator.goal_handle();
    let (_, paused) = runtime.block_on(wunder_server::goal::execute_goal_command(
        service.as_ref(),
        desktop.state().storage.clone(),
        desktop.user_id(),
        &session.id,
        wunder_server::goal::GoalCommand::Pause,
    ))?;
    assert_eq!(paused.unwrap()["phase"], "paused");
    let domain_only = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        10,
    )?;
    assert_eq!(
        domain_only
            .chat
            .iter()
            .find(|row| row.root_turn_id == executing.root_turn_id)
            .unwrap()
            .status,
        "running",
        "the old domain-only route changes the banner but not the executor"
    );
    runtime.block_on(wunder_server::goal::execute_goal_command(
        service.as_ref(),
        desktop.state().storage.clone(),
        desktop.user_id(),
        &session.id,
        wunder_server::goal::GoalCommand::Resume,
    ))?;
    desktop.execute_chat_command(&session.id, NativeChatCommand::GoalPause)?;
    let after = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        10,
    )?;
    let stopped = after
        .chat
        .iter()
        .find(|row| row.root_turn_id == executing.root_turn_id)
        .unwrap();
    assert_eq!(
        stopped.status, "cancelled",
        "a paused banner must not hide a running executor"
    );
    assert!(runtime.block_on(
        desktop
            .state()
            .kernel
            .thread_runtime
            .is_session_available_for_submit(desktop.user_id(), &session.id)
    ));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let turns = desktop.load_chat_turns(&session.id)?;
    assert!(turns
        .iter()
        .any(|turn| turn.root_id == executing.root_turn_id));
    desktop.cancel_chat(&session.id)?;
    Ok(())
}

#[test]
fn goal_banner_save_updates_an_existing_goal() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
    let session = desktop.create_session(None)?;
    desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::GoalSet("Fixture objective".into()),
    )?;
    desktop.execute_chat_command(&session.id, NativeChatCommand::GoalPause)?;
    desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::GoalEdit("Revised fixture objective".into()),
    )?;
    let sessions = desktop.list_sessions(None)?;
    let goal = &sessions
        .iter()
        .find(|item| item.id == session.id)
        .unwrap()
        .goal;
    assert_eq!(goal.objective, "Revised fixture objective");
    assert_eq!(goal.phase, "paused");
    assert!(!goal.active());
    desktop.cancel_chat(&session.id)?;
    Ok(())
}

#[test]
fn goal_edit_grammar_and_rejections_remain_truthful() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
    let session = desktop.create_session(None)?;
    let invalid = desktop
        .execute_chat_command(&session.id, NativeChatCommand::parse("/goal edit").unwrap())?;
    assert!(invalid.contains("replacement objective"));
    assert!(desktop
        .state()
        .storage
        .get_session_goal(desktop.user_id(), &session.id)?
        .is_none());
    desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::parse("/goal 示例目标").unwrap(),
    )?;
    desktop.execute_chat_command(&session.id, NativeChatCommand::GoalPause)?;
    let rejected = desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::GoalSet("Replacement fixture".into()),
    )?;
    assert!(rejected.contains("already paused"));
    desktop.execute_chat_command(
        &session.id,
        NativeChatCommand::parse("/goal edit 修订后的示例目标").unwrap(),
    )?;
    let goal = desktop
        .state()
        .storage
        .get_session_goal(desktop.user_id(), &session.id)?
        .unwrap();
    assert_eq!(goal.objective, "修订后的示例目标");
    assert_eq!(goal.phase, "paused");
    let page = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        10,
    )?;
    assert_eq!(page.chat.len(), 4);
    assert_eq!(
        page.chat.last().unwrap().user.content,
        "/goal edit 修订后的示例目标"
    );
    desktop.execute_chat_command(&session.id, NativeChatCommand::GoalClear)?;
    assert!(desktop
        .state()
        .storage
        .get_session_goal(desktop.user_id(), &session.id)?
        .is_none());
    Ok(())
}
