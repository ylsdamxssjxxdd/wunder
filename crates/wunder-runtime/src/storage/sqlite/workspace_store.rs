use super::SqliteStorage;
use crate::storage::{StorageLifecycle, WorkspaceRecord};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub(super) trait SqliteWorkspaceStorage {
    fn upsert_workspace_impl(&self, record: &WorkspaceRecord) -> Result<()>;
    fn get_workspace_impl(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceRecord>>;
    fn list_workspaces_impl(&self, user_id: &str) -> Result<Vec<WorkspaceRecord>>;
    fn find_workspace_by_root_impl(
        &self,
        user_id: &str,
        root_path: &str,
    ) -> Result<Option<WorkspaceRecord>>;
    fn next_workspace_sort_index_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_workspace_impl(&self, user_id: &str, workspace_id: &str) -> Result<i64>;
}

impl SqliteWorkspaceStorage for SqliteStorage {
    fn upsert_workspace_impl(&self, record: &WorkspaceRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO workspaces (workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(workspace_id) DO UPDATE SET name = excluded.name, root_path = excluded.root_path, \
             icon = excluded.icon, color = excluded.color, sort_index = excluded.sort_index, \
             updated_at = excluded.updated_at",
            params![
                record.workspace_id,
                record.user_id,
                record.name,
                record.root_path,
                record.icon,
                record.color,
                record.sort_index,
                record.created_at,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_workspace_impl(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_id = workspace_id.trim();
        if cleaned_user.is_empty() || cleaned_id.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
                 FROM workspaces WHERE user_id = ? AND workspace_id = ?",
                params![cleaned_user, cleaned_id],
                map_workspace_row,
            )
            .optional()?;
        Ok(record)
    }

    fn list_workspaces_impl(&self, user_id: &str) -> Result<Vec<WorkspaceRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
             FROM workspaces WHERE user_id = ? ORDER BY sort_index ASC, created_at ASC, workspace_id ASC",
        )?;
        let rows = stmt.query_map([cleaned_user], map_workspace_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    fn find_workspace_by_root_impl(
        &self,
        user_id: &str,
        root_path: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_root = root_path.trim();
        if cleaned_user.is_empty() || cleaned_root.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
                 FROM workspaces WHERE user_id = ? AND root_path = ?",
                params![cleaned_user, cleaned_root],
                map_workspace_row,
            )
            .optional()?;
        Ok(record)
    }

    fn next_workspace_sort_index_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(sort_index) FROM workspaces WHERE user_id = ?",
                params![cleaned_user],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(max.unwrap_or(-1) + 1)
    }

    fn delete_workspace_impl(&self, user_id: &str, workspace_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_id = workspace_id.trim();
        if cleaned_user.is_empty() || cleaned_id.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM workspaces WHERE user_id = ? AND workspace_id = ?",
            params![cleaned_user, cleaned_id],
        )?;
        Ok(affected as i64)
    }
}

fn map_workspace_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    Ok(WorkspaceRecord {
        workspace_id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        root_path: row.get(3)?,
        icon: row.get(4)?,
        color: row.get(5)?,
        sort_index: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::SqliteStorage;
    use tempfile::tempdir;

    fn build_storage() -> SqliteStorage {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("workspace-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        std::mem::forget(dir);
        storage
    }

    fn record(user: &str, id: &str, root: &str, sort: i64) -> WorkspaceRecord {
        WorkspaceRecord {
            workspace_id: id.to_string(),
            user_id: user.to_string(),
            name: format!("ws-{id}"),
            root_path: root.to_string(),
            icon: "folder".to_string(),
            color: "blue".to_string(),
            sort_index: sort,
            created_at: 100.0,
            updated_at: 100.0,
        }
    }

    #[test]
    fn workspace_crud_and_uniqueness_roundtrip() {
        let storage = build_storage();
        storage
            .upsert_workspace_impl(&record("u1", "w1", "D:/proj/a", 0))
            .expect("insert w1");
        storage
            .upsert_workspace_impl(&record("u1", "w2", "D:/proj/b", 1))
            .expect("insert w2");

        let listed = storage.list_workspaces_impl("u1").expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].workspace_id, "w1");

        assert_eq!(
            storage.next_workspace_sort_index_impl("u1").expect("next"),
            2
        );

        // Same folder for the same user is rejected by the unique index.
        let duplicate = record("u1", "w3", "D:/proj/a", 2);
        assert!(storage.upsert_workspace_impl(&duplicate).is_err());

        // The same folder under another user stays allowed.
        storage
            .upsert_workspace_impl(&record("u2", "w4", "D:/proj/a", 0))
            .expect("other user same root");

        let found = storage
            .find_workspace_by_root_impl("u1", "D:/proj/b")
            .expect("find")
            .expect("w2 by root");
        assert_eq!(found.workspace_id, "w2");

        // Update in place keeps the identity stable.
        let mut edited = record("u1", "w1", "D:/proj/a", 0);
        edited.name = "renamed".to_string();
        edited.updated_at = 200.0;
        storage.upsert_workspace_impl(&edited).expect("update");
        let reloaded = storage
            .get_workspace_impl("u1", "w1")
            .expect("get")
            .expect("w1 exists");
        assert_eq!(reloaded.name, "renamed");
        assert_eq!(reloaded.updated_at, 200.0);

        assert_eq!(
            storage.delete_workspace_impl("u1", "w1").expect("delete"),
            1
        );
        assert!(storage
            .get_workspace_impl("u1", "w1")
            .expect("get")
            .is_none());
    }
}
