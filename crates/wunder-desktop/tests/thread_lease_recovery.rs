//! 崩溃重启后的线程占用收敛：上一进程留下的打开租约与执行锁必须立刻
//! 失效，否则重新打开线程只会回 THREAD_IN_USE，看起来像永久锁定。

use std::sync::{Mutex, MutexGuard};
use wunder_desktop::{args::DesktopArgs, NativeDesktop};
use wunder_server::storage::{SessionLockStore, ThreadOpenLeaseStatus, ThreadOpenLeaseStore};

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

fn isolated_args(directory: &tempfile::TempDir) -> anyhow::Result<DesktopArgs> {
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
    std::env::set_var("WUNDER_HOME", directory.path().join("runtime"));
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime"));
    args.workspace = Some(directory.path().join("workspace"));
    Ok(args)
}

#[test]
fn restart_releases_dead_desktop_lease_and_orphan_session_lock() -> anyhow::Result<()> {
    let _guard: MutexGuard<'static, ()> = RUNTIME_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let args = isolated_args(&directory)?;

    let first = NativeDesktop::start_with_args(args)?;
    let session = first.create_session(None)?;
    let user = first.user_id().to_string();
    // 模拟上一进程被强杀：租约与执行锁都还远未到 TTL。
    assert!(matches!(
        first.state().storage.try_acquire_thread_open_lease(
            &session.id,
            &user,
            "dead-desktop-instance",
            "desktop",
            300.0,
        )?,
        ThreadOpenLeaseStatus::Acquired { .. }
    ));
    first
        .state()
        .storage
        .try_acquire_session_lock(&session.id, &user, "agent", 300.0, 8)?;
    drop(first);

    // 同一 home 的下一次启动必须立刻接管，而不是等 15 秒 / 120 秒的 TTL。
    let second = NativeDesktop::start_with_args(isolated_args(&directory)?)?;
    second.open_session(&session.id)?;
    let lease = second
        .state()
        .storage
        .get_thread_open_lease(&session.id, &user)?;
    assert!(lease.is_some_and(|lease| lease.owner_instance_id != "dead-desktop-instance"));
    Ok(())
}
