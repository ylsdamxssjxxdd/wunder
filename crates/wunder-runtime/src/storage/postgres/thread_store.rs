//! PostgreSQL canonical thread history.
//!
//! The desktop/local profile keeps using `LocalThreadStore`.  The service
//! profile can use this implementation to keep thread metadata and canonical
//! records in the same PostgreSQL control plane.  Records are stored as one
//! canonical JSON document per ordinal; this keeps the append contract identical to
//! the rollout representation while allowing PostgreSQL to page and index
//! history without a local SQLite database or a bind-mounted JSONL file.

use super::{PgConn, PostgresStorage};
use crate::storage::rollout::{ROLLOUT_MAX_APPEND_BYTES, ROLLOUT_MAX_RECORD_BYTES};
use crate::storage::thread_store::{compact_model_context_payload, compact_replayed_model_message};
use crate::storage::StorageLifecycle;
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use wunder_core::storage_policy::prepare_for_persistence;
use wunder_core::thread_store::*;

#[path = "thread_stress_store.rs"]
mod stress;

const MAX_PAGE: u16 = 500;
const MAX_ITEM_PAGE_BYTES: usize = ROLLOUT_MAX_APPEND_BYTES;
const MAX_CONTEXT_RECORDS: u32 = 8192;
const MAX_CONTEXT_BYTES: u32 = 16 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct PostgresThreadStore {
    storage: Arc<PostgresStorage>,
}

impl PostgresThreadStore {
    pub(crate) fn storage_clone(&self) -> Arc<PostgresStorage> {
        Arc::clone(&self.storage)
    }
    pub(crate) async fn append_model_message(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        payload: serde_json::Value,
    ) -> ThreadStoreResult<AppendOutcome> {
        let now = Utc::now().timestamp_millis();
        let replacement = payload
            .pointer("/meta/replacement_history")
            .and_then(serde_json::Value::as_array)
            .cloned();
        // Normal response items are the durable model-context projection.
        // Keep protocol fields only; the visible/audit item remains written by
        // its separate completion event and retains diagnostics there.
        let payload = if replacement.is_some() {
            payload
        } else {
            compact_model_context_payload(payload)
        };
        let record = if let Some(replacement) = replacement.as_ref() {
            CanonicalRecord::Compacted(CompactionCheckpoint {
                turn_id: turn.clone(),
                summary: payload.get("content").cloned().unwrap_or_default(),
                retained_context: serde_json::Value::Array(replacement.clone()),
            })
        } else {
            let kind = match payload.get("role").and_then(serde_json::Value::as_str) {
                Some("assistant") => CanonicalItemKind::AssistantMessage,
                Some("tool") => CanonicalItemKind::ToolResult,
                _ => CanonicalItemKind::UserMessage,
            };
            CanonicalRecord::ResponseItem(ResponseItem {
                item_id: ItemId::new(uuid::Uuid::new_v4().to_string())?,
                turn_id: turn.clone(),
                root_turn_id: None,
                created_at_ms: now,
                kind,
                payload,
            })
        };
        self.append_canonical(thread, vec![record]).await
    }

