//! Shared, bounded thread directory used by local and remote clients.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::blocking;
use crate::ops::monitor::SessionUsageSummary;
use crate::services::chat_runtime_projection::{load_chat_session_activity, ChatSessionActivity};
use crate::services::user_store::UserStore;
use crate::state::AppState;
use crate::storage::{ChatSessionRecord, ThreadTurnTail, WorkspaceRecord};
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

/// Why a thread sits in `needs_you`, so a directory can name the blocker
/// instead of making the client guess from a status string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPendingReason {
    Approval,
    UserInput,
}

impl ThreadPendingReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::UserInput => "user_input",
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
    /// Restrict the page to one workspace. `None` lists every workspace, which
    /// is what the "all workspaces" escape hatch of the resume flow asks for.
    pub workspace_id: Option<String>,
}

/// One directory row. Status, round, watermark and activity all come from the
/// durable tail or the monitor projection; clients must not re-derive them.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadSnapshot {
    pub session_id: String,
    pub title: String,
    pub status: ThreadStatus,
    pub agent_id: Option<String>,
    /// Workspace this thread belongs to; `None` only for legacy rows.
    pub workspace_id: Option<String>,
    /// Workspace display name and colour, resolved once per page.
    pub workspace_name: Option<String>,
    pub workspace_color: Option<String>,
    pub parent_session_id: Option<String>,
    pub spawn_label: Option<String>,
    pub spawned_by: Option<String>,
    pub updated_at: f64,
    pub last_message_at: f64,
    /// Number of child threads whose parent is this session.
    pub child_threads: i64,
    /// Highest durable user round; 0 when the thread never ran a turn.
    pub user_round: i64,
    /// Status of that durable turn ("" when there is none).
    pub turn_status: String,
    /// Last activity line: the monitor stage while running, the turn summary once settled.
    pub last_activity: Option<String>,
    pub context_tokens: i64,
    pub consumed_tokens: i64,
    pub tool_calls: i64,
    pub model_rounds: Option<i64>,
    /// `thread_log_changes` watermark: the cursor a client replays from.
    pub change_seq: i64,
    pub pending_reason: Option<ThreadPendingReason>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadPage {
    pub items: Vec<ThreadSnapshot>,
    pub offset: i64,
    pub limit: i64,
    pub total: i64,
    /// Highest durable watermark across the page, for incremental refresh.
    pub watermark: i64,
}

