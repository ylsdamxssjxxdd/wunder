//! Synthetic thread generation through the production canonical storage path.

use super::SqliteStorage;
use crate::storage::stress_model::{validate_spec, StressThreadSpec, StressThreadStats};
use crate::storage::WorkspaceStore;
use anyhow::Result;

impl SqliteStorage {
    /// Exercises the same policy, bounded writer and completed-history
    /// projector as a live thread. No relaxed SQLite durability or direct
    /// insertion into execution/history tables is used.
    pub fn generate_stress_thread(
        &self,
        user_id: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        validate_spec(spec)?;
        let store = self.canonical_threads()?.clone();
        let stats = crate::storage::canonical_thread_log::wait(store.generate_stress_thread(
            user_id,
            spec,
            move |done, items| progress(done, items),
        ))??;
        // Canonical history alone is not an openable thread: every session list
        // (蛹's work area, the thread APIs, restart discovery) reads the
        // `chat_sessions` catalog. Register that row only after the history
        // committed, so a failed generation never leaves a session that opens
        // onto nothing, and a completed one is reachable instead of merely
        // present on disk.
        let workspace_id = match spec
            .workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            Some(workspace_id) => Some(workspace_id.to_string()),
            // A generated thread must belong to a work area, otherwise the
            // workspace tree quite correctly hides it.
            None => self
                .list_workspaces(user_id)?
                .into_iter()
                .next()
                .map(|workspace| workspace.workspace_id),
        };
        let record = crate::services::stress_thread::stress_session_record(
            user_id,
            &stats.session_id,
            &spec.title,
            workspace_id.as_deref(),
        );
        crate::services::stress_thread::register_stress_session(self, &record)
            .map_err(|error| anyhow::anyhow!("压测线程已写入，但会话目录登记失败: {error}"))?;
        Ok(stats)
    }
}

#[cfg(all(test, feature = "sqlite-storage"))]
mod tests {
    use super::*;
    use crate::storage::stress_model::MAX_USER_ROUNDS;
    use crate::storage::ChatSessionStore;
    use rusqlite::Connection;
    use serde_json::Value;
    use std::io::{BufRead, BufReader};
    use wunder_core::{
        storage_backend::ThreadLogStore,
        thread_store::{ThreadId, ThreadMetadataStore},
        thread_view::{CHAT_PAGE_MAX, TRAJECTORY_PAGE_MAX},
    };

    fn fixture() -> (tempfile::TempDir, SqliteStorage) {
        let root = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(
            root.path()
                .join("state_1.sqlite")
                .to_string_lossy()
                .into_owned(),
        );
        (root, storage)
    }

    fn spec(user_rounds: i64, model_rounds_per_turn: i64) -> StressThreadSpec {
        StressThreadSpec {
            session_id: "sample-thread".into(),
            title: "Sample history".into(),
            workspace_id: Some("sample-workspace".into()),
            user_rounds,
            model_rounds_per_turn,
            seed: None,
            scenario: crate::storage::StressThreadScenario::Complex,
        }
    }

