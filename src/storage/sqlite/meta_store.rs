use super::SqliteStorage;
use crate::storage::StorageBackend;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub(super) trait SqliteMetaStorage {
    fn get_meta_impl(&self, key: &str) -> Result<Option<String>>;
    fn set_meta_impl(&self, key: &str, value: &str) -> Result<()>;
    fn list_meta_prefix_impl(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn delete_meta_prefix_impl(&self, prefix: &str) -> Result<usize>;
}

impl SqliteMetaStorage for SqliteStorage {
    fn get_meta_impl(&self, key: &str) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    fn set_meta_impl(&self, key: &str, value: &str) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let now = Self::now_ts();
        conn.execute(
            "INSERT INTO meta (key, value, updated_time) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_time = excluded.updated_time",
            params![key, value, now],
        )?;
        Ok(())
    }

    fn list_meta_prefix_impl(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let pattern = format!("{cleaned}%");
        let mut stmt = conn.prepare(
            "SELECT key, value FROM meta WHERE key LIKE ? ORDER BY updated_time DESC, key ASC",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    fn delete_meta_prefix_impl(&self, prefix: &str) -> Result<usize> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let pattern = format!("{prefix}%");
        let affected = conn.execute("DELETE FROM meta WHERE key LIKE ?", params![pattern])?;
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::StorageBackend;
    use tempfile::tempdir;

    #[test]
    fn meta_store_lists_updates_and_deletes_by_prefix() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("meta-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");

        storage.set_meta("scope/a", "one").expect("set first");
        storage.set_meta("scope/b", "two").expect("set second");
        storage.set_meta("other/a", "skip").expect("set other");
        storage
            .set_meta("scope/a", "updated")
            .expect("update first");

        assert_eq!(
            storage.get_meta("scope/a").expect("get meta").as_deref(),
            Some("updated")
        );
        assert_eq!(
            storage
                .list_meta_prefix("scope/")
                .expect("list prefix")
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>(),
            vec!["scope/a=updated", "scope/b=two"]
        );
        assert_eq!(
            storage.delete_meta_prefix("scope/").expect("delete prefix"),
            2
        );
        assert!(storage
            .list_meta_prefix("scope/")
            .expect("list deleted prefix")
            .is_empty());
        assert_eq!(
            storage.get_meta("other/a").expect("get other").as_deref(),
            Some("skip")
        );
    }
}