/// Write-access verdict for one thread. Every thread a user owns is writable;
/// the verdict stays a per-thread query so shells can keep rendering a
/// read-only watch state without re-deriving it from the thread list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ThreadWriteAccess {
    pub writable: bool,
    pub reason: Option<String>,
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
        let workspace = query
            .workspace_id
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
                    parent.as_deref().is_none_or(|parent_id| {
                        record.parent_session_id.as_deref() == Some(parent_id)
                    })
                })
                .filter(|record| {
                    workspace.as_deref().is_none_or(|workspace_id| {
                        record.workspace_id.as_deref() == Some(workspace_id)
                    })
                })
                .collect();
            let total = matching.len() as i64;
            let window: Vec<ChatSessionRecord> = matching
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect();
            (window, total)
        } else if let Some(workspace_id) = workspace.clone() {
            let storage = self.state.storage.clone();
            let user_id = user_id.clone();
            let parent = parent.clone();
            let (records, total) = if parent.is_none() {
                // The common resume path: one workspace, page pushed into SQL.
                blocking::run_db("thread_catalog.list_workspace", move || {
                    storage.list_chat_sessions_by_workspace(
                        &user_id,
                        &workspace_id,
                        None,
                        offset,
                        limit,
                    )
                })
                .await?
            } else {
                // Workspace plus parent filter: scan a bounded number of pages
                // inside the workspace instead of every thread of the user.
                let mut scanned = Vec::new();
                let mut page_offset = 0i64;
                for _ in 0..MAX_SEARCH_SCAN_PAGES {
                    let storage = storage.clone();
                    let user_id = user_id.clone();
                    let workspace_id = workspace_id.clone();
                    let (page, _) =
                        blocking::run_db("thread_catalog.list_workspace_page", move || {
                            storage.list_chat_sessions_by_workspace(
                                &user_id,
                                &workspace_id,
                                None,
                                page_offset,
                                STORE_PAGE_SIZE,
                            )
                        })
                        .await?;
                    let page_len = page.len() as i64;
                    scanned.extend(page);
                    if page_len < STORE_PAGE_SIZE {
                        break;
                    }
                    page_offset += page_len;
                }
                let filtered: Vec<ChatSessionRecord> = scanned
                    .into_iter()
                    .filter(|record| {
                        parent.as_deref().is_none_or(|parent_id| {
                            record.parent_session_id.as_deref() == Some(parent_id)
                        })
                    })
                    .collect();
                let total = filtered.len() as i64;
                let window = filtered
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect::<Vec<_>>();
                (window, total)
            };
            (records, total)
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
        let mut items = self.project_rows(&user_id, &window).await;
        let child_counts = self.child_counts(&user_id, &window).await;
        if !child_counts.is_empty() {
            for item in items.iter_mut() {
                item.child_threads = child_counts.get(&item.session_id).copied().unwrap_or(0);
            }
        }
        let watermark = items.iter().map(|item| item.change_seq).max().unwrap_or(0);
        items.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        Ok(ThreadPage {
            items,
            offset,
            limit,
            total,
            watermark,
        })
    }

    /// Fold a page of session records into directory rows. The durable tails and
    /// the monitor states are read once per page, so cost grows with the page and
    /// not with four queries per row.
    async fn project_rows(
        &self,
        user_id: &str,
        records: &[ChatSessionRecord],
    ) -> Vec<ThreadSnapshot> {
        if records.is_empty() {
            return Vec::new();
        }
        let session_ids = records
            .iter()
            .map(|record| record.session_id.clone())
            .collect::<Vec<_>>();
        let storage = self.state.storage.clone();
        let owner = user_id.to_string();
        let ids = session_ids.clone();
        let tails = blocking::run_db("thread_catalog.tails", move || {
            storage.thread_turn_tails(&owner, &ids)
        })
        .await
        .unwrap_or_default();
        let usage = self.state.monitor.session_usage_summaries(&session_ids);
        // Workspace names are a per-user list, so one query covers the page; a
        // page whose rows carry no workspace is the legacy case and skips it.
        let workspaces = self.page_workspaces(user_id, records).await;
        let mut rows = Vec::with_capacity(records.len());
        for record in records {
            let summary = usage.get(&record.session_id);
            let activity =
                load_chat_session_activity(&self.state, &record.session_id, summary).await;
            let workspace = record
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| workspaces.get(workspace_id));
            rows.push(build_snapshot(
                record,
                tails.get(&record.session_id),
                summary,
                &activity,
                workspace,
            ));
        }
        rows
    }

    /// `workspace_id -> workspace row` for one page, fetched only when needed.
    async fn page_workspaces(
        &self,
        user_id: &str,
        records: &[ChatSessionRecord],
    ) -> HashMap<String, WorkspaceRecord> {
        if !records.iter().any(|record| record.workspace_id.is_some()) {
            return HashMap::new();
        }
        let storage = self.state.storage.clone();
        let owner = user_id.to_string();
        let rows = blocking::run_db("thread_catalog.workspaces", move || {
            storage.list_workspaces(&owner)
        })
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .map(|record| (record.workspace_id.clone(), record))
                .collect(),
            // A missing name never blocks the thread list: the row falls back to
            // the workspace id.
            Err(_) => HashMap::new(),
        }
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
        let mut rows = self
            .project_rows(&user, std::slice::from_ref(&record))
            .await;
        let Some(mut snapshot) = rows.pop() else {
            return Ok(None);
        };
        let page = self
            .list(ThreadListQuery {
                user_id: user.clone(),
                limit: 1,
                parent_session_id: Some(record.session_id.clone()),
                ..Default::default()
            })
            .await;
        snapshot.child_threads = page.map(|page| page.total).unwrap_or(0);
        Ok(Some(snapshot))
    }

    /// Durable ThreadLog frames after `change_seq`, in cursor order. This is the
    /// paging cursor of §10: a client reconnects, dedups and orders by
    /// `thread_log_changes.change_seq`, never by a transport event id. Frames keep
    /// the runtime's shared change-frame shape, including the snapshot guard.
    pub async fn changes(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let session = session_id.trim().to_string();
        if session.is_empty() {
            return Ok(Vec::new());
        }
        let workspace = self.state.workspace.clone();
        blocking::run_db("thread_catalog.changes", move || {
            workspace.try_load_thread_changes(&session, after_seq.max(0), limit)
        })
        .await
    }

    /// Whether this process may write to one thread, and why not. Every thread
    /// is writable now that no background orchestration run can own it; the
    /// verdict stays per-thread so the 舵机 and 蜂窝 submit paths and the 舰体
    /// HTTP layer keep one shared authority instead of open-coding the rule.
    pub async fn write_access(&self, user_id: &str, session_id: &str) -> Result<ThreadWriteAccess> {
        let _ = (user_id, session_id);
        Ok(ThreadWriteAccess {
            writable: true,
            reason: None,
        })
    }
}

