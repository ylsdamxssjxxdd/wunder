//! Storage-neutral contracts for canonical agent-thread history.
//!
//! A rollout JSONL file is the authority for completed execution history.
//! Implementations may maintain a SQLite history projection, but callers only
//! use the IDs, positions and cursors defined here; they never receive a path
//! or a database connection.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! string_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ThreadStoreError> {
                let value = value.into();
                if value.trim().is_empty()
                    || value.len() > if $kind == "cursor" { 16 * 1024 } else { 512 }
                    || value.contains('\0')
                {
                    return Err(ThreadStoreError::InvalidId { kind: $kind, value });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ThreadStoreError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ThreadStoreError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

string_id!(ThreadId, "thread_id");
string_id!(RolloutId, "rollout_id");
string_id!(TurnId, "turn_id");
string_id!(ItemId, "item_id");
string_id!(HistoryRevision, "history_revision");
string_id!(OpaqueCursor, "cursor");

/// A stable boundary in an uncompressed rollout stream.
///
/// `end_ordinal_exclusive` and `end_byte_offset` must both land after a
/// newline-terminated JSONL record. Compression changes the file representation
/// but not this logical position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPosition {
    pub rollout_id: RolloutId,
    pub end_ordinal_exclusive: u64,
    pub end_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RolloutRecord {
    pub timestamp_ms: i64,
    pub ordinal: u64,
    #[serde(flatten)]
    pub record: CanonicalRecord,
}

/// A canonical rollout record. These types are deliberately independent from
/// frontend patches and provider-specific wire events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CanonicalRecord {
    SessionMeta(SessionMeta),
    TurnContext(TurnContext),
    ResponseItem(ResponseItem),
    Event(CanonicalEvent),
    Compacted(CompactionCheckpoint),
    Usage(UsageRecord),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub thread_id: ThreadId,
    pub rollout_id: RolloutId,
    pub created_at_ms: i64,
    pub source: String,
    /// Set only while the thread is created. Resuming a thread must read this
    /// frozen value instead of injecting a fresh system prompt.
    pub initial_system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_memory: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<ThreadId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_base: Option<HistoryPosition>,
    #[serde(default)]
    pub capabilities: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnContext {
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_turn_id: Option<TurnId>,
    pub model: String,
    #[serde(default)]
    pub execution_policy: Value,
}

/// A model-context item. It is intentionally separate from an `ItemCompleted`
/// display item because a context record cannot always be reconstructed from a
/// user-interface projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseItem {
    pub item_id: ItemId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_turn_id: Option<TurnId>,
    pub created_at_ms: i64,
    pub kind: CanonicalItemKind,
    pub payload: Value,
}

