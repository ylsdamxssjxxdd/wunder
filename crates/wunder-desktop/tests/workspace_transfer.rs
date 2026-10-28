//! Workspace façade contract tests. A workspace binds a real user folder;
//! threads live inside exactly one workspace, the sidebar order persists,
//! and deleting a workspace never touches disk contents — threads are
//! archived or their records dropped per the caller's choice.

use wunder_desktop::{args::DesktopArgs, NativeDesktop};

// Runtime startup projects process-wide tool paths. Keep the entire fixture
// serialized and bind its actual state home, not only its temporary files.
struct IsolatedRuntime {
    runtime: NativeDesktop,
    _directory: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for IsolatedRuntime {
    type Target = NativeDesktop;
    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

fn start_isolated() -> Result<IsolatedRuntime, Box<dyn std::error::Error>> {
    start_isolated_with_seed(|_| Ok(()))
}

fn start_isolated_with_seed(
    seed: impl FnOnce(&std::path::Path) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<IsolatedRuntime, Box<dyn std::error::Error>> {
    static FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    seed(directory.path())?;
    std::env::set_var("WUNDER_HOME", directory.path().join("runtime"));
    let runtime = NativeDesktop::start_with_args(isolated_args(directory.path())?)?;
    assert!(directory
        .path()
        .join("runtime/data/state_1.sqlite")
        .is_file());
    Ok(IsolatedRuntime {
        runtime,
        _directory: directory,
        _guard: guard,
    })
}

fn isolated_args(root: &std::path::Path) -> Result<DesktopArgs, std::io::Error> {
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(root.join("runtime").canonicalize()?);
    args.workspace = Some(root.join("workspace"));
    Ok(args)
}

fn make_folder(root: &std::path::Path, name: &str) -> String {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("folder");
    // Production stores workspace roots in the plain canonical form (no
    // Windows `\\?\` prefix). Returning a raw `canonicalize()` here kept the
    // verbatim spelling and hid the containment mismatch that rejected every
    // nested entry at runtime.
    wunder_server::local_bootstrap::normalize_local_workspace_path(&path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
#[test]
fn startup_merges_windows_workspace_paths_and_rebinds_every_thread(
) -> Result<(), Box<dyn std::error::Error>> {
    use wunder_desktop::runtime::DESKTOP_DEFAULT_USER_ID;
    use wunder_server::storage::{
        ChatSessionRecord, ChatSessionStore, SqliteStorage, WorkspaceRecord, WorkspaceStore,
    };

    const KEEPER: &str = "ws_fixture_keeper";
    const DUPLICATE: &str = "ws_fixture_duplicate";
    const UNRELATED: &str = "ws_fixture_unrelated";
    let fixture = start_isolated_with_seed(|root| {
        let project = root.join("project");
        let unrelated = root.join("other");
        std::fs::create_dir_all(&project)?;
        std::fs::create_dir_all(&unrelated)?;
        let plain = wunder_server::local_bootstrap::normalize_local_workspace_path(&project);
        let extended = format!(r"\\?\{}", plain.display());
        assert_ne!(plain.to_string_lossy(), extended);
        let storage = SqliteStorage::new(
            root.join("runtime/data/state_1.sqlite")
                .to_string_lossy()
                .into_owned(),
        );
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        // The extended spelling deliberately wins the sidebar order: its
        // normalized root would hit the plain row's unique index unless the
        // duplicate is removed before root paths are rewritten.
        for (id, path, index) in [
            (KEEPER, extended, 0),
            (DUPLICATE, plain.to_string_lossy().into_owned(), 1),
            (UNRELATED, unrelated.to_string_lossy().into_owned(), 2),
        ] {
            storage.upsert_workspace(&WorkspaceRecord {
                workspace_id: id.into(),
                user_id: DESKTOP_DEFAULT_USER_ID.into(),
                name: id.into(),
                root_path: path,
                icon: "folder".into(),
                color: "teal".into(),
                sort_index: index,
                created_at: now,
                updated_at: now,
            })?;
        }
        for (id, workspace_id, status) in [
            ("fixture-kept", KEEPER, "active"),
            ("fixture-moved", DUPLICATE, "active"),
            ("fixture-archived", DUPLICATE, "archived"),
            ("fixture-unrelated", UNRELATED, "active"),
        ] {
            storage.upsert_chat_session(&ChatSessionRecord {
                session_id: id.into(),
                user_id: DESKTOP_DEFAULT_USER_ID.into(),
                title: id.into(),
                status: status.into(),
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: None,
                workspace_id: Some(workspace_id.into()),
                tool_overrides: Vec::new(),
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })?;
        }
        Ok(())
    })?;
    let normalized = wunder_server::local_bootstrap::normalize_local_workspace_path(
        &fixture._directory.path().join("project"),
    );
    let assert_migrated = |runtime: &NativeDesktop| -> Result<(), Box<dyn std::error::Error>> {
        let workspaces = runtime.list_workspaces()?;
        assert_eq!(workspaces.len(), 2);
        let keeper = workspaces
            .iter()
            .find(|record| record.workspace_id == KEEPER)
            .expect("stable workspace id kept");
        assert_eq!(
            std::path::Path::new(&keeper.root_path),
            normalized.as_path()
        );
        assert_eq!(keeper.name, KEEPER);
        assert_eq!(keeper.color, "teal");
        assert_eq!(keeper.sort_index, 0);
        assert_eq!(keeper.thread_count, 2);
        assert!(workspaces
            .iter()
            .all(|record| record.workspace_id != DUPLICATE));
        let storage = &runtime.state().storage;
        for (id, expected_workspace, expected_status) in [
            ("fixture-kept", KEEPER, "active"),
            ("fixture-moved", KEEPER, "active"),
            ("fixture-archived", KEEPER, "archived"),
            ("fixture-unrelated", UNRELATED, "active"),
        ] {
            let record = storage
                .get_chat_session(runtime.user_id(), id)?
                .expect("thread retained");
            assert_eq!(record.workspace_id.as_deref(), Some(expected_workspace));
            assert_eq!(record.status, expected_status);
            assert_eq!(record.title, id);
        }
        let scope = runtime
            .state()
            .workspace
            .scoped_user_id_for_workspace(runtime.user_id(), KEEPER);
        assert_eq!(runtime.state().workspace.workspace_root(&scope), normalized);
        Ok(())
    };
    assert_migrated(&fixture)?;

    // The durable migration marker skips data rewriting on the next launch,
    // but process-local tool roots must still resolve to the real directory.
    let IsolatedRuntime {
        runtime,
        _directory: directory,
        _guard: guard,
    } = fixture;
    drop(runtime);
    let fixture = IsolatedRuntime {
        runtime: NativeDesktop::start_with_args(isolated_args(directory.path())?)?,
        _directory: directory,
        _guard: guard,
    };
    assert_migrated(&fixture)?;
    Ok(())
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
    let workspace = runtime
        .list_workspaces()?
        .into_iter()
        .find(|workspace| workspace.root_path == root)
        .expect("fixture workspace");
    let edit_report = runtime.validate_workspace_edit_path(&root, &workspace.workspace_id)?;
    assert!(edit_report.occupied_by.is_none());
    assert!(edit_report.nested_in.is_none());
    assert!(edit_report.contains.is_none());

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
    // The seeded root must be immediately usable: entry resolution compares it
    // with resolved children, so a `\\?\` spelling here made every nested
    // entry fail with "路径超出当前工作区".
    runtime.workspace_local_mkdir(&default.root_path, "notes")?;
    runtime.workspace_local_write(&default.root_path, "notes/readme.md")?;
    let resolved = runtime.workspace_local_resolve(&default.root_path, "notes/readme.md")?;
    assert!(resolved.is_file());
    let listing = runtime.workspace_local_listing(&default.root_path, "notes", 0)?;
    assert!(listing
        .entries
        .iter()
        .any(|entry| entry.name == "readme.md"));
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
    assert!(runtime
        .workspace_local_rename(&root, "notes/b.md", "../escape")
        .is_err());
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

#[test]
fn local_copy_uses_selected_directory_and_preserves_self_copy(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let root = make_folder(host.path(), "selected");
    runtime.workspace_local_mkdir(&root, "notes")?;
    let source = host.path().join("sample.txt");
    std::fs::write(&source, "fixture content")?;
    assert_eq!(
        runtime.workspace_local_upload_to(&root, "notes", &source)?,
        "notes/sample.txt"
    );
    let copied = std::path::Path::new(&root).join("notes/sample.txt");
    assert!(!std::path::Path::new(&root).join("sample.txt").exists());
    runtime.workspace_local_upload_to(&root, "notes", &copied)?;
    assert_eq!(std::fs::read_to_string(copied)?, "fixture content");
    assert!(runtime
        .workspace_local_upload_to(&root, "../outside", &source)
        .is_err());
    assert!(runtime
        .workspace_local_upload_to(&root, "missing", &source)
        .is_err());
    Ok(())
}

#[test]
fn local_directory_pages_have_stable_numeric_order_and_exact_total(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    let root = make_folder(host.path(), "paged");
    runtime.workspace_local_mkdir(&root, "folder10")?;
    runtime.workspace_local_mkdir(&root, "folder2")?;
    for index in (0..405).rev() {
        std::fs::write(
            std::path::Path::new(&root).join(format!("sample{index}.txt")),
            "fixture",
        )?;
    }
    let first = runtime.workspace_local_listing(&root, "", 0)?;
    let second = runtime.workspace_local_listing(&root, "", 200)?;
    let third = runtime.workspace_local_listing(&root, "", 400)?;
    assert_eq!(first.total, 407);
    assert_eq!(first.entries[0].name, "folder2");
    assert_eq!(first.entries[1].name, "folder10");
    let names: Vec<_> = first
        .entries
        .into_iter()
        .chain(second.entries)
        .chain(third.entries)
        .skip(2)
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        names,
        (0..405)
            .map(|index| format!("sample{index}.txt"))
            .collect::<Vec<_>>()
    );
    assert!(runtime.workspace_local_listing(&root, "", 2000).is_err());
    Ok(())
}

#[test]
fn sidebar_split_preference_round_trips_and_rejects_invalid_values(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    assert_eq!(runtime.sidebar_split_ratio()?, 0.6);
    runtime.save_sidebar_split_ratio(0.72)?;
    assert_eq!(runtime.sidebar_split_ratio()?, 0.72);
    for ratio in [0.0, 1.0, f32::NAN, f32::INFINITY] {
        assert!(runtime.save_sidebar_split_ratio(ratio).is_err());
    }
    assert_eq!(runtime.sidebar_split_ratio()?, 0.72);
    Ok(())
}

#[test]
fn saving_thread_order_preserves_other_navigation_preferences(
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let mut order = runtime.navigation_order()?;
    order.agents = vec!["fixture-agent".into()];
    order.models = vec!["fixture-model".into()];
    runtime.save_navigation_order(&order)?;
    let threads = vec!["fixture-thread-b".into(), "fixture-thread-a".into()];
    runtime.save_thread_order(&threads)?;
    let saved = runtime.navigation_order()?;
    assert_eq!(saved.threads, threads);
    assert_eq!(saved.agents, order.agents);
    assert_eq!(saved.models, order.models);
    assert!(runtime
        .save_thread_order(&vec!["duplicate".into(); 2])
        .is_err());
    Ok(())
}

#[test]
fn deleting_workspace_drains_every_thread_page() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = start_isolated()?;
    let host = tempfile::tempdir()?;
    for (index, discard) in [false, true].into_iter().enumerate() {
        let root = make_folder(host.path(), &format!("folder{index}"));
        let workspace = runtime.create_workspace("示例工作区", &root, "folder", "blue")?;
        let session = runtime.create_session(Some(&workspace.workspace_id))?;
        let storage = &runtime.state().storage;
        let template = storage
            .get_chat_session(runtime.user_id(), &session.id)?
            .expect("fixture thread");
        for number in 1..205 {
            let mut row = template.clone();
            row.session_id = format!("fixture-thread-{index}-{number}");
            storage.upsert_chat_session(&row)?;
        }
        let summary = runtime.delete_workspace(&workspace.workspace_id, discard)?;
        if discard {
            assert_eq!(summary.deleted_threads, 205);
        } else {
            assert_eq!(summary.archived_threads, 205);
        }
        let (_, remaining) = storage.list_chat_sessions_by_workspace(
            runtime.user_id(),
            &workspace.workspace_id,
            if discard { None } else { Some("active") },
            0,
            1,
        )?;
        assert_eq!(remaining, 0);
        assert!(std::path::Path::new(&root).is_dir());
    }
    Ok(())
}
