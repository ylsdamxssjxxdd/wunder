use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;

pub(super) trait PostgresMetaStorage {
    fn get_meta_impl(&self, key: &str) -> Result<Option<String>>;
    fn set_meta_impl(&self, key: &str, value: &str) -> Result<()>;
    fn list_meta_prefix_impl(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn delete_meta_prefix_impl(&self, prefix: &str) -> Result<usize>;
}

impl PostgresMetaStorage for PostgresStorage {
    fn get_meta_impl(&self, key: &str) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_opt("SELECT value FROM meta WHERE key = $1", &[&key])?;
        Ok(row.map(|row| row.get::<_, String>(0)))
    }

    fn set_meta_impl(&self, key: &str, value: &str) -> Result<()> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO meta (key, value, updated_time) VALUES ($1, $2, $3) \
             ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_time = EXCLUDED.updated_time",
            &[&key, &value, &now],
        )?;
        Ok(())
    }

    fn list_meta_prefix_impl(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("{cleaned}%");
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT key, value FROM meta WHERE key LIKE $1 ORDER BY updated_time DESC, key ASC",
            &[&pattern],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
    }

    fn delete_meta_prefix_impl(&self, prefix: &str) -> Result<usize> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let pattern = format!("{cleaned}%");
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM meta WHERE key LIKE $1", &[&pattern])?;
        Ok(affected as usize)
    }
}
