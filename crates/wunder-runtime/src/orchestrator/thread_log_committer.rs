use crate::core::blocking;
use crate::storage::StorageBackend;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

/// Unified durable ThreadLog commit exit for chat v2.
///
/// Every ThreadLog write in the chat pipeline goes through this API:
///   1. a per-session gate serializes commits so change order == commit order;
///   2. the storage call runs as one short blocking transaction;
///   3. only after the transaction succeeds is the durable cursor published
///      to the change hub (a wake signal, never a source of truth);
///   4. idempotent no-ops (replayed acceptance, unchanged status, repeated
///      feedback) produce no change and no hub notification; failed commits
///      never publish a cursor.
///
/// Callers must never assemble cursors themselves or notify the hub before the
/// transaction completes.
pub(crate) struct ThreadLogCommitter {
    storage: Arc<dyn StorageBackend>,
    hub: Arc<super::thread_change_hub::ThreadChangeHub>,
    gates: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl ThreadLogCommitter {
    pub fn new(
        storage: Arc<dyn StorageBackend>,
        hub: Arc<super::thread_change_hub::ThreadChangeHub>,
    ) -> Self {
        Self {
            storage,
            hub,
            gates: Mutex::new(HashMap::new()),
        }
    }

    async fn gate(&self, session_id: &str) -> Arc<AsyncMutex<()>> {
        let session_id = session_id.trim().to_string();
        if session_id.is_empty() {
            return Arc::new(AsyncMutex::new(()));
        }
        self.gates
            .lock()
            .entry(session_id)
            .or_default()
            .clone()
    }

    /// Publish the receipt cursor when the write actually produced a change.
    fn publish_receipt(&self, session_id: &str, receipt: &Value) {
        if let Some(cursor) = receipt.get("cursor").and_then(Value::as_i64) {
            if cursor > 0 {
                self.hub.publish(session_id, cursor);
            }
        }
    }

    /// Storage helpers without receipt cursors publish the fresh latest seq.
    fn publish_latest(&self, session_id: &str) {
        let Ok(cursor) = self.storage.latest_thread_change_seq_by_session(session_id) else {
            return;
        };
        self.hub.publish(session_id, cursor);
    }

    /// Accept a durable user turn. Returns the accepted turn payload; hub wake
    /// happens only when this call actually allocated a new turn.
    pub(crate) async fn accept_turn(
        &self,
        user_id: &str,
        session_id: &str,
        input: &Value,
    ) -> anyhow::Result<Value> {
        let gate = self.gate(session_id).await;
        let _gate = gate.lock().await;
        let storage = self.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        let input = input.clone();
        let accepted = blocking::run_db("thread_log.committer.accept", move || {
            storage.accept_thread_turn(&owner, &thread, &input)
        })
        .await?;
        if accepted.get("created").and_then(Value::as_bool) == Some(true) {
            self.publish_latest(session_id);
        }
        Ok(accepted)
    }

    /// Update a turn and its settled items. Returns true when a durable change
    /// was written; only then is the feeder wake published.
    pub(crate) async fn update_turn(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        status: &str,
        summary: &str,
        payload: &Value,
    ) -> anyhow::Result<bool> {
        let gate = self.gate(session_id).await;
        let _gate = gate.lock().await;
        let storage = self.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        let turn = turn_id.to_string();
        let summary = summary.to_string();
        let status = status.to_string();
        let payload = payload.clone();
        let changed = blocking::run_db("thread_log.committer.update", move || {
            storage.update_thread_turn(&owner, &thread, &turn, &status, &summary, &payload)
        })
        .await?;
        if changed {
            self.publish_latest(session_id);
        }
        Ok(changed)
    }

    /// Commit (upsert) a durable item version. The receipt carries the
    /// change cursor; no-op replays return None without publishing.
    pub(crate) async fn commit_item(
        &self,
        user_id: &str,
        payload: &Value,
    ) -> anyhow::Result<Option<Value>> {
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let gate = self.gate(session_id).await;
        let _gate = gate.lock().await;
        let storage = self.storage.clone();
        let owner = user_id.to_string();
        let payload = payload.clone();
        let receipt = blocking::run_db("thread_log.committer.item", move || {
            storage.commit_thread_item(&owner, &payload)
        })
        .await?;
        if let Some(receipt) = receipt.as_ref() {
            self.publish_receipt(session_id, receipt);
        }
        Ok(receipt)
    }

    /// Append a durable item through the same unified exit. The storage layer
    /// is the same upsert; no-op repeats return None without publishing.
    pub(crate) async fn append_item(
        &self,
        user_id: &str,
        payload: &Value,
    ) -> anyhow::Result<Option<Value>> {
        self.commit_item(user_id, payload).await
    }

    /// Persist one immutable text block. Returns the allocated change cursor;
    /// an idempotent replay returns 0 without publishing.
    pub(crate) async fn upsert_text_block(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> anyhow::Result<i64> {
        let gate = self.gate(session_id).await;
        let _gate = gate.lock().await;
        let storage = self.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        let block = block.clone();
        let cursor = blocking::run_db("thread_log.committer.text_block", move || {
            storage.upsert_thread_text_block(&owner, &thread, &block)
        })
        .await?;
        if cursor > 0 {
            self.hub.publish(session_id, cursor);
        }
        Ok(cursor)
    }

    /// Record locked user feedback on an assistant item. Publishes only when
    /// the feedback was actually recorded.
    pub(crate) async fn set_feedback(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        vote: &str,
    ) -> anyhow::Result<Option<Value>> {
        let gate = self.gate(session_id).await;
        let _gate = gate.lock().await;
        let storage = self.storage.clone();
        let owner = user_id.to_string();
        let thread = session_id.to_string();
        let item = item_id.to_string();
        let vote = vote.to_string();
        let feedback = blocking::run_db("thread_log.committer.feedback", move || {
            storage.set_thread_item_feedback(&owner, &thread, &item, &vote)
        })
        .await?;
        if feedback.is_some() {
            self.publish_latest(session_id);
        }
        Ok(feedback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build_committer() -> (
        ThreadLogCommitter,
        std::sync::Arc<super::super::thread_change_hub::ThreadChangeHub>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(crate::storage::SqliteStorage::new(
            dir.path()
                .join("committer.db")
                .to_string_lossy()
                .into_owned(),
        ));
        storage.ensure_initialized().unwrap();
        let hub = Arc::new(super::super::thread_change_hub::ThreadChangeHub::new());
        (ThreadLogCommitter::new(storage, hub.clone()), hub, dir)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_op_updates_do_not_publish_or_write() {
        let (committer, hub, _dir) = build_committer();
        // The hub drops publishes with no subscriber yet (poll fallback covers
        // them), so the wake receiver must exist before the durable commit.
        let mut wake = hub.subscribe("session-a");
        let accepted = committer
            .accept_turn(
                "user-a",
                "session-a",
                &json!({"content":"hi","client_message_id":"msg-1"}),
            )
            .await
            .unwrap();
        assert_eq!(accepted["created"], json!(true));
        let cursor = *wake.borrow_and_update();
        assert!(cursor > 0);

        // Re-accepting the same client message is an idempotent replay.
        let replay = committer
            .accept_turn(
                "user-a",
                "session-a",
                &json!({"content":"hi","client_message_id":"msg-1"}),
            )
        .await
        .unwrap();
        assert_eq!(replay["turn_id"], accepted["turn_id"]);
        assert_eq!(replay["created"], json!(false));
        assert!(!wake.has_changed().unwrap());
        // A no-op replay must not allocate a phantom change either.
        assert_eq!(
            committer
                .storage
                .latest_thread_change_seq_by_session("session-a")
                .unwrap(),
            cursor
        );

        // Repeating the same status update writes no change and wakes nothing.
        let turn = accepted["turn_id"].as_str().unwrap();
        let changed = committer
            .update_turn("user-a", "session-a", turn, "queued", "", &json!({}))
            .await
            .unwrap();
        assert!(!changed);
        assert!(!wake.has_changed().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failing_commit_does_not_publish_cursor() {
        let (committer, hub, _dir) = build_committer();
        let mut wake = hub.subscribe("session-a");
        let _ = committer
            .accept_turn("user-a", "session-a", &json!({"content":"hi"}))
            .await
            .unwrap();
        let baseline = *wake.borrow_and_update();

        // A payload pointing at a foreign session owner must fail.
        let err = committer
            .commit_item(
                "other-user",
                &json!({"session_id":"session-a","turn_id":"x","item_id":"x:item",
                         "kind":"assistant_message","status":"running","visibility":"user"}),
            )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("thread owner mismatch"));
        assert!(!wake.has_changed().unwrap());
        assert_eq!(
            *wake.borrow(),
            committer
                .storage
                .latest_thread_change_seq_by_session("session-a")
                .unwrap()
        );
        assert_eq!(*wake.borrow(), baseline);
    }
}
