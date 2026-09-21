use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn sqlite_config(dir: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = dir.join("storage.db").to_string_lossy().into_owned();
    config
}

async fn concurrent_file_tools(config: Config, root: PathBuf) {
    let runtime = Arc::new(FileRuntime::default());
    let contexts = futures::future::join_all((0..24).map(|index| {
        let config = config.clone();
        let root = root.clone();
        let runtime = Arc::clone(&runtime);
        async move {
            let workspace_id = format!("user_{}__c__{}", index / 3, index % 3 + 1);
            let workspace_root = root.join(&workspace_id);
            let context = SandboxContext {
                workspace_root: workspace_root.clone(),
                container_root: root.clone(),
                allow_commands: Arc::default(),
            };
            let request = serde_json::from_value(json!({
                "user_id": format!("user_{}", index / 3), "session_id": format!("session_{index}"),
                "tool": "写入文件",
            }))
            .unwrap();
            let args = json!({ "path": "value.txt", "content": format!("{index}") });
            let result =
                execute_with_runtime(&runtime, config.clone(), &request, &context, &args).await;
            assert!(result.ok, "{}: {}", result.error, result.data);
            assert_eq!(
                tokio::fs::read_to_string(workspace_root.join("value.txt"))
                    .await
                    .unwrap(),
                format!("{index}")
            );
            let key = ContextKey {
                container_root: root,
                workspace_root,
                workspace_id,
            };
            let first = runtime.context(key.clone(), config.clone()).await.unwrap();
            let second = runtime.context(key.clone(), config).await.unwrap();
            assert!(Arc::ptr_eq(&first, &second));
            assert_eq!(
                first.workspace.workspace_root(&key.workspace_id),
                key.workspace_root
            );
            first
        }
    }))
    .await;
    for context in &contexts {
        assert!(Arc::ptr_eq(&contexts[0].storage, &context.storage));
    }
    assert_eq!(runtime.contexts.lock().len(), 24);
}

#[tokio::test]
async fn sqlite_cold_start_is_shared_and_user_container_paths_stay_isolated() {
    let dir = tempfile::tempdir().unwrap();
    concurrent_file_tools(sqlite_config(dir.path()), dir.path().join("workspaces")).await;
}

#[cfg(feature = "postgres-storage")]
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in WUNDER_TEST_SANDBOX_POSTGRES_DSN"]
async fn postgres_cold_start_is_shared_and_user_container_paths_stay_isolated() {
    let mut config = Config::default();
    config.storage.backend = "postgres".to_string();
    config.storage.postgres.dsn = std::env::var("WUNDER_TEST_SANDBOX_POSTGRES_DSN").unwrap();
    let dir = tempfile::tempdir().unwrap();
    concurrent_file_tools(config, dir.path().join("workspaces")).await;
}

#[tokio::test]
async fn canceled_waiter_does_not_duplicate_in_progress_initialization() {
    let cache = Arc::new(SandboxStorage::default());
    let builds = Arc::new(AtomicUsize::new(0));
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::build_storage(&sqlite_config(dir.path()).storage).unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let first = {
        let cache = Arc::clone(&cache);
        let builds = Arc::clone(&builds);
        let storage = Arc::clone(&storage);
        tokio::task::spawn_blocking(move || {
            cache.initialize(|| {
                builds.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(storage)
            })
        })
    };
    started_rx.await.unwrap();
    // Aborting a waiter cannot stop an already running blocking job.
    first.abort();
    let mut waiters = Vec::new();
    for _ in 0..8 {
        let cache = Arc::clone(&cache);
        let builds = Arc::clone(&builds);
        let storage = Arc::clone(&storage);
        waiters.push(tokio::task::spawn_blocking(move || {
            cache.initialize(|| {
                builds.fetch_add(1, Ordering::SeqCst);
                Ok(storage)
            })
        }));
    }
    release_tx.send(()).unwrap();
    for waiter in waiters {
        assert!(Arc::ptr_eq(&storage, &waiter.await.unwrap().unwrap()));
    }
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_initialization_is_throttled_but_can_recover() {
    let cache = SandboxStorage::default();
    assert!(cache
        .initialize(|| Err(anyhow::anyhow!("unavailable")))
        .is_err());
    assert!(cache
        .initialize(|| panic!("must not retry during cooldown"))
        .is_err());
    cache.initialization.lock().as_mut().unwrap().0 -= STORAGE_RETRY_DELAY;
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::build_storage(&sqlite_config(dir.path()).storage).unwrap();
    let result = cache.initialize(|| Ok(Arc::clone(&storage))).unwrap();
    assert!(Arc::ptr_eq(&result, &storage));
}

#[tokio::test]
async fn context_cache_is_bounded_and_expires_without_changing_active_roots() {
    let dir = tempfile::tempdir().unwrap();
    let config = sqlite_config(dir.path());
    let runtime = FileRuntime::default();
    let key = ContextKey {
        container_root: dir.path().to_path_buf(),
        workspace_root: dir.path().join("workspace"),
        workspace_id: "user__c__1".to_string(),
    };
    let active = runtime.context(key.clone(), config.clone()).await.unwrap();
    runtime.contexts.lock().get_mut(&key).unwrap().last_used -= CONTEXT_IDLE_TTL;
    let replacement = runtime.context(key.clone(), config.clone()).await.unwrap();
    assert!(!Arc::ptr_eq(&active, &replacement));
    for index in 0..CONTEXT_CAPACITY {
        let mut other = key.clone();
        other.workspace_root = dir.path().join(format!("workspace_{index}"));
        runtime.context(other, config.clone()).await.unwrap();
    }
    assert_eq!(runtime.contexts.lock().len(), CONTEXT_CAPACITY);
    assert_eq!(
        active.workspace.workspace_root(&key.workspace_id),
        key.workspace_root
    );
}
