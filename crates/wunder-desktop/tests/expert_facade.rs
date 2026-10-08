//! Expert memory and archive contracts, using an isolated local runtime.
//! The desktop owns a single built-in agent, so memories and archives are
//! user-global and unknown agent scopes are rejected.
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

#[test]
fn expert_memories_archives_and_runtime_are_single_agent_scoped() -> anyhow::Result<()> {
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
    let api = NativeDesktop::start_with_args(args)?;
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
