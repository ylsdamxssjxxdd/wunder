//! What the composer reads right after a stop.
//!
//! The input box decides whether a new message waits locally or goes to the
//! engine by asking one question: does the durable snapshot still show work?
//! (`native_chat_observer::durable_busy` matches `正在生成…` / `正在排队`.) A
//! stop therefore must not settle the turn by itself: while the executor is
//! still winding down the thread has to keep reading as running, otherwise the
//! next message is submitted and the engine parks it behind work that never
//! stopped — the status says stopped, the tools keep writing, and a「排队中」
//! bubble appears that nobody asked for.

use std::sync::{Mutex, MutexGuard};
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

fn isolated() -> anyhow::Result<(MutexGuard<'static, ()>, tempfile::TempDir, NativeDesktop)> {
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
                    "model": "fixture-model", "base_url": "http://127.0.0.1:9/v1",
                    "api_key": "fixture-key", "timeout_s": 30, "stream": true,
                    "max_context": 65536, "max_output": 64}
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
fn a_winding_down_turn_still_reads_as_running_to_the_composer() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
    let session = desktop.create_session(None)?;
    let user = desktop.user_id().to_string();
    let accepted = desktop.state().storage.accept_thread_turn(
        &user,
        &session.id,
        &serde_json::json!({"content": "fixture input"}),
    )?;
    let turn = accepted["turn_id"]
        .as_str()
        .expect("accepted turn id")
        .to_string();

    // A stop request reached the engine while its executor is still inside a
    // tool call: the turn stays open and the thread must not read as idle.
    desktop.state().storage.update_thread_turn(
        &user,
        &session.id,
        &turn,
        "running",
        "",
        &serde_json::json!({}),
    )?;
    let turns = desktop.load_chat_turns(&session.id)?;
    let latest = turns.last().expect("one durable turn");
    assert_eq!(
        latest.assistant.stats_status, "正在生成…",
        "the composer holds the next message on exactly this phrase"
    );

    // Only the executor's own terminal turns the thread into「已停止」.
    desktop.state().storage.update_thread_turn(
        &user,
        &session.id,
        &turn,
        "cancelled",
        "",
        &serde_json::json!({"stop_reason": "user_stop"}),
    )?;
    let turns = desktop.load_chat_turns(&session.id)?;
    assert_eq!(turns.last().unwrap().assistant.stats_status, "已停止");
    desktop.cancel_chat(&session.id)?;
    Ok(())
}