fn build_snapshot(
    record: &ChatSessionRecord,
    tail: Option<&ThreadTurnTail>,
    usage: Option<&SessionUsageSummary>,
    activity: &ChatSessionActivity,
    workspace: Option<&WorkspaceRecord>,
) -> ThreadSnapshot {
    let turn_status = tail.map(|tail| tail.turn_status.as_str()).unwrap_or("");
    let monitor_status = usage
        .map(|summary| summary.status.as_str())
        .unwrap_or_default();
    let runtime_status = activity
        .runtime
        .as_ref()
        .and_then(|value| value.get("thread_status"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // A stale monitor row can still claim the thread is live; the in-process
    // activity probe is the only authority on running.
    let status = StatusSignals {
        record: &record.status,
        monitor: monitor_status,
        turn: turn_status,
        runtime: runtime_status,
        running: activity.running,
    }
    .resolve();
    let last_activity = if activity.running {
        usage.map(|summary| summary.activity.clone())
    } else {
        tail.map(|tail| tail.last_activity.clone())
    }
    .filter(|value| !value.trim().is_empty());
    ThreadSnapshot {
        session_id: record.session_id.clone(),
        title: record.title.clone(),
        status,
        agent_id: record.agent_id.clone(),
        workspace_id: record.workspace_id.clone(),
        // The name is what a thread center lists; the workspace directory is
        // already the row's identity for grouping, so no path is duplicated here.
        workspace_name: workspace.map(|item| item.name.clone()),
        workspace_color: workspace.map(|item| item.color.clone()),
        parent_session_id: record.parent_session_id.clone(),
        spawn_label: record.spawn_label.clone(),
        spawned_by: record.spawned_by.clone(),
        updated_at: record.updated_at,
        last_message_at: record.last_message_at,
        child_threads: 0,
        user_round: tail.map(|tail| tail.user_round).unwrap_or(0),
        turn_status: turn_status.to_string(),
        last_activity,
        context_tokens: usage.map(|summary| summary.context_tokens).unwrap_or(0),
        consumed_tokens: usage.map(|summary| summary.consumed_tokens).unwrap_or(0),
        tool_calls: usage.map(|summary| summary.tool_calls).unwrap_or(0),
        model_rounds: usage.and_then(|summary| summary.model_request_count),
        change_seq: tail.map(|tail| tail.change_seq).unwrap_or(0),
        pending_reason: (status == ThreadStatus::NeedsYou)
            .then(|| pending_reason(activity.runtime.as_ref(), monitor_status, turn_status))
            .flatten(),
    }
}

/// The signals a status can be derived from, each with exactly one owner: the
/// chat row, the monitor projection, the durable turn and the live runtime.
/// Every client - 舵机 directory, 蜂窝 façade, 舰体 chat list - builds this once,
/// so no two views answer "what is this thread doing" differently.
#[derive(Debug, Clone, Copy)]
pub struct StatusSignals<'a> {
    /// Status string on the chat session row.
    pub record: &'a str,
    /// Live monitor status ("" when the thread has no monitor row).
    pub monitor: &'a str,
    /// Status of the newest durable user turn ("" when it never ran).
    pub turn: &'a str,
    /// In-process thread runtime status ("" when not loaded here).
    pub runtime: &'a str,
    /// The activity probe's verdict for this thread in this process.
    pub running: bool,
}

impl StatusSignals<'_> {
    fn parts(&self) -> (String, String, String, String) {
        (
            self.record.trim().to_ascii_lowercase(),
            self.monitor.trim().to_ascii_lowercase(),
            self.turn.trim().to_ascii_lowercase(),
            self.runtime.trim().to_ascii_lowercase(),
        )
    }

    /// Fixed derivation order (§4.1): explicit waiting > live runtime or queue >
    /// terminal error > loaded and idle > durable terminal turn or archived >
    /// ready. Durable turn status outranks the chat row, so a restart or a
    /// directory refresh cannot flash a wrong state.
    pub fn resolve(&self) -> ThreadStatus {
        let (record, monitor, turn, runtime) = self.parts();
        if [
            record.as_str(),
            monitor.as_str(),
            turn.as_str(),
            runtime.as_str(),
        ]
        .iter()
        .any(|signal| is_waiting(signal))
        {
            return ThreadStatus::NeedsYou;
        }
        if self.running
            || [monitor.as_str(), turn.as_str(), runtime.as_str()]
                .iter()
                .any(|signal| is_live(signal))
        {
            return ThreadStatus::Working;
        }
        if [record.as_str(), monitor.as_str(), turn.as_str()]
            .iter()
            .any(|signal| is_failed(signal))
        {
            return ThreadStatus::Failed;
        }
        // A cancelled or interrupted run is history, not an idle thread waiting for
        // input: keeping it out of 就绪 is what lets a directory show why the thread
        // stopped instead of implying it is ready to continue untouched.
        if [monitor.as_str(), turn.as_str(), record.as_str()]
            .iter()
            .any(|signal| is_stopped(signal))
        {
            return ThreadStatus::Finished;
        }
        // Ready means "resident and waiting for the next prompt", so it needs live
        // evidence; a detached thread whose last turn settled is history.
        if is_loaded_idle(&monitor) || is_loaded_idle(&runtime) {
            return ThreadStatus::Ready;
        }
        if turn.is_empty() {
            return if is_history_row(&record) {
                ThreadStatus::Finished
            } else {
                ThreadStatus::Ready
            };
        }
        if is_loaded_idle(&record) {
            return ThreadStatus::Ready;
        }
        ThreadStatus::Finished
    }

    /// The wire vocabulary remote clients read. It stays finer than the five tabs -
    /// queued apart from running, cancelled apart from completed - but it is
    /// derived from the same precedence, so one row never answers twice.
    pub fn runtime_status(&self) -> &'static str {
        let (_, monitor, turn, runtime) = self.parts();
        let queued = |value: &str| matches!(value, "queued" | "admitted" | "accepted");
        match self.resolve() {
            ThreadStatus::NeedsYou => "waiting_user_input",
            ThreadStatus::Working => {
                if queued(&monitor) || queued(&turn) || queued(&runtime) {
                    "queued"
                } else {
                    "running"
                }
            }
            ThreadStatus::Failed => "failed",
            ThreadStatus::Ready => "idle",
            ThreadStatus::Finished => {
                if is_stopped(&monitor) || is_stopped(&turn) {
                    "cancelled"
                } else {
                    "completed"
                }
            }
        }
    }
}