    /// Shared application append contract. A caller must initialize frozen
    /// SessionMeta before writing history; this path never manufactures a
    /// blank system prompt or an implicit new execution.
    pub(crate) async fn append_chat_payload(
        &self,
        user: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        use crate::services::chat_payload_sanitizer::sanitize_persisted_chat_payload;
        use crate::services::output_quality::annotate_chat_payload;

        let session = payload
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("chat append requires a thread identity"))?;
        let thread = ThreadId::new(session)?;
        // The initial prompt is immutable once SessionMeta exists. An
        // uninitialized append must fail before admission or file creation.
        self.storage_session_meta(&thread)
            .await
            .context("chat append requires initialized frozen session metadata")?;
        let admissions = self.admission_store();
        let owner = user.to_owned();
        let id = session.to_owned();
        let input = payload.clone();
        let admitted = db(move || {
            admissions.check_owner(&owner, &id)?;
            let turn = if let Some(turn) = input
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            {
                turn.to_owned()
            } else {
                let round = input
                    .get("user_round")
                    .and_then(serde_json::Value::as_i64)
                    .filter(|round| *round > 0)
                    .ok_or_else(|| anyhow!("chat append requires an admitted turn identity"))?;
                admissions
                    .find_turn(&id, round)?
                    .ok_or_else(|| anyhow!("admitted chat turn not found"))?
            };
            admissions
                .turn(&id, &turn)?
                .ok_or_else(|| anyhow!("admitted chat turn not found"))
        })
        .await?;
        let turn = TurnId::new(
            admitted["turn_id"]
                .as_str()
                .ok_or_else(|| anyhow!("admitted chat turn has no identity"))?,
        )?;
        let root = admitted["root_turn_id"]
            .as_str()
            .map(TurnId::new)
            .transpose()?;
        let payload = sanitize_persisted_chat_payload(&annotate_chat_payload(payload));
        let now = Utc::now().timestamp_millis();
        let checkpoint = payload
            .pointer("/meta/replacement_history")
            .and_then(serde_json::Value::as_array);
        let model_message = crate::services::history::build_message_from_item(&payload, true);
        let role = payload
            .get("role")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("chat append requires a message role"))?;
        let kind = match role {
            "user" => CanonicalItemKind::UserMessage,
            "assistant" => CanonicalItemKind::AssistantMessage,
            "tool" => CanonicalItemKind::ToolResult,
            "system" if checkpoint.is_some() => CanonicalItemKind::Configuration,
            _ => bail!("chat append has an unsupported message role"),
        };
        let item_id = ItemId::new(
            payload
                .get("item_id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        )?;
        match self.read_item(&thread, &item_id).await {
            Ok(_) => return Ok(()),
            Err(ThreadStoreError::NotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
        let mut records = Vec::with_capacity(2);
        if let Some(checkpoint) = checkpoint {
            records.push(CanonicalRecord::Compacted(CompactionCheckpoint {
                turn_id: turn.clone(),
                summary: payload.get("content").cloned().unwrap_or_default(),
                retained_context: serde_json::Value::Array(checkpoint.clone()),
            }));
        } else if let Some(model_message) = model_message {
            records.push(CanonicalRecord::ResponseItem(ResponseItem {
                item_id: ItemId::new(format!("{}:model", item_id.as_str()))?,
                turn_id: turn.clone(),
                root_turn_id: root.clone(),
                created_at_ms: now,
                kind,
                payload: model_message,
            }));
        }
        records.push(CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
            CanonicalItem {
                item_id,
                turn_id: turn,
                root_turn_id: root,
                created_at_ms: now,
                kind,
                payload,
                display_output: None,
                display_output_truncated: false,
            },
        )));
        self.append_canonical(&thread, records).await?;
        Ok(())
    }

    pub(crate) async fn model_messages(
        &self,
        thread: &ThreadId,
        excluding_turn: Option<&TurnId>,
    ) -> ThreadStoreResult<Vec<serde_json::Value>> {
        let context = self
            .load_model_context(ModelContextRequest {
                thread_id: thread.clone(),
                position: None,
                max_records: MAX_CONTEXT_RECORDS,
                max_bytes: MAX_CONTEXT_BYTES,
            })
            .await?;
        if context.truncated {
            return Err(ThreadStoreError::CorruptHistory{message:"model context exceeds its read budget; an explicit context checkpoint is required".into()});
        }
        let mut messages = Vec::new();
        for record in context.records {
            match record.record {
                CanonicalRecord::Compacted(checkpoint) => {
                    messages.clear();
                    messages.extend(
                        checkpoint
                            .retained_context
                            .as_array()
                            .into_iter()
                            .flatten()
                            .cloned()
                            .filter_map(compact_replayed_model_message),
                    );
                }
                CanonicalRecord::ResponseItem(item) if excluding_turn != Some(&item.turn_id) => {
                    // Re-apply the projection at read time as well.  Stress
                    // fixtures, imported rollouts and older writers can
                    // contain a full event payload even though new appends
                    // are compacted before persistence.  Keeping this boundary
                    // deterministic prevents audit metadata from entering a
                    // resumed model request and preserves prompt-cache hits.
                    if let Some(message) = compact_replayed_model_message(item.payload) {
                        messages.push(message);
                    }
                }
                _ => {}
            }
        }
        Ok(messages)
    }

    pub(crate) async fn begin_turn(
        &self,
        user: &str,
        thread: &ThreadId,
        turn: &TurnId,
        model: &str,
    ) -> ThreadStoreResult<()> {
        let admissions = self.admission_store();
        let user = user.to_owned();
        let thread = thread.to_string();
        let turn = turn.to_string();
        let model = model.to_owned();
        db(move || admissions.begin_turn(&user, &thread, &turn, &model)).await
    }

    pub(crate) async fn append_canonical(
        &self,
        thread: &ThreadId,
        records: Vec<CanonicalRecord>,
    ) -> ThreadStoreResult<AppendOutcome> {
        let now = Utc::now().timestamp_millis();
        self.append_records(
            thread,
            records
                .into_iter()
                .map(|record| RolloutRecord {
                    timestamp_ms: now,
                    ordinal: 0,
                    record,
                })
                .collect(),
        )
        .await
    }

    pub(crate) fn admission_store(&self) -> crate::storage::postgres::PostgresAdmissionStore {
        crate::storage::postgres::PostgresAdmissionStore::new(self.storage.clone())
    }
    pub(crate) fn new(storage: Arc<PostgresStorage>) -> Result<Self> {
        let store = Self { storage };
        store.ensure_schema()?;
        Ok(store)
    }

    fn ensure_schema(&self) -> Result<()> {
        self.storage.ensure_initialized()?;
        use sha2::{Digest, Sha256};
        const MARKER: &str = "postgres_thread_schema_fingerprint_v1";
        static FINGERPRINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let fingerprint = FINGERPRINT.get_or_init(|| {
            let mut hash = Sha256::new();
            for source in [
                include_str!("thread_store.rs"),
                include_str!("thread_display.rs"),
            ] {
                hash.update(source.as_bytes());
            }
            format!("{:x}", hash.finalize())
        });
        let mut conn = self.storage.conn()?;
        if conn
            .query_opt("SELECT value FROM meta WHERE key=$1", &[&MARKER])?
            .is_some_and(|row| row.get::<_, String>(0) == *fingerprint)
        {
            return Ok(());
        }
        let mut tx = conn.transaction()?;
        // IF NOT EXISTS does not serialize competing catalog inserts. Hold a
        // schema-scoped transaction lock through all service index DDL.
        tx.query_one(
            "SELECT pg_advisory_xact_lock(1937202, hashtext(current_schema()))",
            &[],
        )?;
        // A different replica may already have finished while this one waited.
        // Repeating index DDL can deadlock against ordinary canonical writes,
        // even when every CREATE includes IF NOT EXISTS.
        if tx
            .query_opt("SELECT value FROM meta WHERE key=$1", &[&MARKER])?
            .is_some_and(|row| row.get::<_, String>(0) == *fingerprint)
        {
            tx.commit()?;
            return Ok(());
        }
        tx.batch_execute(
            r#"
            CREATE TABLE IF NOT EXISTS service_threads (
                thread_id TEXT PRIMARY KEY,
                rollout_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at_ms BIGINT NOT NULL,
                updated_at_ms BIGINT NOT NULL,
                title TEXT,
                workspace_id TEXT,
                pinned BOOLEAN NOT NULL DEFAULT FALSE,
                history_revision BIGINT NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS service_threads_directory
                ON service_threads (updated_at_ms DESC, thread_id DESC);
            CREATE INDEX IF NOT EXISTS service_threads_workspace
                ON service_threads (workspace_id, updated_at_ms DESC, thread_id DESC);
            CREATE TABLE IF NOT EXISTS service_thread_records (
                thread_id TEXT NOT NULL REFERENCES service_threads(thread_id) ON DELETE CASCADE,
                ordinal BIGINT NOT NULL,
                timestamp_ms BIGINT NOT NULL,
                record_json TEXT NOT NULL,
                record_type TEXT NOT NULL,
                turn_id TEXT,
                item_id TEXT,
                PRIMARY KEY (thread_id, ordinal)
            );
            CREATE INDEX IF NOT EXISTS service_thread_records_turn
                ON service_thread_records (thread_id, turn_id, ordinal);
            CREATE INDEX IF NOT EXISTS service_thread_records_item
                ON service_thread_records (thread_id, item_id);
            CREATE INDEX IF NOT EXISTS service_thread_records_type
                ON service_thread_records (thread_id, record_type, ordinal);
            CREATE INDEX IF NOT EXISTS service_thread_context_records
                ON service_thread_records (thread_id, ordinal DESC)
                WHERE record_type IN ('response_item','compacted');
            CREATE TABLE IF NOT EXISTS service_thread_sessions (
                thread_id TEXT PRIMARY KEY REFERENCES service_threads(thread_id) ON DELETE CASCADE,
                rollout_id TEXT NOT NULL,
                session_json TEXT NOT NULL
            );
            "#,
        )?;
        super::thread_display::initialize(&mut tx)?;
        tx.execute(
            "INSERT INTO meta(key,value,updated_time) VALUES($1,$2,$3) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_time=excluded.updated_time",
            &[&MARKER, fingerprint, &PostgresStorage::now_ts()],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn map_error(error: anyhow::Error) -> ThreadStoreError {
        ThreadStoreError::Unavailable {
            message: format!("postgres thread store: {error:#}"),
        }
    }

    fn history_before(
        cursor: Option<&OpaqueCursor>,
        thread: &ThreadId,
        rollout: &RolloutId,
        revision: i64,
        kind: &str,
        turn: Option<&TurnId>,
    ) -> ThreadStoreResult<i64> {
        let Some(cursor) = cursor else {
            return Ok(revision);
        };
        let cursor: HistoryCursor =
            serde_json::from_str(cursor.as_str()).map_err(|_| ThreadStoreError::Conflict {
                message: "invalid PostgreSQL history cursor".into(),
            })?;
        if cursor.thread != thread.as_str()
            || cursor.rollout != rollout.as_str()
            || cursor.kind != kind
            || cursor.turn.as_deref() != turn.map(TurnId::as_str)
            || cursor.before < 0
            || cursor.before > revision
        {
            return Err(ThreadStoreError::Conflict {
                message: "history cursor does not match the requested page".into(),
            });
        }
        if cursor.revision != revision {
            return Err(ThreadStoreError::HistoryChanged {
                current: Self::revision(revision)?,
            });
        }
        Ok(cursor.before)
    }
    fn history_cursor(
        thread: &ThreadId,
        rollout: &RolloutId,
        revision: i64,
        kind: &str,
        turn: Option<&TurnId>,
        before: i64,
    ) -> ThreadStoreResult<OpaqueCursor> {
        let raw = serde_json::to_string(&HistoryCursor {
            thread: thread.to_string(),
            rollout: rollout.to_string(),
            revision,
            kind: kind.into(),
            turn: turn.map(ToString::to_string),
            before,
        })
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })?;
        OpaqueCursor::new(raw)
    }

    fn revision(value: i64) -> ThreadStoreResult<HistoryRevision> {
        HistoryRevision::new(value.max(0).to_string()).map_err(|e| e)
    }

    fn cursor(value: i64) -> ThreadStoreResult<OpaqueCursor> {
        OpaqueCursor::new(value.max(0).to_string()).map_err(|e| e)
    }

    fn status_name(status: ThreadStatus) -> &'static str {
        match status {
            ThreadStatus::Active => "active",
            ThreadStatus::Archived => "archived",
        }
    }

    fn decode_status(value: &str) -> ThreadStatus {
        if value.eq_ignore_ascii_case("archived") {
            ThreadStatus::Archived
        } else {
            ThreadStatus::Active
        }
    }

    fn metadata_from_row(row: &tokio_postgres::Row) -> ThreadMetadata {
        let status: String = row.get("status");
        ThreadMetadata {
            thread_id: ThreadId::new(row.get::<_, String>("thread_id")).expect("validated id"),
            current_rollout_id: RolloutId::new(row.get::<_, String>("rollout_id"))
                .expect("validated id"),
            status: Self::decode_status(&status),
            created_at_ms: row.get("created_at_ms"),
            updated_at_ms: row.get("updated_at_ms"),
            title: row.get("title"),
            workspace_id: row.get("workspace_id"),
            pinned: row.get("pinned"),
        }
    }

    pub(super) fn record_type(
        record: &CanonicalRecord,
    ) -> (&'static str, Option<String>, Option<String>) {
        match record {
            CanonicalRecord::SessionMeta(_) => ("session_meta", None, None),
            CanonicalRecord::TurnContext(context) => {
                ("turn_context", Some(context.turn_id.to_string()), None)
            }
            CanonicalRecord::ResponseItem(item) => (
                "response_item",
                Some(item.turn_id.to_string()),
                Some(item.item_id.to_string()),
            ),
            CanonicalRecord::Compacted(checkpoint) => {
                ("compacted", Some(checkpoint.turn_id.to_string()), None)
            }
            CanonicalRecord::Usage(usage) => (
                "usage",
                usage.turn_id.as_ref().map(ToString::to_string),
                None,
            ),
            CanonicalRecord::Event(event) => match event {
                CanonicalEvent::TurnStarted { turn_id, .. } => {
                    ("turn_started", Some(turn_id.to_string()), None)
                }
                CanonicalEvent::TurnCompleted { turn_id, .. } => {
                    ("turn_completed", Some(turn_id.to_string()), None)
                }
                CanonicalEvent::TurnAborted { turn_id, .. } => {
                    ("turn_aborted", Some(turn_id.to_string()), None)
                }
                CanonicalEvent::ItemCompleted(item) => (
                    "item_completed",
                    Some(item.turn_id.to_string()),
                    Some(item.item_id.to_string()),
                ),
                CanonicalEvent::TextDelta { item_id, .. }
                | CanonicalEvent::ReasoningDelta { item_id, .. }
                | CanonicalEvent::PlanDelta { item_id, .. }
                | CanonicalEvent::CommandOutputDelta { item_id, .. }
                | CanonicalEvent::ItemStarted { item_id }
                | CanonicalEvent::ToolStarted { item_id } => {
                    ("event", None, Some(item_id.to_string()))
                }
                _ => ("event", None, None),
            },
        }
    }

    fn decode_record(row: &tokio_postgres::Row) -> ThreadStoreResult<RolloutRecord> {
        let value: Value = serde_json::from_str(row.get::<_, String>("record_json").as_str())
            .map_err(|error| ThreadStoreError::CorruptHistory {
                message: format!("invalid postgres canonical record: {error}"),
            })?;
        serde_json::from_value(value).map_err(|error| ThreadStoreError::CorruptHistory {
            message: format!("invalid postgres canonical record: {error}"),
        })
    }

    fn missing(kind: &'static str, id: impl Into<String>) -> ThreadStoreError {
        ThreadStoreError::NotFound {
            kind,
            id: id.into(),
        }
    }

    fn item_from_record(record: &RolloutRecord) -> Option<ItemSummary> {
        let item = match &record.record {
            CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) => item.clone(),
            _ => return None,
        };
        Some(ItemSummary {
            item,
            rollout_ordinal: record.ordinal,
        })
    }

    fn item_content(
        item: &ItemSummary,
        offset: u64,
        max: u32,
    ) -> ThreadStoreResult<ItemContentPage> {
        let text = item
            .item
            .display_output
            .clone()
            .or_else(|| {
                item.item
                    .payload
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                item.item
                    .payload
                    .pointer("/payload/content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| serde_json::to_string(&item.item.payload).unwrap_or_default());
        if max == 0 || max > 64 * 1024 {
            return Err(ThreadStoreError::Conflict {
                message: "invalid content page budget".into(),
            });
        }
        let start = usize::try_from(offset).map_err(unavailable)?;
        if start > text.len() || !text.is_char_boundary(start) {
            return Err(ThreadStoreError::Conflict {
                message: "invalid UTF-8 content offset".into(),
            });
        }
        let mut end = start.saturating_add(max as usize).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start && start < text.len() {
            return Err(ThreadStoreError::Conflict {
                message: "content page budget cannot fit the next UTF-8 character".into(),
            });
        }
        let content = text[start..end].to_owned();
        Ok(ItemContentPage {
            item_id: item.item.item_id.clone(),
            offset_bytes: start as u64,
            content,
            next_offset_bytes: (end < text.len()).then_some(end as u64),
        })
    }

    fn current_metadata(
        &self,
        thread: &ThreadId,
        conn: &mut PgConn<'_>,
    ) -> ThreadStoreResult<(ThreadMetadata, i64)> {
        let row = conn
            .query_opt("SELECT thread_id,rollout_id,status,created_at_ms,updated_at_ms,title,workspace_id,pinned,history_revision FROM service_threads WHERE thread_id=$1", &[&thread.as_str()])
            .map_err(Self::map_error)?
            .ok_or_else(|| Self::missing("thread", thread.to_string()))?;
        Ok((Self::metadata_from_row(&row), row.get("history_revision")))
    }

    fn append_sync(
        &self,
        thread: &ThreadId,
        records: Vec<RolloutRecord>,
    ) -> ThreadStoreResult<AppendOutcome> {
        if records.len() > 512 {
            return Err(ThreadStoreError::Conflict {
                message: "canonical append exceeds record budget".into(),
            });
        }
        let mut prepared = Vec::with_capacity(records.len());
        let mut bytes = 0usize;
        for record in records {
            if matches!(record.record, CanonicalRecord::SessionMeta(_)) {
                return Err(ThreadStoreError::Conflict {
                    message: "session metadata is frozen at thread creation".into(),
                });
            }
            if let Some(record) = prepare_for_persistence(&record) {
                let size = serde_json::to_vec(&record)
                    .map_err(|e| ThreadStoreError::Unavailable {
                        message: e.to_string(),
                    })?
                    .len();
                if size > ROLLOUT_MAX_RECORD_BYTES {
                    return Err(ThreadStoreError::Conflict {
                        message: "canonical record exceeds byte budget".into(),
                    });
                }
                bytes = bytes.saturating_add(size);
                prepared.push(record);
            }
        }
        if prepared.len() > 1 && bytes > ROLLOUT_MAX_APPEND_BYTES {
            return Err(ThreadStoreError::Conflict {
                message: "canonical append exceeds byte budget".into(),
            });
        }
        let records = prepared;
        if records.is_empty() {
            return self.flush_sync(thread);
        }
        super::thread_display::ensure_ready(&self.storage, thread.as_str())
            .map_err(Self::map_error)?;
        let mut conn = self.storage.conn().map_err(Self::map_error)?;
        let mut tx = conn.transaction().map_err(Self::map_error)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&thread.as_str()],
        )
        .map_err(Self::map_error)?;
        let row = tx.query_opt("SELECT rollout_id,status,history_revision FROM service_threads WHERE thread_id=$1 FOR UPDATE", &[&thread.as_str()]).map_err(Self::map_error)?.ok_or_else(|| Self::missing("thread", thread.to_string()))?;
        let rollout_id: String = row.get("rollout_id");
        if row.get::<_, String>("status") == "archived" {
            return Err(ThreadStoreError::Conflict {
                message: "cannot append to an archived thread".into(),
            });
        }
        let mut max_ordinal = row.get::<_, i64>("history_revision");
        let mut display_records = Vec::with_capacity(records.len());
        for mut record in records {
            // Every caller uses the same transaction allocator, including retries.
            // Completed semantic item identities are immutable and deduplicated
            // before writing their paired model-context record.
            let (_, _, item_id) = Self::record_type(&record.record);
            if let Some(item_id) = item_id.as_deref() {
                let kind = Self::record_type(&record.record).0;
                if matches!(kind, "item_completed" | "response_item") && tx.query_opt(
                    "SELECT 1 FROM service_thread_records WHERE thread_id=$1 AND item_id=$2 AND record_type=$3 LIMIT 1",
                    &[&thread.as_str(), &item_id, &kind],
                ).map_err(Self::map_error)?.is_some() { continue; }
            }
            record.ordinal = max_ordinal as u64;
            let ordinal = max_ordinal;
            let value =
                serde_json::to_value(&record).map_err(|e| ThreadStoreError::Unavailable {
                    message: e.to_string(),
                })?;
            let (kind, turn_id, item_id) = Self::record_type(&record.record);
            let value_text =
                serde_json::to_string(&value).map_err(|e| ThreadStoreError::Unavailable {
                    message: e.to_string(),
                })?;
            let inserted = tx.execute("INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json,record_type,turn_id,item_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(thread_id,ordinal) DO NOTHING", &[&thread.as_str(), &ordinal, &record.timestamp_ms, &value_text, &kind, &turn_id, &item_id]).map_err(Self::map_error)?;
            if inserted == 0 {
                let existing = tx.query_one("SELECT record_json FROM service_thread_records WHERE thread_id=$1 AND ordinal=$2", &[&thread.as_str(), &ordinal]).map_err(Self::map_error)?.get::<_, String>(0);
                if existing != value_text {
                    return Err(ThreadStoreError::Conflict {
                        message: format!("ordinal {ordinal} already contains a different record"),
                    });
                }
            }
            max_ordinal = max_ordinal.max(ordinal.saturating_add(1));
            if inserted > 0 {
                display_records.push(record);
            }
        }
        super::thread_display::project_batch(
            &mut tx,
            thread.as_str(),
            &display_records,
            max_ordinal,
        )
        .map_err(Self::map_error)?;
        let now = Utc::now().timestamp_millis();
        tx.execute("UPDATE service_threads SET updated_at_ms=$2,history_revision=GREATEST(history_revision,$3) WHERE thread_id=$1", &[&thread.as_str(), &now, &max_ordinal]).map_err(Self::map_error)?;
        tx.commit().map_err(Self::map_error)?;
        let rollout = RolloutId::new(rollout_id).map_err(|e| e)?;
        Ok(AppendOutcome {
            rollout_id: rollout.clone(),
            persisted_through: HistoryPosition {
                rollout_id: rollout.clone(),
                end_ordinal_exclusive: max_ordinal as u64,
                end_byte_offset: max_ordinal as u64,
            },
            projected_through: Some(HistoryPosition {
                rollout_id: rollout,
                end_ordinal_exclusive: max_ordinal as u64,
                end_byte_offset: max_ordinal as u64,
            }),
            projection_pending: false,
        })
    }

    fn flush_sync(&self, thread: &ThreadId) -> ThreadStoreResult<AppendOutcome> {
        let mut conn = self.storage.conn().map_err(Self::map_error)?;
        let (meta, revision) = self.current_metadata(thread, &mut conn)?;
        let rollout = meta.current_rollout_id.clone();
        Ok(AppendOutcome {
            rollout_id: rollout.clone(),
            persisted_through: HistoryPosition {
                rollout_id: rollout.clone(),
                end_ordinal_exclusive: revision as u64,
                end_byte_offset: revision as u64,
            },
            projected_through: Some(HistoryPosition {
                rollout_id: rollout,
                end_ordinal_exclusive: revision as u64,
                end_byte_offset: revision as u64,
            }),
            projection_pending: false,
        })
    }
}

