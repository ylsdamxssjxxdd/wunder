//! Durable, thread-scoped intents for mutations crossing state and queue.
//! Recovery is requested by thread access; no timer or history scan is needed.
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Component, Path};
use wunder_core::thread_store::{
    CanonicalRecord, RolloutId, RolloutRecord, SessionMeta, ThreadId, ThreadMetadata, TurnId,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct RevertIntent {
    pub metadata: ThreadMetadata,
    pub session_meta: SessionMeta,
    pub expected_rollout: RolloutId,
    pub through_turn: TurnId,
    pub relative_path: String,
}

pub(crate) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS canonical_pending_queue_reverts (
            thread_id TEXT PRIMARY KEY NOT NULL,
            intent_json TEXT NOT NULL
         );",
    )?;
    Ok(())
}

pub(crate) fn pending(conn: &Connection, thread: &ThreadId) -> Result<Option<RevertIntent>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT intent_json FROM canonical_pending_queue_reverts WHERE thread_id=?1",
            [thread.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|value| {
            if value.len() > super::rollout::ROLLOUT_MAX_RECORD_BYTES + 64 * 1024 {
                bail!("revert intent exceeds its byte budget");
            }
            let intent: RevertIntent = serde_json::from_str(&value)?;
            if intent.metadata.thread_id != *thread
                || intent.session_meta.thread_id != *thread
                || intent.session_meta.rollout_id != intent.metadata.current_rollout_id
                || intent.session_meta.history_base.is_none()
                || intent.metadata.current_rollout_id == intent.expected_rollout
            {
                bail!("revert intent identity mismatch");
            }
            Ok(intent)
        })
        .transpose()
}

pub(crate) fn insert(conn: &Connection, intent: &RevertIntent) -> Result<()> {
    let value = serde_json::to_string(intent)?;
    if value.len() > super::rollout::ROLLOUT_MAX_RECORD_BYTES + 64 * 1024 {
        bail!("revert intent exceeds its byte budget");
    }
    conn.execute(
        "INSERT INTO canonical_pending_queue_reverts(thread_id,intent_json) VALUES(?1,?2)",
        params![intent.metadata.thread_id.as_str(), value],
    )?;
    Ok(())
}

pub(crate) fn remove(conn: &Connection, thread: &ThreadId) -> Result<()> {
    conn.execute(
        "DELETE FROM canonical_pending_queue_reverts WHERE thread_id=?1",
        [thread.as_str()],
    )?;
    Ok(())
}

pub(crate) fn relative_path(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if path.is_absolute()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        bail!("invalid revert rollout path");
    }
    Ok(path)
}

/// Publish exactly one complete metadata record before exposing the new root.
/// An interrupted, unpublished file can be reconstructed from the intent.
pub(crate) fn prepare_suffix(path: &Path, intent: &RevertIntent, published: bool) -> Result<()> {
    let record = RolloutRecord {
        timestamp_ms: intent.session_meta.created_at_ms,
        ordinal: intent
            .session_meta
            .history_base
            .as_ref()
            .context("revert history boundary is absent")?
            .end_ordinal_exclusive,
        record: CanonicalRecord::SessionMeta(intent.session_meta.clone()),
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    if bytes.len() > super::rollout::ROLLOUT_MAX_RECORD_BYTES {
        bail!("revert metadata record exceeds its byte budget");
    }
    let parent = path.parent().context("revert suffix has no parent")?;
    // The intent is durable before this directory is materialized. Creating it
    // is therefore recoverable and does not make an idle thread write files.
    std::fs::create_dir_all(parent)?;
    let _guard = super::rollout_compression::try_representation_lock(path)?
        .context("revert suffix has another maintenance owner")?;
    if path.try_exists()? {
        let mut actual = Vec::with_capacity(bytes.len());
        std::fs::File::open(path)?
            .take(bytes.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        if actual == bytes {
            return Ok(());
        }
        if published {
            bail!("published revert suffix differs from its durable intent");
        }
    } else if published {
        bail!("published revert suffix is missing");
    }
    // The root still points to the old rollout, so an incomplete unpublished
    // suffix may be replaced under its representation lock on a retry.
    if path.try_exists()? {
        std::fs::remove_file(path)?;
    }
    let compressed = super::rollout_compression::compressed_path(path);
    if compressed.try_exists()? {
        std::fs::remove_file(compressed)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    // This replacement is only allowed before the root is published.
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