/// A completed item shown in thread history. `payload` is the canonical item
/// body. Command/MCP display output has an explicit field so the persistence
/// policy can bound that display copy without corrupting structured payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalItem {
    pub item_id: ItemId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_turn_id: Option<TurnId>,
    pub created_at_ms: i64,
    pub kind: CanonicalItemKind,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_output: Option<String>,
    #[serde(default)]
    pub display_output_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalItemKind {
    UserMessage,
    AssistantMessage,
    AssistantReasoning,
    Plan,
    ToolCall,
    ToolResult,
    CommandResult,
    McpResult,
    CustomToolResult,
    Image,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub enum CanonicalEvent {
    TurnStarted {
        turn_id: TurnId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root_turn_id: Option<TurnId>,
        started_at_ms: i64,
    },
    TurnCompleted {
        turn_id: TurnId,
        completed_at_ms: i64,
    },
    TurnAborted {
        turn_id: TurnId,
        aborted_at_ms: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
    },
    ItemCompleted(CanonicalItem),
    GoalUpdated(Value),
    Rollback {
        position: HistoryPosition,
    },
    SettingsApplied(Value),
    TokenCount(UsageRecord),
    // The following are live-only by policy. Keeping them in the canonical
    // type lets transports use one event vocabulary without making them facts.
    TextDelta {
        item_id: ItemId,
        text: String,
    },
    ReasoningDelta {
        item_id: ItemId,
        text: String,
    },
    PlanDelta {
        item_id: ItemId,
        text: String,
    },
    CommandOutputDelta {
        item_id: ItemId,
        text: String,
    },
    ItemStarted {
        item_id: ItemId,
    },
    ToolStarted {
        item_id: ItemId,
    },
    Progress(Value),
    Warning(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub turn_id: TurnId,
    pub summary: Value,
    pub retained_context: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadMetadata {
    pub thread_id: ThreadId,
    pub current_rollout_id: RolloutId,
    pub status: ThreadStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateThreadRequest {
    pub metadata: ThreadMetadata,
    pub session_meta: SessionMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<OpaqueCursor>,
    pub limit: u16,
}

impl PageRequest {
    pub const DEFAULT_THREAD_LIMIT: u16 = 50;
    pub const MAX_THREAD_LIMIT: u16 = 200;
    pub const DEFAULT_TURN_LIMIT: u16 = 20;
    pub const MAX_TURN_LIMIT: u16 = 100;
    pub const DEFAULT_ITEM_LIMIT: u16 = 100;
    pub const MAX_ITEM_LIMIT: u16 = 500;

    pub fn validate(&self, max: u16) -> ThreadStoreResult<()> {
        if self.limit == 0 || self.limit > max {
            return Err(ThreadStoreError::InvalidPageLimit {
                requested: self.limit,
                maximum: max,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub entries: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<OpaqueCursor>,
    pub history_revision: HistoryRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    pub page: PageRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnListQuery {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<OpaqueCursor>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemListQuery {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<OpaqueCursor>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnSummary {
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_turn_id: Option<TurnId>,
    pub rollout_ordinal: u64,
    pub status: TurnCompletionStatus,
    pub item_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnCompletionStatus {
    Running,
    Completed,
    Aborted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub item: CanonicalItem,
    pub rollout_ordinal: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemContentRequest {
    pub thread_id: ThreadId,
    pub item_id: ItemId,
    pub offset_bytes: u64,
    pub max_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemContentPage {
    pub item_id: ItemId,
    pub offset_bytes: u64,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppendOutcome {
    pub rollout_id: RolloutId,
    pub persisted_through: HistoryPosition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projected_through: Option<HistoryPosition>,
    pub projection_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelContext {
    pub thread_id: ThreadId,
    pub history_revision: HistoryRevision,
    pub records: Vec<RolloutRecord>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextRequest {
    pub thread_id: ThreadId,
    pub position: Option<HistoryPosition>,
    pub max_records: u32,
    pub max_bytes: u32,
}

/// Metadata reads are separate from history reads so thread directories never
/// need to deserialize a thread's complete history.
#[async_trait::async_trait]
pub trait ThreadMetadataStore: Send + Sync {
    async fn create_thread(
        &self,
        request: CreateThreadRequest,
    ) -> ThreadStoreResult<ThreadMetadata>;
    async fn get_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<ThreadMetadata>;
    async fn list_threads(&self, query: ThreadListQuery)
        -> ThreadStoreResult<Page<ThreadMetadata>>;
    async fn archive_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()>;
    async fn unarchive_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()>;
    async fn delete_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()>;
}

#[async_trait::async_trait]
pub trait ThreadHistoryReader: Send + Sync {
    async fn list_turns(&self, query: TurnListQuery) -> ThreadStoreResult<Page<TurnSummary>>;
    async fn list_items(&self, query: ItemListQuery) -> ThreadStoreResult<Page<ItemSummary>>;
    async fn read_item(
        &self,
        thread_id: &ThreadId,
        item_id: &ItemId,
    ) -> ThreadStoreResult<ItemSummary>;
    async fn read_item_content(
        &self,
        request: ItemContentRequest,
    ) -> ThreadStoreResult<ItemContentPage>;
    async fn load_model_context(
        &self,
        request: ModelContextRequest,
    ) -> ThreadStoreResult<ModelContext>;
}

/// The only contract allowed to append canonical history. A successful append
/// is durably present in the rollout; the SQLite projection may still lag and
/// is reported through `projection_pending`.
#[async_trait::async_trait]
pub trait ThreadStore: ThreadMetadataStore + ThreadHistoryReader {
    async fn append_records(
        &self,
        thread_id: &ThreadId,
        records: Vec<RolloutRecord>,
    ) -> ThreadStoreResult<AppendOutcome>;

    async fn flush(&self, thread_id: &ThreadId) -> ThreadStoreResult<AppendOutcome>;
    async fn shutdown(&self, thread_id: &ThreadId) -> ThreadStoreResult<()>;
}

pub type ThreadStoreResult<T> = Result<T, ThreadStoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadStoreError {
    InvalidId { kind: &'static str, value: String },
    InvalidPageLimit { requested: u16, maximum: u16 },
    NotFound { kind: &'static str, id: String },
    Conflict { message: String },
    HistoryChanged { current: HistoryRevision },
    ProjectionPending { rollout_id: RolloutId },
    Busy { message: String },
    CorruptHistory { message: String },
    Unavailable { message: String },
}

impl fmt::Display for ThreadStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { kind, value } => write!(f, "invalid {kind}: {value:?}"),
            Self::InvalidPageLimit { requested, maximum } => {
                write!(f, "invalid page limit {requested}; maximum is {maximum}")
            }
            Self::NotFound { kind, id } => write!(f, "{kind} not found: {id}"),
            Self::Conflict { message }
            | Self::Busy { message }
            | Self::CorruptHistory { message }
            | Self::Unavailable { message } => f.write_str(message),
            Self::HistoryChanged { current } => {
                write!(f, "history changed; current revision is {current}")
            }
            Self::ProjectionPending { rollout_id } => {
                write!(f, "history projection is pending for rollout {rollout_id}")
            }
        }
    }
}

impl Error for ThreadStoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_ids_reject_blank_values_and_round_trip_through_json() {
        assert!(ThreadId::new("  ").is_err());
        assert!(ThreadId::new("a".repeat(513)).is_err());
        assert!(ThreadId::new("a\0b").is_err());
        assert!(ThreadId::new("a".repeat(512)).is_ok());
        assert!(OpaqueCursor::new("a".repeat(1024)).is_ok());
        assert!(OpaqueCursor::new("a".repeat(16 * 1024 + 1)).is_err());
        assert!(serde_json::from_str::<ThreadId>("\"  \"").is_err());
        let id = ThreadId::new("thread-a").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"thread-a\"");
        let decoded: ThreadId = serde_json::from_str("\"thread-a\"").unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn page_limits_are_bounded() {
        assert!(PageRequest {
            cursor: None,
            limit: 0
        }
        .validate(PageRequest::MAX_ITEM_LIMIT)
        .is_err());
        assert!(PageRequest {
            cursor: None,
            limit: 501
        }
        .validate(PageRequest::MAX_ITEM_LIMIT)
        .is_err());
        assert!(PageRequest {
            cursor: None,
            limit: 500
        }
        .validate(PageRequest::MAX_ITEM_LIMIT)
        .is_ok());
    }
}
