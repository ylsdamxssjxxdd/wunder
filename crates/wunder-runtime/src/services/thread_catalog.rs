//! Shared, bounded thread directory used by local and remote clients.

use crate::core::blocking;
use crate::services::chat_runtime_projection::load_chat_session_activity;
use crate::state::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Working,
    NeedsYou,
    Ready,
    Failed,
    Finished,
}

impl ThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsYou => "needs_you",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ThreadListQuery {
    pub user_id: String,
    pub offset: i64,
    pub limit: i64,
    pub search: Option<String>,
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadSnapshot {
    pub session_id: String,
    pub title: String,
    pub status: ThreadStatus,
    pub agent_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub spawn_label: Option<String>,
    pub spawned_by: Option<String>,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub monitor: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadPage {
    pub items: Vec<ThreadSnapshot>,
    pub offset: i64,
    pub limit: i64,
    pub total: i64,
}

#[derive(Clone)]
pub struct ThreadCatalogService {
    state: AppState,
}

impl ThreadCatalogService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn list(&self, query: ThreadListQuery) -> Result<ThreadPage> {
        let user_id = query.user_id.trim().to_string();
        let offset = query.offset.max(0);
        let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
        let search = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let parent = query
            .parent_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let store = self.state.user_store.clone();
        let records = blocking::run_db("thread_catalog.list", move || {
            store.list_chat_sessions(&user_id, None, parent.as_deref(), offset, limit)
        })
        .await?;
        let (records, total) = records;
        let mut snapshots = Vec::with_capacity(records.len());
        for record in records {
            let searchable = format!(
                "{} {} {} {}",
                record.session_id,
                record.title,
                record.agent_id.as_deref().unwrap_or_default(),
                record.spawn_label.as_deref().unwrap_or_default()
            )
            .to_ascii_lowercase();
            if search
                .as_deref()
                .is_some_and(|needle| !searchable.contains(needle))
            {
                continue;
            }
            let monitor = self.state.monitor.get_record(&record.session_id);
            let activity =
                load_chat_session_activity(&self.state, &record.session_id, monitor.as_ref()).await;
            let status = normalize_status(&record.status, monitor.as_ref(), activity.running);
            snapshots.push(ThreadSnapshot {
                session_id: record.session_id,
                title: record.title,
                status,
                agent_id: record.agent_id,
                parent_session_id: record.parent_session_id,
                spawn_label: record.spawn_label,
                spawned_by: record.spawned_by,
                updated_at: record.updated_at,
                last_message_at: record.last_message_at,
                monitor,
            });
        }
        snapshots.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        let items = snapshots;
        Ok(ThreadPage {
            items,
            offset,
            limit,
            total,
        })
    }

    pub async fn snapshot(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ThreadSnapshot>> {
        let page = self
            .list(ThreadListQuery {
                user_id: user_id.to_string(),
                search: Some(session_id.to_string()),
                limit: 100,
                ..Default::default()
            })
            .await?;
        Ok(page
            .items
            .into_iter()
            .find(|item| item.session_id == session_id))
    }
}

fn normalize_status(record_status: &str, monitor: Option<&Value>, running: bool) -> ThreadStatus {
    let monitor_status = monitor
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(monitor_status, "waiting")
        || matches!(
            record_status,
            "waiting_input" | "needs_input" | "waiting_approval"
        )
    {
        return ThreadStatus::NeedsYou;
    }
    if running || matches!(monitor_status, "running" | "queued" | "cancelling") {
        return ThreadStatus::Working;
    }
    if matches!(record_status, "failed" | "error") || matches!(monitor_status, "error") {
        return ThreadStatus::Failed;
    }
    if matches!(record_status, "ready" | "idle" | "paused") {
        return ThreadStatus::Ready;
    }
    ThreadStatus::Finished
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_priority_prefers_needs_you_over_running() {
        assert_eq!(
            normalize_status("running", Some(&json!({"status":"waiting"})), true),
            ThreadStatus::NeedsYou
        );
    }

    #[test]
    fn status_maps_terminal_and_ready_states() {
        assert_eq!(
            normalize_status("failed", None, false),
            ThreadStatus::Failed
        );
        assert_eq!(normalize_status("idle", None, false), ThreadStatus::Ready);
        assert_eq!(
            normalize_status("completed", None, false),
            ThreadStatus::Finished
        );
    }
}
