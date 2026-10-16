//! Bounded presentation reads. Execution snapshots are not a page contract.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CHAT_PAGE_SIZE: usize = 12;
pub const CHAT_PAGE_MAX: usize = 20;
pub const TRAJECTORY_PAGE_SIZE: usize = 80;
pub const TRAJECTORY_PAGE_MAX: usize = 100;
pub const BODY_CHUNK_BYTES: usize = 8192;
pub const CHAT_RESPONSE_BYTES: usize = 128 * 1024;
pub const TRAJECTORY_RESPONSE_BYTES: usize = 192 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BodyRef {
    pub body_id: String,
    pub revision: i64,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewMessage {
    pub item_id: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub created_seq: i64,
    #[serde(default)]
    pub durable_seq: i64,
    #[serde(default)]
    pub content_offset: usize,
    /// UTF-16 cursor for the independent reasoning stream.  Keeping this
    /// separate from `content_offset` lets replay and live tails reject
    /// duplicate/out-of-order reasoning frames without guessing from a
    /// bounded preview.
    #[serde(default)]
    pub reasoning_offset: usize,
    pub body: Option<BodyRef>,
    pub feedback: Option<Value>,
    pub stats: Option<Value>,
    #[serde(default)]
    pub tail: String,
    #[serde(default)]
    pub tail_offset: usize,
    #[serde(default)]
    pub failure: String,
    #[serde(default)]
    pub attachments: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatViewRow {
    #[serde(default)]
    pub execution_turn_id: String,
    pub root_turn_id: String,
    pub user_round: i64,
    pub client_message_id: Option<String>,
    pub revision: i64,
    pub status: String,
    /// Explicit composer ownership, independent of the executing thread head.
    #[serde(default)]
    pub queued_input: Option<QueuedInput>,
    pub user: ViewMessage,
    pub assistant: ViewMessage,
    pub workflow_count: i64,
    /// Monotonic durable tool identity for directory invalidation, including
    /// queue-owned runs and replay where transient workspace events are absent.
    #[serde(default)]
    pub workspace_change_seq: i64,
    /// Number of model rounds that emitted reasoning in this user turn.
    #[serde(default)]
    pub reasoning_count: i64,
    #[serde(default)]
    pub reasoning_item_id: String,
    /// Latest bounded execution phase; never includes tool output or reasoning.
    #[serde(default)]
    pub process_activity: ProcessActivity,
    pub failed_count: i64,
    #[serde(default)]
    pub subagents: Vec<Value>,
    pub created_time: f64,
    pub updated_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrajectoryViewRow {
    pub item_id: String,
    pub root_turn_id: String,
    pub turn_id: String,
    pub user_round: i64,
    pub item_index: i64,
    pub created_seq: i64,
    pub revision: i64,
    pub kind: String,
    pub status: String,
    pub summary: String,
    /// Overview pages contain one representative row per user turn. Selecting
    /// one must load that turn's complete, keyset-paged trajectory.
    #[serde(default)]
    pub overview_only: bool,
    pub tool_name: Option<String>,
    pub model_round: i64,
    /// Bounded token usage projection from message_stats. Detail rows need this
    /// on the page itself so trajectory rendering does not fetch whole bodies.
    #[serde(default)]
    pub stats: Option<Value>,
    pub started_at: f64,
    pub completed_at: Option<f64>,
    pub detail: Option<BodyRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrajectoryTile {
    pub bucket: i64,
    pub count: i64,
    pub failed: i64,
    pub running: i64,
    pub start: f64,
    pub end: f64,
    /// Per-lane activity totals. Older projections can omit these fields;
    /// consumers must use `count` as a bounded all-activity fallback.
    #[serde(default)]
    pub input_count: i64,
    #[serde(default)]
    pub model_count: i64,
    #[serde(default)]
    pub tool_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessActivity {
    pub kind: String,
    pub item_id: String,
    pub tool: String,
    pub created_seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueuedInput {
    pub queue_id: String,
    pub reason: String,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadExecutionHead {
    pub runtime_status: String,
    pub active_turn_id: String,
    pub epoch: i64,
    pub view_seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadViewPage {
    pub runtime_status: String,
    pub active_turn_id: String,
    pub session_id: String,
    pub epoch: i64,
    pub view_seq: i64,
    pub projection_state: String,
    /// The durable session exists, but the bounded presentation read could
    /// not finish before its deadline. Consumers must keep any already
    /// committed rows and surface this as a degraded view rather than a
    /// session-open failure.
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub diagnostic: Option<String>,
    pub thread_empty: Option<bool>,
    pub latest_round: i64,
    pub chat: Vec<ChatViewRow>,
    pub trajectory: Vec<TrajectoryViewRow>,
    pub tiles: Vec<TrajectoryTile>,
    pub tile_level: usize,
    pub next_before: Option<i64>,
    pub has_more: bool,
    /// Exclusive forward cursor for returning through an evicted window.
    #[serde(default)]
    pub next_after: Option<i64>,
    #[serde(default)]
    pub has_newer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadBodyPage {
    pub body_id: String,
    pub revision: i64,
    pub chunks: Vec<String>,
    pub next_cursor: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadViewChanges {
    pub runtime_status: String,
    pub active_turn_id: String,
    pub epoch: i64,
    pub view_seq: i64,
    pub reset_required: bool,
    pub rows: Vec<ChatViewRow>,
}

/// Byte bounded UTF-8 preview, also safe for JSON's worst case escaping.
pub fn preview(text: &str, budget: usize) -> String {
    let mut end = text.len().min(budget);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn chunks(text: &str) -> impl Iterator<Item = &str> {
    let mut remaining = text;
    std::iter::from_fn(move || {
        if remaining.is_empty() {
            return None;
        }
        let mut end = remaining.len().min(BODY_CHUNK_BYTES);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, rest) = remaining.split_at(end);
        remaining = rest;
        Some(chunk)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadSearchHit {
    pub item_id: String,
    pub body: BodyRef,
    pub cursor: i64,
    pub snippet: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadSearchPage {
    pub hits: Vec<ThreadSearchHit>,
    pub has_more: bool,
}