#[async_trait::async_trait]
impl ThreadMetadataStore for PostgresThreadStore {
    async fn create_thread(
        &self,
        request: CreateThreadRequest,
    ) -> ThreadStoreResult<ThreadMetadata> {
        if request.session_meta.thread_id != request.metadata.thread_id
            || request.session_meta.rollout_id != request.metadata.current_rollout_id
        {
            return Err(ThreadStoreError::Conflict {
                message: "session metadata identity mismatch".into(),
            });
        }
        if request.session_meta.history_base.is_some() {
            return Err(ThreadStoreError::Conflict {
                message: "lineage creation requires an explicit fork operation".into(),
            });
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            let mut tx = conn.transaction().map_err(Self::map_error)?;
            tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", &[&request.metadata.thread_id.as_str()]).map_err(Self::map_error)?;
            let metadata = request.metadata;
            let session = request.session_meta;
            let session_json = serde_json::to_value(&session).map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?;
            let inserted = tx.execute("INSERT INTO service_threads(thread_id,rollout_id,status,created_at_ms,updated_at_ms,title,workspace_id,pinned,history_revision) VALUES($1,$2,$3,$4,$5,$6,$7,$8,1) ON CONFLICT(thread_id) DO NOTHING", &[&metadata.thread_id.as_str(), &metadata.current_rollout_id.as_str(), &Self::status_name(metadata.status), &metadata.created_at_ms, &metadata.updated_at_ms, &metadata.title, &metadata.workspace_id, &metadata.pinned]).map_err(Self::map_error)?;
            if inserted == 0 { return Err(ThreadStoreError::Conflict { message: "thread already exists".into() }); }
            let session_text = serde_json::to_string(&session_json).map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?;
            tx.execute("INSERT INTO service_thread_sessions(thread_id,rollout_id,session_json) VALUES($1,$2,$3)", &[&metadata.thread_id.as_str(), &session.rollout_id.as_str(), &session_text]).map_err(Self::map_error)?;
            let record = RolloutRecord { timestamp_ms: session.created_at_ms, ordinal: 0, record: CanonicalRecord::SessionMeta(session) };
            let record_json = serde_json::to_string(&record).map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?;
            if record_json.len() > ROLLOUT_MAX_RECORD_BYTES { return Err(ThreadStoreError::Conflict { message: "session metadata exceeds byte budget".into() }); }
            tx.execute("INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json,record_type) VALUES($1,0,$2,$3,'session_meta')", &[&metadata.thread_id.as_str(),&record.timestamp_ms,&record_json]).map_err(Self::map_error)?;
            super::thread_display::project_batch(&mut tx, metadata.thread_id.as_str(), &[record], 1).map_err(Self::map_error)?;
            tx.commit().map_err(Self::map_error)?;
            Ok(metadata)
        }).await.map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?
    }

    async fn get_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<ThreadMetadata> {
        let this = self.clone();
        let id = thread_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            this.current_metadata(&id, &mut conn).map(|v| v.0)
        })
        .await
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })?
    }

    async fn list_threads(
        &self,
        query: ThreadListQuery,
    ) -> ThreadStoreResult<Page<ThreadMetadata>> {
        query.page.validate(PageRequest::MAX_THREAD_LIMIT)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            let before: Option<(i64,String)> = query.page.cursor.as_ref().map(|cursor| serde_json::from_str(cursor.as_str()).map_err(|_| ThreadStoreError::Conflict { message: "invalid thread directory cursor".into() })).transpose()?;
            let at = before.as_ref().map(|v|v.0);
            let id = before.as_ref().map(|v|v.1.as_str());
            let status = query.archived.map(|archived| if archived { "archived" } else { "active" });
            let rows = conn.query("SELECT thread_id,rollout_id,status,created_at_ms,updated_at_ms,title,workspace_id,pinned FROM service_threads WHERE ($1::text IS NULL OR workspace_id=$1) AND ($2::text IS NULL OR status=$2) AND ($3::bigint IS NULL OR (updated_at_ms,thread_id)<($3,$4::text)) ORDER BY updated_at_ms DESC,thread_id DESC LIMIT $5", &[&query.workspace_id, &status, &at, &id, &(i64::from(query.page.limit)+1)]).map_err(Self::map_error)?;
            let more = rows.len() > query.page.limit as usize;
            let entries = rows.iter().take(query.page.limit as usize).map(Self::metadata_from_row).collect::<Vec<_>>();
            let next_cursor = if more { entries.last().map(|meta| OpaqueCursor::new(serde_json::to_string(&(meta.updated_at_ms,meta.thread_id.as_str())).expect("serializable directory position"))).transpose()? } else { None };
            Ok(Page { entries, next_cursor, history_revision: Self::revision(Utc::now().timestamp_millis())? })
        }).await.map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?
    }

    async fn archive_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()> {
        self.set_status(thread_id, true).await
    }
    async fn unarchive_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()> {
        self.set_status(thread_id, false).await
    }
    async fn delete_thread(&self, thread_id: &ThreadId) -> ThreadStoreResult<()> {
        let this = self.clone();
        let id = thread_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = this.storage.conn().map_err(Self::map_error)?;
            let mut tx = c.transaction().map_err(Self::map_error)?;
            tx.query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&id.as_str()],
            )
            .map_err(Self::map_error)?;
            let n = tx
                .execute(
                    "DELETE FROM service_threads WHERE thread_id=$1",
                    &[&id.as_str()],
                )
                .map_err(Self::map_error)?;
            if n == 0 {
                return Err(Self::missing("thread", id.to_string()));
            }
            tx.execute(
                "DELETE FROM service_thread_admissions WHERE thread_id=$1",
                &[&id.as_str()],
            )
            .map_err(Self::map_error)?;
            tx.execute(
                "DELETE FROM service_thread_owners WHERE thread_id=$1",
                &[&id.as_str()],
            )
            .map_err(Self::map_error)?;
            tx.commit().map_err(Self::map_error)?;
            Ok(())
        })
        .await
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })?
    }
}

