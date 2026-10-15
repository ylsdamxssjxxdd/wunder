//! Workspace façade contract tests. A workspace binds a real user folder;
//! threads live inside exactly one workspace, the sidebar order persists,
//! and deleting a workspace never touches disk contents — threads are
//! archived or their records dropped per the caller's choice.

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

fn make_folder(root: &std::path::Path, name: &str) -> String {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("folder");
    path.canonicalize()
        .expect("canonical")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn workspace_crud_binds_threads_and_persists_order() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;

    let first =
        runtime.create_workspace("项目一", &make_folder(host.path(), "one"), "code", "teal")?;
    let second = runtime.create_workspace(
        "项目二",
        &make_folder(host.path(), "two"),
        "folder",
        "amber",
    )?;
    assert_ne!(first.workspace_id, second.workspace_id);

    // Startup migration guarantees a default workspace; the new ones join it.
    let listed = runtime.list_workspaces()?;
    assert!(listed.len() >= 3);
    assert!(listed.iter().any(|w| w.workspace_id == first.workspace_id));

    // Threads are created inside a workspace and counted per workspace.
    let session = runtime.create_session(Some(&first.workspace_id))?;
    assert_eq!(
        session.workspace_id.as_deref(),
        Some(first.workspace_id.as_str())
    );
    assert_eq!(session.workspace_name, "项目一");
    let counted = runtime
        .list_workspaces()?
        .into_iter()
        .find(|workspace| workspace.workspace_id == first.workspace_id)
        .expect("workspace listed");
    assert_eq!(counted.thread_count, 1);

    // Sidebar ordering persists; the default workspace joins the reorder.
    let default_id = runtime
        .list_workspaces()?
        .iter()
        .find(|workspace| workspace.name == "默认工作区")
        .map(|workspace| workspace.workspace_id.clone())
        .expect("default workspace listed");
    runtime.reorder_workspaces(&[
        second.workspace_id.clone(),
        first.workspace_id.clone(),
        default_id,
    ])?;
    let listed = runtime.list_workspaces()?;
    assert_eq!(listed[0].workspace_id, second.workspace_id);

    // Unknown workspace ids are rejected when creating threads.
    assert!(runtime.create_session(Some("ws_missing")).is_err());
    Ok(())
}

#[test]
fn workspace_path_validation_reports_occupancy_and_nesting(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let root = make_folder(host.path(), "project");
    runtime.create_workspace("已占用", &root, "folder", "blue")?;

    let report = runtime.validate_workspace_path(&root)?;
    assert!(report.exists && report.is_directory);
    assert!(report.readable && report.writable);
    assert_eq!(report.occupied_by.as_deref(), Some("已占用"));

    // A subfolder of an existing workspace folder is rejected as nested.
    let sub = make_folder(host.path(), "project/sub");
    let report = runtime.validate_workspace_path(&sub)?;
    assert!(report.nested_in.is_some() || report.contains.is_some());

    let missing =
        runtime.validate_workspace_path(host.path().join("not-there").to_str().unwrap())?;
    assert!(!missing.exists);
    Ok(())
}

#[test]
fn delete_workspace_archives_or_discards_threads_without_touching_disk(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let root = make_folder(host.path(), "to-delete");
    let marker = std::path::Path::new(&root).join("keep-me.txt");
    std::fs::write(&marker, "磁盘内容必须保留")?;

    // Archive policy: thread stays, only the workspace binding goes away.
    let archived_ws = runtime.create_workspace("归档区", &root, "doc", "gray")?;
    runtime.create_session(Some(&archived_ws.workspace_id))?;
    let summary = runtime.delete_workspace(&archived_ws.workspace_id, false)?;
    assert_eq!(summary.archived_threads, 1);
    assert!(runtime
        .list_workspaces()?
        .iter()
        .all(|w| w.workspace_id != archived_ws.workspace_id));
    assert_eq!(std::fs::read_to_string(&marker)?, "磁盘内容必须保留");

    // Discard policy: thread records go, disk still untouched.
    let deleted_ws = runtime.create_workspace("删除区", &root, "doc", "red")?;
    runtime.create_session(Some(&deleted_ws.workspace_id))?;
    let summary = runtime.delete_workspace(&deleted_ws.workspace_id, true)?;
    assert_eq!(summary.deleted_threads, 1);
    assert_eq!(std::fs::read_to_string(&marker)?, "磁盘内容必须保留");
    Ok(())
}

#[test]
fn default_workspace_lands_in_the_user_workspace_folder() -> Result<(), Box<dyn std::error::Error>>
{
    // The working directory must be the user directory's own `workspace`
    // subfolder (matching the cloud split); the user directory itself keeps
    // the engine config folders (agents/global/skills/…) and never shows up
    // as a working directory.
    let runtime = start_isolated()?;
    let default = runtime
        .list_workspaces()?
        .into_iter()
        .find(|workspace| workspace.name == "默认工作区")
        .expect("startup migration created the default workspace");
    let root = std::path::Path::new(&default.root_path);
    assert!(root.is_dir(), "{}", default.root_path);
    assert_eq!(
        root.file_name().and_then(|name| name.to_str()),
        Some("workspace")
    );
    let user_dir = root.parent().expect("user directory");
    assert!(user_dir.is_dir());
    Ok(())
}

#[test]
fn local_workspace_folder_crud_stays_inside_the_root() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let root = make_folder(host.path(), "local-ops");
    runtime.workspace_local_mkdir(&root, "notes")?;
    runtime.workspace_local_write(&root, "notes/a.md")?;
    assert!(std::path::Path::new(&root).join("notes/a.md").is_file());

    // Overwrites are refused for local writes.
    assert!(runtime.workspace_local_write(&root, "notes/a.md").is_err());

    runtime.workspace_local_rename(&root, "notes/a.md", "b.md")?;
    assert!(std::path::Path::new(&root).join("notes/b.md").is_file());
    // The name is one segment; traversal is rejected up front.
    assert!(runtime.workspace_local_rename(&root, "notes/b.md", "../escape").is_err());
    assert!(runtime.workspace_local_mkdir(&root, "../outside").is_err());

    let resolved = runtime.workspace_local_resolve(&root, "notes/b.md")?;
    assert!(resolved.is_file());
    // Directories are not drag-out material.
    assert!(runtime.workspace_local_resolve(&root, "notes").is_err());

    runtime.workspace_local_delete(&root, "notes/b.md")?;
    assert!(!std::path::Path::new(&root).join("notes/b.md").exists());
    runtime.workspace_local_delete(&root, "notes")?;
    assert!(!std::path::Path::new(&root).join("notes").exists());
    // The workspace root itself is never deletable.
    assert!(runtime.workspace_local_delete(&root, ".").is_err());
    Ok(())
}
