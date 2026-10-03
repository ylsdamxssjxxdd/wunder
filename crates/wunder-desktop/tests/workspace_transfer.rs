//! Workspace transfer contract tests. Desktop file transfer stays on the
//! local link: import streams a host file into the confined container and
//! export copies it back out — no byte channel through the façade, and path
//! confinement still rejects escapes on both sides.

use wunder_desktop::{args::DesktopArgs, NativeDesktop};

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(NativeDesktop::start_with_args(args)?)
}

#[test]
fn import_then_export_round_trips_through_local_copy() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let source = host.path().join("import-source.txt");
    std::fs::write(&source, "本地导入内容\n第二行")?;

    runtime.import_workspace_file("", source.to_str().unwrap(), "imported.txt")?;
    let page = runtime.workspace_directory("", "", 0)?;
    assert!(page
        .entries
        .iter()
        .any(|entry| entry.name == "imported.txt"));
    let preview = runtime.workspace_preview("", "imported.txt")?;
    assert!(preview.starts_with("本地导入内容"));

    let target = host.path().join("export-target.txt");
    runtime.export_workspace_file("", "imported.txt", target.to_str().unwrap())?;
    assert_eq!(std::fs::read_to_string(&target)?, "本地导入内容\n第二行");
    Ok(())
}

#[test]
fn transfer_rejects_missing_sources_bad_destinations_and_directories(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;

    // Import source must be an existing regular file.
    assert!(runtime
        .import_workspace_file(
            "",
            host.path().join("missing.txt").to_str().unwrap(),
            "a.txt"
        )
        .is_err());
    assert!(runtime
        .import_workspace_file("", host.path().to_str().unwrap(), "a.txt")
        .is_err());
    // Import destination stays confined: escapes and absolute forms fail.
    assert!(runtime
        .import_workspace_file(
            "",
            host.path().join("s.txt").to_str().unwrap(),
            "../escape.txt"
        )
        .is_err());

    // Export source must be a regular file inside the container.
    let target = host.path().join("out.txt");
    assert!(runtime
        .export_workspace_file("", "missing.txt", target.to_str().unwrap())
        .is_err());
    assert!(runtime
        .export_workspace_file("", "", target.to_str().unwrap())
        .is_err());
    assert!(runtime
        .export_workspace_file("", "../escape.txt", target.to_str().unwrap())
        .is_err());

    // Seed one file, then verify an existing directory target is refused.
    let source = host.path().join("seed.txt");
    std::fs::write(&source, "seed")?;
    runtime.import_workspace_file("", source.to_str().unwrap(), "seed.txt")?;
    let dir_target = host.path().join("save-dir");
    std::fs::create_dir(&dir_target)?;
    assert!(runtime
        .export_workspace_file("", "seed.txt", dir_target.to_str().unwrap())
        .is_err());
    Ok(())
}