impl PostgresThreadStore {
    async fn set_status(&self, thread_id: &ThreadId, archived: bool) -> ThreadStoreResult<()> {
        let this = self.clone();
        let id = thread_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = this.storage.conn().map_err(Self::map_error)?;
            let n = c
                .execute(
                    "UPDATE service_threads SET status=$2,updated_at_ms=$3 WHERE thread_id=$1",
                    &[
                        &id.as_str(),
                        &if archived { "archived" } else { "active" },
                        &Utc::now().timestamp_millis(),
                    ],
                )
                .map_err(Self::map_error)?;
            if n == 0 {
                return Err(Self::missing("thread", id.to_string()));
            }
            Ok(())
        })
        .await
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })?
    }
}

#[async_trait::async_trait]
impl ThreadStore for PostgresThreadStore {
    async fn append_records(
        &self,
        thread_id: &ThreadId,
        records: Vec<RolloutRecord>,
    ) -> ThreadStoreResult<AppendOutcome> {
        let this = self.clone();
        let id = thread_id.clone();
        tokio::task::spawn_blocking(move || this.append_sync(&id, records))
            .await
            .map_err(|e| ThreadStoreError::Unavailable {
                message: e.to_string(),
            })?
    }
    async fn flush(&self, thread_id: &ThreadId) -> ThreadStoreResult<AppendOutcome> {
        let this = self.clone();
        let id = thread_id.clone();
        tokio::task::spawn_blocking(move || this.flush_sync(&id))
            .await
            .map_err(|e| ThreadStoreError::Unavailable {
                message: e.to_string(),
            })?
    }
    async fn shutdown(&self, _thread_id: &ThreadId) -> ThreadStoreResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl ThreadHistoryReader for PostgresThreadStore {
    async fn list_items(&self, query: ItemListQuery) -> ThreadStoreResult<Page<ItemSummary>> {
        validate_limit(query.limit, MAX_PAGE)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            let (meta, revision) = this.current_metadata(&query.thread_id, &mut conn)?;
            let before = Self::history_before(query.cursor.as_ref(), &query.thread_id, &meta.current_rollout_id, revision, "items", query.turn_id.as_ref())?;
            let turn = query.turn_id.as_ref().map(TurnId::as_str);
            // Fetch bounded scalar metadata before any bodies. A count limit
            // alone permits hundreds of maximum-size canonical records.
            let candidates = conn.query("SELECT ordinal,octet_length(record_json) AS bytes FROM service_thread_records WHERE thread_id=$1 AND record_type='item_completed' AND ordinal<$2 AND ($3::text IS NULL OR turn_id=$3) ORDER BY ordinal DESC LIMIT $4", &[&query.thread_id.as_str(), &before, &turn, &(i64::from(query.limit)+1)]).map_err(Self::map_error)?;
            let mut ordinals = Vec::with_capacity(query.limit as usize);
            let mut bytes = 0usize;
            for row in &candidates {
                let size = usize::try_from(row.get::<_, i32>("bytes")).map_err(|_| ThreadStoreError::CorruptHistory { message: "canonical item has an invalid byte length".into() })?;
                if size == 0 || size > ROLLOUT_MAX_RECORD_BYTES {
                    return Err(ThreadStoreError::CorruptHistory { message: "canonical item exceeds its record byte budget".into() });
                }
                if ordinals.len() >= query.limit as usize || (!ordinals.is_empty() && bytes.saturating_add(size) > MAX_ITEM_PAGE_BYTES) { break; }
                // One legal large record can occupy its own page; never skip
                // it or return an empty page with a nonadvancing cursor.
                bytes += size;
                ordinals.push(row.get::<_, i64>("ordinal"));
            }
            let has_more = ordinals.len() < candidates.len();
            let rows = if ordinals.is_empty() { Vec::new() } else {
                conn.query("SELECT ordinal,record_json FROM service_thread_records WHERE thread_id=$1 AND ordinal=ANY($2) AND octet_length(record_json)<=$3 ORDER BY ordinal DESC", &[&query.thread_id.as_str(), &ordinals, &(ROLLOUT_MAX_RECORD_BYTES as i32)]).map_err(Self::map_error)?
            };
            if rows.len() != ordinals.len() { return Err(ThreadStoreError::CorruptHistory { message: "canonical item page changed during its bounded read".into() }); }
            let entries = rows.into_iter().map(|row| {
                let record = Self::decode_record(&row)?;
                if record.ordinal != row.get::<_, i64>("ordinal") as u64 { return Err(ThreadStoreError::CorruptHistory { message: "canonical item ordinal does not match its index".into() }); }
                Self::item_from_record(&record).ok_or_else(|| ThreadStoreError::CorruptHistory { message: "item index points to a different record type".into() })
            }).collect::<ThreadStoreResult<Vec<_>>>()?;
            let next_cursor = if has_more { entries.last().map(|item| Self::history_cursor(&query.thread_id, &meta.current_rollout_id, revision, "items", query.turn_id.as_ref(), item.rollout_ordinal as i64)).transpose()? } else { None };
            Ok(Page { entries, next_cursor, history_revision: Self::revision(revision)? })
        }).await.map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?
    }
    async fn list_turns(&self, query: TurnListQuery) -> ThreadStoreResult<Page<TurnSummary>> {
        validate_limit(query.limit, 100)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            let (meta, revision) = this.current_metadata(&query.thread_id, &mut conn)?;
            let before = Self::history_before(query.cursor.as_ref(), &query.thread_id, &meta.current_rollout_id, revision, "turns", None)?;
            let rows = conn.query("SELECT s.record_json,s.ordinal,c.item_count,e.terminal_json FROM (
                SELECT record_json,ordinal,turn_id FROM service_thread_records WHERE thread_id=$1 AND record_type='turn_started' AND ordinal<$2 ORDER BY ordinal DESC LIMIT $3
                ) s LEFT JOIN LATERAL (SELECT COUNT(*) AS item_count FROM service_thread_records WHERE thread_id=$1 AND turn_id=s.turn_id AND record_type='item_completed' AND ordinal<$4) c ON TRUE
                LEFT JOIN LATERAL (SELECT record_json AS terminal_json FROM service_thread_records WHERE thread_id=$1 AND turn_id=s.turn_id AND record_type IN ('turn_completed','turn_aborted') AND ordinal<$4 ORDER BY ordinal DESC LIMIT 1) e ON TRUE
                ORDER BY s.ordinal DESC", &[&query.thread_id.as_str(), &before, &(i64::from(query.limit)+1), &revision]).map_err(Self::map_error)?;
            let has_more = rows.len() > query.limit as usize;
            let mut entries = Vec::with_capacity(query.limit as usize);
            for row in rows.into_iter().take(query.limit as usize) {
                let record = Self::decode_record(&row)?;
                let CanonicalRecord::Event(CanonicalEvent::TurnStarted { turn_id, root_turn_id, started_at_ms }) = record.record else {
                    return Err(ThreadStoreError::CorruptHistory { message: "turn index points to a different record type".into() });
                };
                let mut status = TurnCompletionStatus::Running;
                let mut completed_at_ms = None;
                if let Some(raw) = row.get::<_, Option<String>>("terminal_json") {
                    let terminal: RolloutRecord = serde_json::from_str(&raw).map_err(|e| ThreadStoreError::CorruptHistory { message: e.to_string() })?;
                    match terminal.record {
                        CanonicalRecord::Event(CanonicalEvent::TurnCompleted { completed_at_ms: at, .. }) => { status = TurnCompletionStatus::Completed; completed_at_ms = Some(at); },
                        CanonicalRecord::Event(CanonicalEvent::TurnAborted { aborted_at_ms, error, .. }) => { status = if error.is_some() { TurnCompletionStatus::Failed } else { TurnCompletionStatus::Aborted }; completed_at_ms = Some(aborted_at_ms); },
                        _ => {}
                    }
                }
                entries.push(TurnSummary { turn_id, root_turn_id, rollout_ordinal: record.ordinal, status, item_count: row.get::<_,i64>("item_count").min(u32::MAX as i64) as u32, started_at_ms: Some(started_at_ms), completed_at_ms });
            }
            let next_cursor = if has_more { entries.last().map(|turn| Self::history_cursor(&query.thread_id, &meta.current_rollout_id, revision, "turns", None, turn.rollout_ordinal as i64)).transpose()? } else { None };
            Ok(Page { entries, next_cursor, history_revision: Self::revision(revision)? })
        }).await.map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?
    }
    async fn read_item(
        &self,
        thread_id: &ThreadId,
        item_id: &wunder_core::thread_store::ItemId,
    ) -> ThreadStoreResult<ItemSummary> {
        let this = self.clone();
        let t = thread_id.clone();
        let i = item_id.clone();
        tokio::task::spawn_blocking(move || { let mut c=this.storage.conn().map_err(Self::map_error)?; let row=c.query_opt("SELECT record_json FROM service_thread_records WHERE thread_id=$1 AND item_id=$2 AND record_type='item_completed' ORDER BY ordinal DESC LIMIT 1", &[&t.as_str(), &i.as_str()]).map_err(Self::map_error)?.ok_or_else(|| Self::missing("item", i.to_string()))?; let r:RolloutRecord=serde_json::from_str(row.get::<_, String>("record_json").as_str()).map_err(|e| ThreadStoreError::CorruptHistory { message:e.to_string() })?; Self::item_from_record(&r).ok_or_else(|| Self::missing("item", i.to_string())) }).await.map_err(|e| ThreadStoreError::Unavailable { message:e.to_string() })?
    }
    async fn read_item_content(
        &self,
        request: ItemContentRequest,
    ) -> ThreadStoreResult<ItemContentPage> {
        let item = self.read_item(&request.thread_id, &request.item_id).await?;
        Self::item_content(&item, request.offset_bytes, request.max_bytes)
    }
    async fn load_model_context(
        &self,
        request: ModelContextRequest,
    ) -> ThreadStoreResult<ModelContext> {
        if request.max_records == 0
            || request.max_records > MAX_CONTEXT_RECORDS
            || request.max_bytes == 0
            || request.max_bytes > MAX_CONTEXT_BYTES
        {
            return Err(ThreadStoreError::Conflict {
                message: "invalid model context budget".into(),
            });
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = this.storage.conn().map_err(Self::map_error)?;
            let (meta, revision) = this.current_metadata(&request.thread_id, &mut conn)?;
            let before = if let Some(position) = &request.position {
                if position.rollout_id != meta.current_rollout_id || position.end_ordinal_exclusive > revision as u64 || position.end_byte_offset != position.end_ordinal_exclusive {
                    return Err(ThreadStoreError::Conflict { message: "invalid PostgreSQL model context boundary".into() });
                }
                position.end_ordinal_exclusive as i64
            } else { revision };
            let thread = request.thread_id.as_str();
            let checkpoint = conn.query_opt("SELECT ordinal FROM service_thread_records WHERE thread_id=$1 AND record_type='compacted' AND ordinal<$2 ORDER BY ordinal DESC LIMIT 1", &[&thread, &before]).map_err(Self::map_error)?.map(|row| row.get::<_,i64>(0)).unwrap_or(0);
            // Read only bounded ordinal/size metadata first. Audit-only rows
            // never consume the model replay budget or cross the network.
            let rows = conn.query("SELECT ordinal,octet_length(record_json) AS bytes FROM service_thread_records WHERE thread_id=$1 AND ordinal>=$2 AND ordinal<$3 AND record_type IN ('response_item','compacted') ORDER BY ordinal DESC LIMIT $4", &[&thread, &checkpoint, &before, &(i64::from(request.max_records)+1)]).map_err(Self::map_error)?;
            let mut ordinals = Vec::with_capacity(request.max_records as usize);
            let mut bytes = 0usize;
            let mut truncated = false;
            for row in rows {
                let size = row.get::<_,i32>("bytes") as usize;
                if ordinals.len() >= request.max_records as usize || bytes.saturating_add(size) > request.max_bytes as usize { truncated = true; break; }
                bytes += size;
                ordinals.push(row.get::<_,i64>("ordinal"));
            }
            let records = if ordinals.is_empty() { Vec::new() } else {
                conn.query("SELECT record_json FROM service_thread_records WHERE thread_id=$1 AND ordinal=ANY($2) ORDER BY ordinal", &[&thread, &ordinals]).map_err(Self::map_error)?.iter().map(Self::decode_record).collect::<ThreadStoreResult<Vec<_>>>()?
            };
            Ok(ModelContext { thread_id: request.thread_id, history_revision: Self::revision(before)?, records, truncated })
        }).await.map_err(|e| ThreadStoreError::Unavailable { message: e.to_string() })?
    }
}