fn is_waiting(status: &str) -> bool {
    matches!(
        status,
        "waiting" | "waiting_input" | "waiting_user_input" | "waiting_approval" | "needs_input"
    )
}

fn is_live(status: &str) -> bool {
    matches!(
        status,
        "running" | "queued" | "cancelling" | "streaming" | "executing" | "admitted" | "accepted"
    )
}

fn is_failed(status: &str) -> bool {
    matches!(status, "failed" | "error" | "system_error" | "rejected")
}

fn is_stopped(status: &str) -> bool {
    matches!(status, "cancelled" | "canceled" | "interrupted" | "stopped")
}

fn is_loaded_idle(status: &str) -> bool {
    matches!(status, "idle" | "ready" | "paused")
}

fn is_history_row(status: &str) -> bool {
    matches!(status, "completed" | "finished" | "archived")
}

fn pending_reason(
    runtime: Option<&Value>,
    monitor_status: &str,
    turn_status: &str,
) -> Option<ThreadPendingReason> {
    let turn = runtime.and_then(|value| value.get("turn"));
    let thread_status = runtime
        .and_then(|value| value.get("thread_status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let approvals = turn
        .and_then(|turn| turn.get("pending_approval_count"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if thread_status == "waiting_approval"
        || approvals > 0
        || monitor_status == "waiting_approval"
        || turn_status == "waiting_approval"
    {
        return Some(ThreadPendingReason::Approval);
    }
    let waits_for_input = turn
        .and_then(|turn| turn.get("waiting_for_user_input"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if thread_status == "waiting_user_input"
        || waits_for_input
        || monitor_status == "waiting_user_input"
        || matches!(
            turn_status,
            "waiting" | "waiting_input" | "waiting_user_input" | "needs_input"
        )
    {
        return Some(ThreadPendingReason::UserInput);
    }
    // Neither signal was explicit: a waiting row is an answer request, because
    // approvals always carry a pending count.
    Some(ThreadPendingReason::UserInput)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signals(
        record: &str,
        monitor: &str,
        turn: &str,
        runtime: &str,
        running: bool,
    ) -> ThreadStatus {
        StatusSignals {
            record,
            monitor,
            turn,
            runtime,
            running,
        }
        .resolve()
    }

    #[test]
    fn one_mapping_table_answers_both_clients() {
        // The five directory tabs and the remote wire vocabulary come from the same
        // precedence, so 舵机, 蜂窝 and 舰体 cannot disagree about one row.
        let cases = [
            (
                "running",
                "waiting",
                "",
                "",
                false,
                ThreadStatus::NeedsYou,
                "waiting_user_input",
            ),
            (
                "active",
                "waiting",
                "",
                "",
                true,
                ThreadStatus::NeedsYou,
                "waiting_user_input",
            ),
            (
                "active",
                "",
                "waiting_approval",
                "",
                false,
                ThreadStatus::NeedsYou,
                "waiting_user_input",
            ),
            (
                "active",
                "queued",
                "",
                "",
                false,
                ThreadStatus::Working,
                "queued",
            ),
            (
                "active",
                "",
                "queued",
                "",
                false,
                ThreadStatus::Working,
                "queued",
            ),
            (
                "active",
                "running",
                "",
                "",
                false,
                ThreadStatus::Working,
                "running",
            ),
            (
                "active",
                "cancelling",
                "",
                "",
                false,
                ThreadStatus::Working,
                "running",
            ),
            (
                "active",
                "",
                "streaming",
                "",
                false,
                ThreadStatus::Working,
                "running",
            ),
            (
                "active",
                "",
                "",
                "running",
                true,
                ThreadStatus::Working,
                "running",
            ),
            ("failed", "", "", "", false, ThreadStatus::Failed, "failed"),
            (
                "active",
                "error",
                "",
                "",
                false,
                ThreadStatus::Failed,
                "failed",
            ),
            (
                "active",
                "",
                "rejected",
                "",
                false,
                ThreadStatus::Failed,
                "failed",
            ),
            (
                "active",
                "idle",
                "completed",
                "idle",
                false,
                ThreadStatus::Ready,
                "idle",
            ),
            ("active", "", "", "", false, ThreadStatus::Ready, "idle"),
            (
                "active",
                "",
                "completed",
                "",
                false,
                ThreadStatus::Finished,
                "completed",
            ),
            (
                "archived",
                "",
                "completed",
                "",
                false,
                ThreadStatus::Finished,
                "completed",
            ),
            (
                "active",
                "cancelled",
                "",
                "",
                false,
                ThreadStatus::Finished,
                "cancelled",
            ),
            (
                "active",
                "",
                "interrupted",
                "",
                false,
                ThreadStatus::Finished,
                "cancelled",
            ),
            (
                "archived",
                "",
                "",
                "",
                false,
                ThreadStatus::Finished,
                "completed",
            ),
        ];
        for (record, monitor, turn, runtime, running, expected_status, expected_wire) in cases {
            let signals = StatusSignals {
                record,
                monitor,
                turn,
                runtime,
                running,
            };
            assert_eq!(
                signals.resolve(),
                expected_status,
                "status for {record}/{monitor}/{turn}/{runtime}"
            );
            assert_eq!(
                signals.runtime_status(),
                expected_wire,
                "wire status for {record}/{monitor}/{turn}/{runtime}"
            );
        }
    }

    #[test]
    fn status_priority_prefers_needs_you_over_running() {
        assert_eq!(
            signals(
                "running",
                "waiting",
                "waiting_input",
                "waiting_user_input",
                true
            ),
            ThreadStatus::NeedsYou
        );
    }

    #[test]
    fn durable_turn_status_outlives_a_stale_monitor_row() {
        // Nothing is loaded in this process and the durable turn settled: the
        // directory must show history, not flash a live state.
        assert_eq!(
            signals("active", "", "completed", "not_loaded", false),
            ThreadStatus::Finished
        );
        assert_eq!(
            signals("active", "", "failed", "", false),
            ThreadStatus::Failed
        );
        assert_eq!(
            signals("active", "", "cancelled", "", false),
            ThreadStatus::Finished
        );
    }

    #[test]
    fn live_runtime_still_beats_a_durable_terminal_turn() {
        assert_eq!(
            signals("completed", "running", "completed", "running", true),
            ThreadStatus::Working
        );
        // A resident thread that settled its last turn waits for the next prompt.
        assert_eq!(
            signals("active", "idle", "completed", "idle", false),
            ThreadStatus::Ready
        );
    }

    #[test]
    fn status_maps_terminal_and_ready_states() {
        assert_eq!(signals("failed", "", "", "", false), ThreadStatus::Failed);
        assert_eq!(signals("idle", "", "", "", false), ThreadStatus::Ready);
        // A chat row that finished without a durable turn is history, not a draft.
        assert_eq!(
            signals("completed", "", "", "", false),
            ThreadStatus::Finished
        );
        assert_eq!(
            signals("archived", "", "completed", "", false),
            ThreadStatus::Finished
        );
    }

    #[test]
    fn pending_reason_prefers_the_live_runtime_signal() {
        let approval =
            json!({"thread_status": "waiting_approval", "turn": {"pending_approval_count": 1}});
        assert_eq!(
            pending_reason(Some(&approval), "waiting", ""),
            Some(ThreadPendingReason::Approval)
        );
        let input = json!({"thread_status": "idle", "turn": {"waiting_for_user_input": true}});
        assert_eq!(
            pending_reason(Some(&input), "waiting", "waiting_input"),
            Some(ThreadPendingReason::UserInput)
        );
        assert_eq!(
            pending_reason(None, "waiting", "waiting_approval"),
            Some(ThreadPendingReason::Approval)
        );
    }

    #[test]
    fn row_reads_watermark_round_and_activity() {
        let record = sample_record("completed");
        let tail = ThreadTurnTail {
            session_id: "s-1".to_string(),
            user_round: 4,
            turn_status: "completed".to_string(),
            last_activity: "done".to_string(),
            change_seq: 97,
        };
        let usage = SessionUsageSummary {
            status: "finished".to_string(),
            context_tokens: 1200,
            consumed_tokens: 900,
            tool_calls: 3,
            model_request_count: Some(5),
            activity: "streaming".to_string(),
            ..Default::default()
        };
        let activity = ChatSessionActivity {
            runtime: None,
            running: false,
        };
        let row = build_snapshot(&record, Some(&tail), Some(&usage), &activity, None);
        assert_eq!(row.status, ThreadStatus::Finished);
        assert_eq!(row.user_round, 4);
        assert_eq!(row.change_seq, 97);
        assert_eq!(row.context_tokens, 1200);
        assert_eq!(row.consumed_tokens, 900);
        assert_eq!(row.tool_calls, 3);
        assert_eq!(row.model_rounds, Some(5));
        // Settled rows show the durable turn summary, not the stale monitor stage.
        assert_eq!(row.last_activity.as_deref(), Some("done"));
        assert_eq!(row.pending_reason, None);
    }

    #[test]
    fn running_row_reports_the_live_stage() {
        let record = sample_record("running");
        let tail = ThreadTurnTail {
            session_id: "s-1".to_string(),
            user_round: 2,
            turn_status: "running".to_string(),
            last_activity: String::new(),
            change_seq: 12,
        };
        let usage = SessionUsageSummary {
            status: "running".to_string(),
            activity: "tool: read_file".to_string(),
            ..Default::default()
        };
        let activity = ChatSessionActivity {
            runtime: Some(json!({"thread_status": "running"})),
            running: true,
        };
        let row = build_snapshot(&record, Some(&tail), Some(&usage), &activity, None);
        assert_eq!(row.status, ThreadStatus::Working);
        assert_eq!(row.last_activity.as_deref(), Some("tool: read_file"));
    }

    #[test]
    fn waiting_row_names_the_blocker() {
        let record = sample_record("waiting_input");
        let tail = ThreadTurnTail {
            session_id: "s-1".to_string(),
            user_round: 1,
            turn_status: "waiting_input".to_string(),
            last_activity: String::new(),
            change_seq: 4,
        };
        let activity = ChatSessionActivity {
            runtime: None,
            running: false,
        };
        let row = build_snapshot(&record, Some(&tail), None, &activity, None);
        assert_eq!(row.status, ThreadStatus::NeedsYou);
        assert_eq!(row.pending_reason, Some(ThreadPendingReason::UserInput));
    }

    fn sample_record(status: &str) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: "s-1".to_string(),
            user_id: "u-1".to_string(),
            title: "Task".to_string(),
            status: status.to_string(),
            created_at: 1.0,
            updated_at: 2.0,
            last_message_at: 2.0,
            agent_id: None,
            workspace_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    async fn build_state(name: &str) -> (AppState, tempfile::TempDir) {
        use crate::config::Config;
        use crate::config_store::ConfigStore;
        use crate::state::AppStateInitOptions;

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_dir.path().to_path_buf();
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = root
            .join(format!("{name}.db"))
            .to_string_lossy()
            .to_string();
        config.workspace.root = root.join("workspaces").to_string_lossy().to_string();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(root.join("wunder.yaml"));
        let config_for_store = config.clone();
        config_store
            .update(|current| *current = config_for_store.clone())
            .await
            .expect("write config");
        let state = AppState::new_with_options(
            config_store,
            config,
            AppStateInitOptions::cli_default().with_start_thread_runtime(false),
        )
        .expect("create app state");
        (state, temp_dir)
    }

    #[tokio::test]
    async fn the_directory_lists_one_workspace_and_names_it_on_every_row() {
        let (state, _temp) = build_state("workspace_filter").await;
        let catalog = ThreadCatalogService::new(state.clone());
        let now = 1_700_000_000.0;
        for (session_id, workspace_id) in [
            ("alpha-1", "ws_alpha"),
            ("alpha-2", "ws_alpha"),
            ("beta-1", "ws_beta"),
        ] {
            state
                .user_store
                .upsert_chat_session(&ChatSessionRecord {
                    session_id: session_id.to_string(),
                    user_id: "owner".to_string(),
                    title: format!("Task {session_id}"),
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_message_at: now,
                    agent_id: None,
                    workspace_id: Some(workspace_id.to_string()),
                    tool_overrides: Vec::new(),
                    parent_session_id: None,
                    parent_message_id: None,
                    spawn_label: None,
                    spawned_by: None,
                })
                .expect("seed thread");
        }

        let alpha = catalog
            .list(ThreadListQuery {
                user_id: "owner".to_string(),
                limit: 20,
                workspace_id: Some("ws_alpha".to_string()),
                ..Default::default()
            })
            .await
            .expect("list alpha");
        let ids: Vec<&str> = alpha
            .items
            .iter()
            .map(|item| item.session_id.as_str())
            .collect();
        assert_eq!(ids.len(), 2, "only the requested workspace: {ids:?}");
        assert!(ids.contains(&"alpha-1") && ids.contains(&"alpha-2"));
        assert_eq!(alpha.total, 2);
        assert!(
            alpha
                .items
                .iter()
                .all(|item| item.workspace_id.as_deref() == Some("ws_alpha")),
            "every row names its workspace"
        );

        // No filter is the explicit "every workspace" view.
        let all = catalog
            .list(ThreadListQuery {
                user_id: "owner".to_string(),
                limit: 20,
                ..Default::default()
            })
            .await
            .expect("list all");
        assert_eq!(all.total, 3);
        assert!(all
            .items
            .iter()
            .any(|item| item.workspace_id.as_deref() == Some("ws_beta")));
    }

    #[tokio::test]
    async fn write_access_allows_plain_threads() {
        let (state, _temp) = build_state("write_access").await;
        let catalog = ThreadCatalogService::new(state.clone());
        let open = catalog
            .write_access("owner", "free-thread")
            .await
            .expect("read access");
        assert!(open.writable, "a plain thread is writable");
        assert_eq!(open.reason, None);
    }
}
