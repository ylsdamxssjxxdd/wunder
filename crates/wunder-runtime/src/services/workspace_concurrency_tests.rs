use super::*;

#[test]
fn concurrent_file_only_workspace_access_does_not_materialize_history_storage() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state_1.sqlite");
    let storage = Arc::new(crate::storage::SqliteStorage::new(
        state_path.to_string_lossy().into_owned(),
    ));
    let workspace = Arc::new(WorkspaceManager::new(
        &dir.path().join("files").to_string_lossy(),
        storage,
        0,
        &HashMap::new(),
    ));
    std::thread::scope(|scope| {
        for index in 0..8 {
            let workspace = Arc::clone(&workspace);
            scope.spawn(move || {
                let root = workspace
                    .ensure_user_root(&format!("sample-user-{index}"))
                    .unwrap();
                assert!(root.is_dir());
            });
        }
    });
    assert!(!state_path.exists());
    for name in ["queue_1.sqlite", "thread_history_1.sqlite", "logs_1.sqlite"] {
        assert!(
            !dir.path().join(name).exists(),
            "file access must not create {name}"
        );
    }
    assert!(!dir.path().join("sessions").exists());
}