fn validate_limit(limit: u16, max: u16) -> ThreadStoreResult<()> {
    if limit == 0 || limit > max {
        return Err(ThreadStoreError::InvalidPageLimit {
            requested: limit,
            maximum: max,
        });
    }
    Ok(())
}

impl PostgresThreadStore {
    pub(crate) async fn storage_session_meta(
        &self,
        thread: &ThreadId,
    ) -> ThreadStoreResult<wunder_core::thread_store::SessionMeta> {
        let this = self.clone();
        let id = thread.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = this.storage.conn().map_err(Self::map_error)?;
            let row = c
                .query_opt(
                    "SELECT session_json FROM service_thread_sessions WHERE thread_id=$1",
                    &[&id.as_str()],
                )
                .map_err(Self::map_error)?
                .ok_or_else(|| Self::missing("thread", id.to_string()))?;
            serde_json::from_str::<wunder_core::thread_store::SessionMeta>(
                row.get::<_, String>("session_json").as_str(),
            )
            .map_err(|e| ThreadStoreError::CorruptHistory {
                message: e.to_string(),
            })
        })
        .await
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })?
    }
    pub(crate) async fn history_position_public(
        &self,
        thread: &ThreadId,
    ) -> ThreadStoreResult<HistoryPosition> {
        let meta = self.get_thread(thread).await?;
        let this = self.clone();
        let id = thread.clone();
        let revision = tokio::task::spawn_blocking(move || {
            let mut c = this.storage.conn().map_err(Self::map_error)?;
            let row = c
                .query_one(
                    "SELECT history_revision FROM service_threads WHERE thread_id=$1",
                    &[&id.as_str()],
                )
                .map_err(Self::map_error)?;
            Ok::<i64, ThreadStoreError>(row.get("history_revision"))
        })
        .await
        .map_err(|e| ThreadStoreError::Unavailable {
            message: e.to_string(),
        })??;
        Ok(HistoryPosition {
            rollout_id: meta.current_rollout_id,
            end_ordinal_exclusive: revision.max(0) as u64,
            end_byte_offset: revision.max(0) as u64,
        })
    }
}

async fn db<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> ThreadStoreResult<T> {
    crate::core::blocking::run_db("postgres.thread_store", work)
        .await
        .map_err(PostgresThreadStore::map_error)
}

#[derive(Serialize, Deserialize)]
struct HistoryCursor {
    thread: String,
    rollout: String,
    revision: i64,
    kind: String,
    turn: Option<String>,
    before: i64,
}

fn unavailable(error: impl std::fmt::Display) -> ThreadStoreError {
    ThreadStoreError::Unavailable {
        message: error.to_string(),
    }
}
