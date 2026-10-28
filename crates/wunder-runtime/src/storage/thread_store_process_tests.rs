use super::*;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

fn create_request(thread: &ThreadId, rollout: RolloutId) -> CreateThreadRequest {
    CreateThreadRequest {
        metadata: ThreadMetadata {
            thread_id: thread.clone(),
            current_rollout_id: rollout.clone(),
            status: ThreadStatus::Active,
            created_at_ms: 0,
            updated_at_ms: 0,
            title: None,
            workspace_id: None,
            pinned: false,
        },
        session_meta: SessionMeta {
            thread_id: thread.clone(),
            rollout_id: rollout,
            created_at_ms: 0,
            source: "sample".into(),
            initial_system_prompt: "sample-system".into(),
            initial_memory: None,
            parent_thread_id: None,
            history_base: None,
            capabilities: serde_json::json!({}),
        },
    }
}

fn record() -> CanonicalRecord {
    CanonicalRecord::Event(CanonicalEvent::SettingsApplied(
        serde_json::json!({"sample":true}),
    ))
}

struct FixtureChild(Child);

impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_fixture(root: &Path, worker: &str, mode: &str) -> FixtureChild {
    FixtureChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage::thread_store::process_tests::local_process_fixture",
                "--nocapture",
            ])
            .env("WUNDER_STORE_TEST_ROOT", root)
            .env("WUNDER_STORE_TEST_WORKER", worker)
            .env("WUNDER_STORE_TEST_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

fn wait_ready(child: &mut FixtureChild, root: &Path, worker: &str) {
    let started = Instant::now();
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "storage child exited before readiness"
        );
        if root.join(format!("{worker}.ready")).exists() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "storage child readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn finish(child: &mut FixtureChild) {
    let started = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "storage child failed: {status}");
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "storage child exit timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn local_process_fixture() {
    let Some(root) = std::env::var_os("WUNDER_STORE_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let worker = std::env::var("WUNDER_STORE_TEST_WORKER").unwrap();
    let mode = std::env::var("WUNDER_STORE_TEST_MODE").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let store = LocalThreadStore::open(&root).unwrap();
        let thread = ThreadId::new("sample-shared-thread").unwrap();
        if mode == "independent" || mode == "crash" {
            let thread = ThreadId::new(format!("sample-{worker}")).unwrap();
            let rollout = RolloutId::new(format!("sample-rollout-{worker}")).unwrap();
            store
                .create_thread(create_request(&thread, rollout))
                .await
                .unwrap();
            store
                .append_canonical(&thread, vec![record()])
                .await
                .unwrap();
            fs::write(root.join(format!("{worker}.ready")), []).unwrap();
        } else {
            assert!(store
                .append_canonical(&thread, vec![record()])
                .await
                .is_err());
            fs::write(root.join(format!("{worker}.ready")), []).unwrap();
        }
        let started = Instant::now();
        while !root.join("release").exists() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "parent did not release child"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if mode == "handoff" {
            store
                .append_canonical(&thread, vec![record()])
                .await
                .unwrap();
            store.flush(&thread).await.unwrap();
            store.shutdown(&thread).await.unwrap();
        } else {
            store
                .shutdown(&ThreadId::new(format!("sample-{worker}")).unwrap())
                .await
                .unwrap();
        }
    });
}

#[test]
fn multiple_local_processes_share_root_without_sharing_threads() {
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let desktop = LocalThreadStore::open(root.path()).unwrap();
    let mut workers = (0..3)
        .map(|index| {
            let worker = format!("worker-{index}");
            (
                worker.clone(),
                spawn_fixture(root.path(), &worker, "independent"),
            )
        })
        .collect::<Vec<_>>();
    for (worker, child) in &mut workers {
        wait_ready(child, root.path(), worker);
    }
    runtime.block_on(async {
        for (worker, _) in &workers {
            let thread = ThreadId::new(format!("sample-{worker}")).unwrap();
            assert_eq!(
                desktop
                    .session_meta(&thread)
                    .await
                    .unwrap()
                    .initial_system_prompt,
                "sample-system"
            );
            assert!(
                desktop
                    .history_position(&thread)
                    .await
                    .unwrap()
                    .end_ordinal_exclusive
                    >= 2
            );
        }
    });
    fs::write(root.path().join("release"), []).unwrap();
    for (_, child) in &mut workers {
        finish(child);
    }
}

#[test]
fn another_process_cannot_write_active_thread_but_can_take_over_after_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let desktop = LocalThreadStore::open(root.path()).unwrap();
    let thread = ThreadId::new("sample-shared-thread").unwrap();
    runtime.block_on(async {
        desktop
            .create_thread(create_request(
                &thread,
                RolloutId::new("sample-shared-rollout").unwrap(),
            ))
            .await
            .unwrap();
        desktop
            .append_canonical(&thread, vec![record()])
            .await
            .unwrap();
    });
    let mut child = spawn_fixture(root.path(), "handoff", "handoff");
    wait_ready(&mut child, root.path(), "handoff");
    runtime.block_on(desktop.shutdown(&thread)).unwrap();
    fs::write(root.path().join("release"), []).unwrap();
    finish(&mut child);
    runtime.block_on(async {
        assert_eq!(
            desktop
                .history_position(&thread)
                .await
                .unwrap()
                .end_ordinal_exclusive,
            3
        );
        assert_eq!(
            desktop
                .session_meta(&thread)
                .await
                .unwrap()
                .initial_system_prompt,
            "sample-system"
        );
    });
}

#[test]
fn local_process_exit_releases_thread_without_removing_persistent_lock_markers() {
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let desktop = LocalThreadStore::open(root.path()).unwrap();
    let mut child = spawn_fixture(root.path(), "crash", "crash");
    wait_ready(&mut child, root.path(), "crash");
    let thread = ThreadId::new("sample-crash").unwrap();
    runtime.block_on(async {
        assert!(desktop
            .append_canonical(&thread, vec![record()])
            .await
            .is_err());
    });
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    runtime.block_on(async {
        desktop
            .append_canonical(&thread, vec![record()])
            .await
            .unwrap();
        desktop.shutdown(&thread).await.unwrap();
        assert_eq!(
            desktop
                .history_position(&thread)
                .await
                .unwrap()
                .end_ordinal_exclusive,
            3
        );
        assert_eq!(
            desktop
                .session_meta(&thread)
                .await
                .unwrap()
                .initial_system_prompt,
            "sample-system"
        );
    });
    assert!(root.path().join(".canonical-store.lock").exists());
}
