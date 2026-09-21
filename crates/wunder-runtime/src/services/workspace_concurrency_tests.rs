use super::*;

#[test]
fn sandbox_file_only_workspace_does_not_start_writer_but_concurrent_writes_flush() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(crate::storage::SqliteStorage::new(
        dir.path().join("storage.db").to_string_lossy().into_owned(),
    ));
    let workspace = Arc::new(WorkspaceManager::new(
        &dir.path().to_string_lossy(),
        storage,
        0,
        &HashMap::new(),
    ));
    assert!(workspace.flush_writes());
    assert!(workspace.write_queue.get().is_none());
    std::thread::scope(|scope| {
        for index in 0..8 {
            let workspace = Arc::clone(&workspace);
            scope.spawn(move || {
                workspace
                    .append_chat(
                        "user",
                        &json!({
                            "session_id": "session", "role": "user", "content": index.to_string(),
                        }),
                    )
                    .unwrap()
            });
        }
    });
    assert!(workspace.flush_writes());
    assert!(workspace.write_queue.get().is_some());
    assert_eq!(
        workspace.load_history("user", "session", 16).unwrap().len(),
        8
    );
}
