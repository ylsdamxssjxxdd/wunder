//! Independent, lazily opened runtime databases. No permanent ATTACH and no
//! background maintenance loop; each role retains its own bounded read pool.
use super::{read_connection::ReadConnection, SqliteStorage};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DatabaseRole {
    State,
    Queue,
    Goals,
    Memories,
    Logs,
}

impl SqliteStorage {
    /// Explicit administration queries may read related role databases. These
    /// attachments are request-scoped and never enter an idle connection pool.
    pub(super) fn attach_state_and_queue(&self, connection: &Connection) -> Result<()> {
        self.ensure_initialized()?;
        let queue = self.runtime_database(DatabaseRole::Queue);
        queue.ensure_initialized()?;
        connection.execute(
            "ATTACH DATABASE ?1 AS control_state",
            [self.db_path.to_string_lossy().as_ref()],
        )?;
        connection.execute(
            "ATTACH DATABASE ?1 AS runtime_queue",
            [queue.db_path.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    pub(super) fn projected_chat_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, i64>>> {
        super::super::runtime_logs::projected_chat_stats(self.canonical_threads()?.root())
    }

    fn runtime_database(&self, role: DatabaseRole) -> &Self {
        if self.database_role == role {
            return self;
        }
        let (index, filename) = match role {
            DatabaseRole::Queue => (0, "queue_1.sqlite"),
            DatabaseRole::Goals => (1, "goals_1.sqlite"),
            DatabaseRole::Memories => (2, "memories_1.sqlite"),
            DatabaseRole::Logs => (3, "logs_1.sqlite"),
            DatabaseRole::State => return self,
        };
        self.runtime_databases[index].get_or_init(|| {
            let root = self
                .db_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            Box::new(Self::with_role(root.join(filename), role))
        })
    }

    pub(super) fn open_queue(&self) -> Result<Connection> {
        self.open_runtime(DatabaseRole::Queue)
    }

    pub(super) fn read_queue(&self) -> Result<ReadConnection<'_>> {
        let storage = self.runtime_database(DatabaseRole::Queue);
        storage.ensure_initialized()?;
        storage.read_connection()
    }

    pub(super) fn open_goals(&self) -> Result<Connection> {
        self.open_runtime(DatabaseRole::Goals)
    }

    pub(super) fn open_memories(&self) -> Result<Connection> {
        self.open_runtime(DatabaseRole::Memories)
    }

    pub(super) fn open_logs(&self) -> Result<Connection> {
        self.open_runtime(DatabaseRole::Logs)
    }

    pub(super) fn read_logs(&self) -> Result<ReadConnection<'_>> {
        let storage = self.runtime_database(DatabaseRole::Logs);
        storage.ensure_initialized()?;
        storage.read_connection()
    }

    fn open_runtime(&self, role: DatabaseRole) -> Result<Connection> {
        let storage = self.runtime_database(role);
        storage.ensure_initialized()?;
        storage.open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        AgentRuntimeStore, AgentTaskRecord, ConversationLogStore, MemoryRecordStore,
        SessionGoalRecord, SessionGoalStore,
    };
    use serde_json::json;
    use std::path::Path;

    fn has_table(path: &Path, table: &str) -> bool {
        let conn = Connection::open(path).unwrap();
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .unwrap()
    }

    #[test]
    fn roles_open_lazily_and_never_share_runtime_tables_with_state() {
        let root = tempfile::tempdir().unwrap();
        let state = SqliteStorage::new(root.path().join("state_1.sqlite").to_string_lossy().into());
        state.ensure_initialized().unwrap();
        assert!(root.path().join("state_1.sqlite").exists());
        for file in [
            "queue_1.sqlite",
            "goals_1.sqlite",
            "memories_1.sqlite",
            "logs_1.sqlite",
        ] {
            assert!(
                !root.path().join(file).exists(),
                "{file} opened before its role was used"
            );
        }

        state.set_memory_enabled("fixture-user", true).unwrap();
        state
            .upsert_session_goal(&SessionGoalRecord {
                session_id: "fixture-thread".into(),
                user_id: "fixture-user".into(),
                goal_id: "fixture-goal".into(),
                revision: 1,
                objective: "fixture objective".into(),
                phase: "active".into(),
                blocked_code: None,
                blocked_message: None,
                max_goal_rounds: 4,
                rounds_started: 0,
                created_at: 1.0,
                updated_at: 1.0,
            })
            .unwrap();
        state
            .insert_agent_task(&AgentTaskRecord {
                task_id: "fixture-task".into(),
                thread_id: "fixture-thread".into(),
                user_id: "fixture-user".into(),
                agent_id: String::new(),
                session_id: "fixture-thread".into(),
                status: "pending".into(),
                request_payload: json!({"question":"fixture"}),
                request_id: None,
                retry_count: 0,
                retry_at: 1.0,
                created_at: 1.0,
                updated_at: 1.0,
                started_at: None,
                finished_at: None,
                last_error: None,
            })
            .unwrap();
        state
            .append_tool_log(
                "fixture-user",
                &json!({"session_id":"fixture-thread","tool":"fixture","ok":true}),
            )
            .unwrap();

        let state_path = root.path().join("state_1.sqlite");
        assert!(!has_table(&state_path, "agent_tasks"));
        assert!(!has_table(&state_path, "session_goals"));
        assert!(!has_table(&state_path, "memory_settings"));
        assert!(!has_table(&state_path, "runtime_tool_metrics"));
        assert!(has_table(
            &root.path().join("queue_1.sqlite"),
            "agent_tasks"
        ));
        assert!(has_table(
            &root.path().join("goals_1.sqlite"),
            "session_goals"
        ));
        assert!(has_table(
            &root.path().join("memories_1.sqlite"),
            "memory_settings"
        ));
        assert!(has_table(
            &root.path().join("logs_1.sqlite"),
            "runtime_tool_metrics"
        ));
    }
}
