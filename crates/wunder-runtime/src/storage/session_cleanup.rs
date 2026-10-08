// Shared by PostgreSQL, SQLite and the explicit legacy repair script. A missing
// monitor record alone is never proof of deletion: unused drafts have none.
pub(crate) const EMPTY_HISTORY_PREDICATE: &str = include_str!("session_cleanup_empty.sql");

pub(crate) const LIVE_SESSION_PREDICATE: &str = include_str!("session_cleanup_live.sql");

pub(crate) const CATALOG_DEPENDENT_TABLES: &[&str] = &["session_goals"];

#[cfg(test)]
mod tests;
