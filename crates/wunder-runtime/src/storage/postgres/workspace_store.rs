use super::PostgresStorage;
use crate::storage::{StorageLifecycle, WorkspaceRecord};
use anyhow::Result;

pub(super) trait PostgresWorkspaceStorage {
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

impl PostgresWorkspaceStorage for PostgresStorage {
    fn upsert_workspace_impl(&self, record: &WorkspaceRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO workspaces (workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT(workspace_id) DO UPDATE SET name = EXCLUDED.name, root_path = EXCLUDED.root_path, \
             icon = EXCLUDED.icon, color = EXCLUDED.color, sort_index = EXCLUDED.sort_index, \
             updated_at = EXCLUDED.updated_at",
            &[
                &record.workspace_id,
                &record.user_id,
                &record.name,
                &record.root_path,
                &record.icon,
                &record.color,
                &record.sort_index,
                &record.created_at,
                &record.updated_at,
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
        let mut conn = self.conn()?;
        let row = conn
            .query_opt(
                "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
                 FROM workspaces WHERE user_id = $1 AND workspace_id = $2",
                &[&cleaned_user, &cleaned_id],
            )?
            .map(|row| map_workspace_row(&row));
        Ok(row)
    }

    fn list_workspaces_impl(&self, user_id: &str) -> Result<Vec<WorkspaceRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
             FROM workspaces WHERE user_id = $1 ORDER BY sort_index ASC, created_at ASC, workspace_id ASC",
            &[&cleaned_user],
        )?;
        Ok(rows.iter().map(map_workspace_row).collect())
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
        let mut conn = self.conn()?;
        let row = conn
            .query_opt(
                "SELECT workspace_id, user_id, name, root_path, icon, color, sort_index, created_at, updated_at \
                 FROM workspaces WHERE user_id = $1 AND root_path = $2",
                &[&cleaned_user, &cleaned_root],
            )?
            .map(|row| map_workspace_row(&row));
        Ok(row)
    }

    fn next_workspace_sort_index_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT MAX(sort_index) FROM workspaces WHERE user_id = $1",
            &[&cleaned_user],
        )?;
        let max: Option<i64> = row.and_then(|row| row.get(0));
        Ok(max.unwrap_or(-1) + 1)
    }

    fn delete_workspace_impl(&self, user_id: &str, workspace_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_id = workspace_id.trim();
        if cleaned_user.is_empty() || cleaned_id.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM workspaces WHERE user_id = $1 AND workspace_id = $2",
            &[&cleaned_user, &cleaned_id],
        )?;
        Ok(affected as i64)
    }
}

fn map_workspace_row(row: &tokio_postgres::Row) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: row.get(0),
        user_id: row.get(1),
        name: row.get(2),
        root_path: row.get(3),
        icon: row.get(4),
        color: row.get(5),
        sort_index: row.get(6),
        created_at: row.get(7),
        updated_at: row.get(8),
    }
}
