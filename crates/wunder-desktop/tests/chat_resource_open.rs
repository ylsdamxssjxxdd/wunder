//! A transcript reference must resolve to the same file the row rendered.
//!
//! The chat picture loader accepts three source shapes on a local install:
//! chat attachments in the private store, a workspace-relative path, and — on
//! the single-root local forms the desktop ships — any local absolute path.
//! Clicking a card used to go through the plain workspace resolver, so every
//! absolute reference failed with "路径超出当前工作区" while its pixels showed
//! up fine. These assertions pin the shared resolution, and pin the boundary
//! that keeps a click from launching something the renderer never carded.

use std::sync::Mutex;
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

/// One runtime per home at a time: a second initializer in the same home races
/// the default-user seed.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn start_isolated() -> Result<(NativeDesktop, std::path::PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().to_path_buf();
    std::env::set_var("WUNDER_HOME", &home);
    std::mem::forget(directory);
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    let engine_config = serde_json::json!({
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false, "api_key": "contract-secret"}}}
    });
    std::fs::write(
        config.join("wunder.yaml"),
        serde_json::to_vec(&engine_config)?,
    )?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(home.join("temp"));
    args.workspace = Some(home.join("workspace"));
    Ok((NativeDesktop::start_with_args(args)?, home))
}

#[test]
fn transcript_references_resolve_like_the_picture_loader() {
    let _serial = serial();
    let (runtime, home) = start_isolated().expect("isolated runtime");
    let root = home.join("workspace");
    let root_text = root.to_string_lossy().to_string();

    let inside = root.join("notes/plan.md");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(&inside, b"# plan").unwrap();
    let outside_dir = home.join("outside");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let outside_doc = outside_dir.join("report.pdf");
    std::fs::write(&outside_doc, b"pdf").unwrap();
    let outside_script = outside_dir.join("deploy.sh");
    std::fs::write(&outside_script, b"#!/bin/sh\n").unwrap();

    // Workspace-relative and workspace-absolute references both reach the file
    // the transcript drew.
    for source in ["notes/plan.md", "./notes/plan.md"] {
        let target = runtime
            .chat_resource_target(&root_text, source)
            .expect("relative document resolves");
        assert_eq!(target.canonicalize().unwrap(), inside.canonicalize().unwrap());
    }
    let absolute_inside = inside.to_string_lossy().to_string();
    let target = runtime
        .chat_resource_target(&root_text, &absolute_inside)
        .expect("workspace absolute document resolves");
    assert_eq!(target.canonicalize().unwrap(), inside.canonicalize().unwrap());

    // A document card outside the workspace renders on a local install, so the
    // click has to reach it too — even with no workspace bound at all.
    let absolute_outside = outside_doc.to_string_lossy().to_string();
    for root_path in [root_text.as_str(), ""] {
        let target = runtime
            .chat_resource_target(root_path, &absolute_outside)
            .expect("out-of-workspace card resolves");
        assert_eq!(
            target.canonicalize().unwrap(),
            outside_doc.canonicalize().unwrap()
        );
    }

    // The absolute shortcut belongs to card extensions only: text the renderer
    // kept as prose must not become a way to launch a program.
    let absolute_script = outside_script.to_string_lossy().to_string();
    assert!(runtime
        .chat_resource_target(&root_text, &absolute_script)
        .is_err());
    // Escaping a relative source stays rejected, and a missing file is named.
    assert!(runtime
        .chat_resource_target(&root_text, "../outside/report.pdf")
        .is_err());
    assert!(runtime
        .chat_resource_target(&root_text, &absolute_outside.replace("report.pdf", "missing.pdf"))
        .is_err());
}

/// The Android shell cannot open a bare host path: its viewer receives a
/// content URI built from a registered workspace id and a relative path. A card
/// click must therefore map back onto the catalogue entry the workspace drawer
/// uses, and refuse the files no provider serves instead of opening something
/// else.
#[test]
fn transcript_cards_map_onto_the_workspace_provider_route() {
    let _serial = serial();
    let (runtime, home) = start_isolated().expect("isolated runtime");
    let workspace = runtime
        .list_workspaces()
        .expect("workspace catalogue")
        .into_iter()
        .next()
        .expect("default workspace");
    let root = std::path::PathBuf::from(&workspace.root_path);
    let inside = root.join("notes/plan.md");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(&inside, b"# plan").unwrap();

    let (workspace_id, relative) = runtime
        .chat_resource_document(&workspace.root_path, "notes/plan.md")
        .expect("workspace document maps to a provider route");
    assert_eq!(workspace_id, workspace.workspace_id);
    assert_eq!(relative, "notes/plan.md");

    // Absolute references reach the same catalogue entry.
    let absolute = inside.to_string_lossy().to_string();
    let (workspace_id, relative) = runtime
        .chat_resource_document(&workspace.root_path, &absolute)
        .expect("absolute workspace document maps");
    assert_eq!(workspace_id, workspace.workspace_id);
    assert_eq!(relative, "notes/plan.md");

    // A local card outside every workspace has no provider route.
    let outside = home.join("outside/report.pdf");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"pdf").unwrap();
    let outside_text = outside.to_string_lossy().to_string();
    assert!(runtime
        .chat_resource_document(&workspace.root_path, &outside_text)
        .is_err());
}
