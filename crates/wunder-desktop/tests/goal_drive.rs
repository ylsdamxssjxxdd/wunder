//! Reproduction for "蛹的目标不会持续驱动": a desktop session with an armed
//! goal and a tool-less virtual model must keep starting rounds on its own.
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wunder_desktop::{args::DesktopArgs, native::NativeChatCommand, NativeDesktop};

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

fn isolated() -> anyhow::Result<(
    std::sync::MutexGuard<'static, ()>,
    tempfile::TempDir,
    NativeDesktop,
)> {
    let guard = RUNTIME_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(
        config.join("wunder.yaml"),
        serde_json::to_vec(&serde_json::json!({
            "llm": {"default": "fixture-virtual", "models": {
                "fixture-virtual": {"enable": true, "provider": "virtual",
                    "model": "fixture-virtual", "timeout_s": 30, "stream": true,
                    "max_context": 16384, "max_output": 64,
                    "simulation_speed": "fast",
                    "simulation": {"support_tools": false, "support_reasoning": false,
                        "image_tokens": 256, "audio_tokens": 1024,
                        "error_rate": 0.0, "disconnect_rate": 0.0}}
            }},
            "tools": {"builtin": {"enabled": []}},
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
fn goal_keeps_driving_rounds_without_human_input() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
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
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let goal = desktop
            .state()
            .storage
            .get_session_goal(desktop.user_id(), &session.id)?
            .expect("goal record");
        if goal.rounds_started >= 3 {
            break;
        }
        if Instant::now() > deadline {
            let page = desktop.state().storage.thread_view_page(
                desktop.user_id(),
                &session.id,
                "chat",
                None,
                10,
            )?;
            let statuses: Vec<String> = page
                .chat
                .iter()
                .map(|row| format!("{}:{}", row.root_turn_id, row.status))
                .collect();
            panic!(
                "goal stalled at round {} (phase {}, activation); turn rows: {:?}",
                goal.rounds_started, goal.phase, statuses
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    desktop.cancel_chat(&session.id)?;
    Ok(())
}