    #[test]
    fn generated_history_uses_canonical_files_and_normal_read_contracts() {
        let (root, storage) = fixture();
        let spec = spec(3, 4);
        let mut progress = Vec::new();
        let stats = storage
            .generate_stress_thread("sample-user", &spec, |done, items| {
                progress.push((done, items))
            })
            .unwrap();
        assert_eq!(stats.user_turns, 3);
        assert_eq!(stats.model_requests, 12);
        assert_eq!(stats.items, 3 + stats.model_requests + stats.tool_calls);
        assert_eq!(stats.model_actions, stats.model_requests + stats.tool_calls);
        assert_eq!(progress.len(), 3);
        assert_eq!(progress.last().copied(), Some((3, stats.items)));
        let storage_bytes = stats.storage_bytes.expect("bounded storage snapshot");
        assert!(storage_bytes.rollout_raw_bytes > 0);
        assert_eq!(storage_bytes.rollout_zstd_bytes, 0);
        assert!(storage_bytes.state_bytes > 0);
        assert!(storage_bytes.queue_bytes > 0);
        assert!(storage_bytes.thread_history_bytes > 0);
        assert_eq!(
            storage_bytes.total_bytes,
            storage_bytes.rollout_raw_bytes
                + storage_bytes.rollout_zstd_bytes
                + storage_bytes.state_bytes
                + storage_bytes.state_wal_bytes
                + storage_bytes.queue_bytes
                + storage_bytes.queue_wal_bytes
                + storage_bytes.thread_history_bytes
                + storage_bytes.thread_history_wal_bytes
        );

        let history = Connection::open(root.path().join("thread_history_1.sqlite")).unwrap();
        assert_eq!(
            history
                .query_row("SELECT COUNT(*) FROM thread_items", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            stats.items,
        );
        assert_eq!(
            history
                .query_row(
                    "SELECT COUNT(*) FROM thread_turns WHERE status='completed'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            stats.user_turns,
        );
        let state = Connection::open(root.path().join("state_1.sqlite")).unwrap();
        assert_eq!(
            state.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('thread_logs','thread_items','thread_log_changes','tool_logs')", [], |r| r.get::<_, i64>(0)).unwrap(),
            0,
        );
        let queue = Connection::open(root.path().join("queue_1.sqlite")).unwrap();
        assert_eq!(
            queue.query_row("SELECT COUNT(*) FROM canonical_turn_admissions WHERE status='completed' AND input_json IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(),
            stats.user_turns,
        );
        assert!(!root.path().join("logs_1.sqlite").exists());

        let paths = walkdir::WalkDir::new(root.path().join("sessions"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "jsonl")
            })
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        let reader = BufReader::new(std::fs::File::open(&paths[0]).unwrap());
        let mut records = 0_u64;
        let mut usage_records = Vec::new();
        for line in reader.lines() {
            let record: Value = serde_json::from_str(&line.unwrap()).unwrap();
            assert_eq!(record["ordinal"].as_u64(), Some(records));
            assert!(!record.to_string().contains("thread_item_tail"));
            if record["type"] == "usage" {
                usage_records.push(record["payload"].clone());
            }
            records += 1;
        }
        assert_eq!(records, stats.rollout_records);
        assert_eq!(usage_records.len() as i64, stats.model_requests);
        assert!(usage_records.iter().all(|usage| {
            usage["input_tokens"].as_u64().unwrap_or_default() > 0
                && usage["output_tokens"].as_u64().unwrap_or_default() > 0
                && usage["metadata"]["estimated"] == Value::Bool(true)
                && usage["metadata"]["profile"] == "storage-stress-v1"
                && usage["metadata"]["request_token_breakdown"]["estimated_input_tokens"]
                    == usage["input_tokens"]
        }));
        let thread_usage = usage_records
            .iter()
            .map(|usage| &usage["metadata"]["thread_usage"])
            .collect::<Vec<_>>();
        assert!(thread_usage.iter().all(|usage| {
            usage["input_tokens"].as_u64().unwrap_or_default() > 0
                && usage["output_tokens"].as_u64().unwrap_or_default() > 0
                && usage["total_tokens"].as_u64().unwrap_or_default()
                    >= usage["input_tokens"].as_u64().unwrap_or_default()
                        + usage["output_tokens"].as_u64().unwrap_or_default()
        }));
        assert!(thread_usage.windows(2).all(|pair| {
            pair[1]["input_tokens"].as_u64() >= pair[0]["input_tokens"].as_u64()
                && pair[1]["output_tokens"].as_u64() >= pair[0]["output_tokens"].as_u64()
                && pair[1]["total_tokens"].as_u64() >= pair[0]["total_tokens"].as_u64()
        }));

        let visible = storage
            .list_thread_visible_messages("sample-user", &spec.session_id, None, 200)
            .unwrap();
        assert_eq!(visible.len(), 15);
        let usage = crate::storage::runtime_logs::projected_chat_stats(root.path()).unwrap();
        assert_eq!(usage["sample-user"]["chat_records"], visible.len() as i64);
        assert_eq!(
            visible.iter().filter(|row| row["role"] == "user").count(),
            3
        );
        assert!(visible.iter().all(|row| row["status"] == "completed"));
        let assistant_stats = visible
            .iter()
            .filter(|row| row["role"] == "assistant")
            .map(|row| &row["meta"]["message_stats"])
            .collect::<Vec<_>>();
        assert_eq!(assistant_stats.len() as i64, stats.model_requests);
        assert!(assistant_stats.iter().all(|message_stats| {
            message_stats["usage"]["estimated"] == Value::Bool(true)
                && message_stats["usage"]["input_tokens"]
                    == message_stats["context_occupancy_tokens"]
                && message_stats["round_usage"]["total_tokens"].as_u64()
                    >= message_stats["usage"]["total_tokens"].as_u64()
        }));
        let turns = storage
            .list_thread_turns("sample-user", &spec.session_id, None, 50)
            .unwrap();
        assert_eq!(turns.len(), 3);
        assert!(turns.iter().all(|turn| turn["status"] == "completed"));
    }

    #[test]
    fn generated_history_preserves_metadata_and_pages_every_completed_item() {
        let (root, storage) = fixture();
        let spec = spec(25, 2);
        let stats = storage
            .generate_stress_thread("sample-user", &spec, |_, _| {})
            .unwrap();
        let store = storage.canonical_threads().unwrap();
        let metadata = crate::storage::canonical_thread_log::wait(
            store.get_thread(&ThreadId::new(spec.session_id.clone()).unwrap()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(metadata.workspace_id, spec.workspace_id);

        let mut before = None;
        let mut rounds = Vec::new();
        loop {
            let page = storage
                .thread_view_page("sample-user", &spec.session_id, "chat", before, usize::MAX)
                .unwrap();
            assert_eq!(page.projection_state, "ready");
            assert!(page.chat.len() <= CHAT_PAGE_MAX);
            let page_rounds = page
                .chat
                .iter()
                .map(|row| row.user_round)
                .collect::<Vec<_>>();
            rounds.splice(0..0, page_rounds);
            if !page.has_more {
                break;
            }
            assert!(page.next_before.is_some());
            assert_ne!(page.next_before, before);
            before = page.next_before;
        }
        assert_eq!(rounds, (1..=25).collect::<Vec<_>>());

        let mut before = None;
        let mut item_ids = std::collections::HashSet::new();
        loop {
            let page = storage
                .thread_view_page(
                    "sample-user",
                    &spec.session_id,
                    "trajectory",
                    before,
                    usize::MAX,
                )
                .unwrap();
            assert!(page.trajectory.len() <= TRAJECTORY_PAGE_MAX);
            for row in page.trajectory {
                assert!(
                    item_ids.insert(row.item_id),
                    "completed items must not repeat between pages"
                );
            }
            if !page.has_more {
                break;
            }
            assert!(page.next_before.is_some());
            assert_ne!(page.next_before, before);
            before = page.next_before;
        }
        assert_eq!(item_ids.len() as i64, stats.items);
        assert!(!root.path().join("logs_1.sqlite").exists());
    }

    #[test]
    fn oversized_requests_are_rejected_before_creating_storage() {
        let (root, storage) = fixture();
        let spec = spec(MAX_USER_ROUNDS + 1, 4);
        assert!(storage
            .generate_stress_thread("sample-user", &spec, |_, _| {})
            .is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /// A generated thread must be reachable, not just present: the work area
    /// lists `chat_sessions`, so the catalog row is what makes it openable.
    #[test]
    fn a_generated_thread_is_registered_in_its_work_area() {
        let (_root, storage) = fixture();
        let spec = spec(2, 2);
        let stats = storage
            .generate_stress_thread("sample-user", &spec, |_, _| {})
            .unwrap();
        let (sessions, total) = storage
            .list_chat_sessions_by_workspace(
                "sample-user",
                "sample-workspace",
                Some("active"),
                0,
                10,
            )
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(sessions[0].session_id, stats.session_id);
        assert_eq!(sessions[0].title, spec.title);
        assert_eq!(
            sessions[0].workspace_id.as_deref(),
            Some("sample-workspace"),
            "the thread belongs to the work area that asked for it"
        );
    }

    /// A caller without a work area (舵机, or a settings page opened before the
    /// workspace selector projected its first frame) still yields a thread the
    /// work area can show, instead of a workspace-less row nothing lists.
    #[test]
    fn a_thread_without_a_work_area_lands_in_the_first_one() {
        let (_root, storage) = fixture();
        let now = 1_700_000_000.0;
        storage
            .upsert_workspace(&wunder_core::storage_records::WorkspaceRecord {
                workspace_id: "fallback-workspace".into(),
                user_id: "sample-user".into(),
                name: "Sample work area".into(),
                root_path: "D:\\sample-workspace".into(),
                icon: String::new(),
                color: String::new(),
                sort_index: 0,
                created_at: now,
                updated_at: now,
            })
            .unwrap();
        let mut spec = spec(1, 1);
        spec.workspace_id = None;
        let stats = storage
            .generate_stress_thread("sample-user", &spec, |_, _| {})
            .unwrap();
        let (sessions, _) = storage
            .list_chat_sessions_by_workspace(
                "sample-user",
                "fallback-workspace",
                Some("active"),
                0,
                10,
            )
            .unwrap();
        assert_eq!(
            sessions
                .iter()
                .filter(|session| session.session_id == stats.session_id)
                .count(),
            1
        );
    }
}
