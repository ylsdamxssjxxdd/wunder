//! Shared, bounded thread directory used by local and remote clients.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::blocking;
use crate::services::chat_runtime_projection::load_chat_session_activity;
use crate::services::user_store::UserStore;
use crate::state::AppState;
use crate::storage::ChatSessionRecord;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_PAGE_SIZE: i64 = 100;
/// Hard bound on records scanned for a keyword search: 10 store pages of 100.
/// Search runs at the service layer over whole pages, so matches beyond this
/// cap are not returned; the cap keeps catalog latency bounded.
const MAX_SEARCH_SCAN_PAGES: usize = 10;
const STORE_PAGE_SIZE: i64 = 100;

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
    /// Number of child threads whose parent is this session.
    pub child_threads: i64,
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
        let (window, total) = if let Some(needle) = search.as_deref() {
            // Keyword search must consider records beyond one store page, so it
            // is executed here over bounded consecutive pages instead of being
            // applied as a post-pagination filter. Offset/limit window the
            // matching set; total covers everything inside the scan cap.
            let scanned = search_scan(store.clone(), &user_id).await?;
            let matching: Vec<ChatSessionRecord> = scanned
                .into_iter()
                .filter(|record| searchable_text(record).contains(needle))
                .filter(|record| {
                    parent
                        .as_deref()
                        .is_none_or(|parent_id| record.parent_session_id.as_deref() == Some(parent_id))
                })
                .collect();
            let total = matching.len() as i64;
            let window: Vec<ChatSessionRecord> = matching
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect();
            (window, total)
        } else {
            let store = store.clone();
            let user_id = user_id.clone();
            let parent = parent.clone();
            let (records, total) = blocking::run_db("thread_catalog.list", move || {
                store.list_chat_sessions(&user_id, None, parent.as_deref(), offset, limit)
            })
            .await?;
            (records, total)
        };
        let child_counts = self.child_counts(&user_id, &window).await;
        let mut items = Vec::with_capacity(window.len());
        for record in window {
            let monitor = self.state.monitor.get_record(&record.session_id);
            let activity =
                load_chat_session_activity(&self.state, &record.session_id, monitor.as_ref()).await;
            let status = normalize_status(&record.status, monitor.as_ref(), activity.running);
            items.push(ThreadSnapshot {
                child_threads: child_counts
                    .get(&record.session_id)
                    .copied()
                    .unwrap_or(0),
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
        items.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        Ok(ThreadPage {
            items,
            offset,
            limit,
            total,
        })
    }

    async fn child_counts(
        &self,
        user_id: &str,
        records: &[ChatSessionRecord],
    ) -> HashMap<String, i64> {
        if records.is_empty() {
            return HashMap::new();
        }
        let parents: Vec<String> = records
            .iter()
            .map(|record| record.session_id.clone())
            .collect();
        let user = user_id.to_string();
        let store = self.state.user_store.clone();
        let rows = blocking::run_db("thread_catalog.child_counts", move || {
            store.count_child_chat_sessions(&user, &parents)
        })
        .await;
        match rows {
            Ok(rows) => rows.into_iter().collect(),
            Err(_) => HashMap::new(),
        }
    }

    pub async fn snapshot(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ThreadSnapshot>> {
        let session = session_id.trim();
        if session.is_empty() {
            return Ok(None);
        }
        let store = self.state.user_store.clone();
        let user = user_id.trim().to_string();
        let target = session.to_string();
        let user_for_snapshot = user.clone();
        let record = blocking::run_db("thread_catalog.snapshot", move || {
            store.get_chat_session(&user_for_snapshot, &target)
        })
        .await?;
        let Some(record) = record else {
            return Ok(None);
        };
        let page = self
            .list(ThreadListQuery {
                user_id: user,
                limit: 1,
                parent_session_id: Some(record.session_id.clone()),
                ..Default::default()
            })
            .await;
        let child_threads = page.map(|page| page.total).unwrap_or(0);
        let monitor = self.state.monitor.get_record(&record.session_id);
        let activity =
            load_chat_session_activity(&self.state, &record.session_id, monitor.as_ref()).await;
        let status = normalize_status(&record.status, monitor.as_ref(), activity.running);
        Ok(Some(ThreadSnapshot {
            session_id: record.session_id,
            title: record.title,
            status,
            agent_id: record.agent_id,
            parent_session_id: record.parent_session_id,
            spawn_label: record.spawn_label,
            spawned_by: record.spawned_by,
            updated_at: record.updated_at,
            last_message_at: record.last_message_at,
            child_threads,
            monitor,
        }))
    }
}

fn searchable_text(record: &ChatSessionRecord) -> String {
    format!(
        "{} {} {} {}",
        record.session_id,
        record.title,
        record.agent_id.as_deref().unwrap_or_default(),
        record.spawn_label.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase()
}

async fn search_scan(store: Arc<UserStore>, user_id: &str) -> Result<Vec<ChatSessionRecord>> {
    let mut scanned = Vec::new();
    let mut offset = 0i64;
    let user = user_id.to_string();
    for _ in 0..MAX_SEARCH_SCAN_PAGES {
        let store = store.clone();
        let user = user.clone();
        let (page, _) = blocking::run_db("thread_catalog.search", move || {
            store.list_chat_sessions(&user, None, None, offset, STORE_PAGE_SIZE)
        })
        .await?;
        let page_len = page.len() as i64;
        scanned.extend(page);
        if page_len < STORE_PAGE_SIZE {
            break;
        }
        offset += page_len;
    }
    Ok(scanned)
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
