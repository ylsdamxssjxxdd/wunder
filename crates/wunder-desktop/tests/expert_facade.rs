//! Expert memory and archive contracts, using an isolated local runtime.
//! The desktop owns a single built-in agent, so memories and archives are
//! user-global and unknown agent scopes are rejected.
use wunder_desktop::{args::DesktopArgs, NativeDesktop};
use wunder_server::storage::ThreadOpenLeaseStore;

struct IsolatedDesktop {
    api: NativeDesktop,
    _directory: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for IsolatedDesktop {
    type Target = NativeDesktop;

    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

fn isolated_desktop() -> anyhow::Result<IsolatedDesktop> {
    static FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    std::fs::write(
        config.join("desktop.settings.json"),
        r#"{"lan_mesh":{"enabled":false},"llm":{"default":"","models":{}}}"#,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::env::set_var("WUNDER_HOME", directory.path().join("runtime"));
    let api = NativeDesktop::start_with_args(args)?;
    Ok(IsolatedDesktop {
        api,
        _directory: directory,
        _guard: guard,
    })
}

#[test]
fn expert_memories_archives_and_runtime_are_single_agent_scoped() -> anyhow::Result<()> {
    let api = isolated_desktop()?;
    api.save_expert_memory(
        "__default__",
        "",
        "sample-title",
        "sample-content",
        "preference",
    )?;
    let rows = api.expert_memories("__default__", "sample-content", "preference")?;
    assert_eq!(rows.len(), 1);
    api.save_expert_memory(
        "__default__",
        &rows[0].id,
        "sample-revised",
        "sample-revised-content",
        "preference",
    )?;
    assert_eq!(
        api.expert_memories("__default__", "sample-revised", "")?
            .len(),
        1
    );
    // Replicating onto the same single agent is a no-op source/target clash.
    assert!(api
        .replicate_expert_memories("__default__", "__default__")
        .is_err());
    api.delete_expert_memory("__default__", &rows[0].id)?;
    assert!(api.expert_memories("__default__", "", "")?.is_empty());

    let first = api.create_session(None)?;
    let second = api.create_session(None)?;
    api.archive_session(&first.id)?;
    api.archive_session(&second.id)?;
    let (rows, total) = api.expert_archives("__default__", 0)?;
    assert_eq!(total, 2);
    assert_eq!(rows[0].id, second.id);
    assert!(api.expert_archives("__default__", 50)?.0.is_empty());
    let runtime = api.expert_runtime("__default__", Some("2026-01-01"))?;
    assert_eq!(runtime["data"]["daily"].as_array().unwrap().len(), 14);
    assert_eq!(runtime["data"]["summary"]["tool_calls"], 0);
    assert!(api.expert_memories("missing-agent", "", "").is_err());
    assert!(api.expert_archives("missing-agent", 0).is_err());
    Ok(())
}

#[test]
fn archived_threads_can_be_restored_or_deleted_but_active_threads_are_protected(
) -> anyhow::Result<()> {
    let api = isolated_desktop()?;
    let restored = api.create_session(None)?;
    let removed = api.create_session(None)?;
    let active = api.create_session(None)?;

    // The permanent-delete operation is intentionally archive-only. It must
    // never become a bypass around the normal archive/cancel protection.
    assert!(api.delete_archived_session(&active.id).is_err());
    assert_eq!(api.expert_archives("__default__", 0)?.1, 0);

    api.archive_session(&restored.id)?;
    api.archive_session(&removed.id)?;
    let (archives, total) = api.expert_archives("__default__", 0)?;
    assert_eq!(total, 2);
    let archive_ids = archives
        .iter()
        .map(|session| session.id.as_str())
        .collect::<Vec<_>>();
    assert!(archive_ids.contains(&restored.id.as_str()));
    assert!(archive_ids.contains(&removed.id.as_str()));
    assert!(!archive_ids.contains(&active.id.as_str()));

    // Restore returns the session to the active catalogue and removes it from
    // the archive projection without implicitly acquiring an execution lease.
    api.restore_session(&restored.id)?;
    let (archives, total) = api.expert_archives("__default__", 0)?;
    assert_eq!(total, 1);
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].id, removed.id);
    let active_ids = api
        .list_sessions(None)?
        .into_iter()
        .map(|session| session.id)
        .collect::<Vec<_>>();
    assert!(active_ids.contains(&restored.id));
    assert!(active_ids.contains(&active.id));
    assert!(!active_ids.contains(&removed.id));

    api.delete_archived_session(&removed.id)?;
    assert_eq!(api.expert_archives("__default__", 0)?.1, 0);
    assert!(api.get_session(&removed.id).is_err());
    assert!(api.delete_archived_session(&restored.id).is_err());
    Ok(())
}

#[test]
fn archived_detail_is_read_only_and_does_not_acquire_a_thread_lease() -> anyhow::Result<()> {
    let api = isolated_desktop()?;
    let session = api.create_session(None)?;
    api.archive_session(&session.id)?;

    let (detail, _messages) = api.get_archived_session_detail(&session.id)?;
    assert_eq!(detail.id, session.id);
    assert!(api
        .state()
        .storage
        .get_thread_open_lease(&session.id, api.user_id())?
        .is_none());
    assert!(api.get_archived_session_detail("missing").is_err());

    Ok(())
}
