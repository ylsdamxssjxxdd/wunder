use super::support::{equivalent_peer_kinds, extract_chat_content, now_ts};
use super::ChannelHub;
use crate::channels::pending_files::{
    read_pending_files_from_metadata, write_pending_files_to_metadata, PendingChannelFile,
};
use crate::channels::types::ChannelMessage;
use crate::config::Config;
use crate::core::blocking;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
    ChannelSessionRecord, ChatSessionRecord, ListChannelUserBindingsQuery,
    UpdateChannelOutboxStatusParams, UserAgentRecord,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::warn;
use uuid::Uuid;

async fn run_channel_db<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    blocking::run_db(label, task).await
}

impl ChannelHub {
    pub(super) async fn load_channel_account(
        &self,
        channel: &str,
        account_id: &str,
        config: &Config,
    ) -> Result<ChannelAccountRecord> {
        let channel = channel.trim();
        let account_id = account_id.trim();
        if channel.is_empty() || account_id.is_empty() {
            return Err(anyhow!("missing channel/account"));
        }
        let storage = self.storage.clone();
        let channel_key = channel.to_string();
        let account_key = account_id.to_string();
        let channel_lookup = channel_key.clone();
        let account_lookup = account_key.clone();
        let record = run_channel_db("channels.persistence.load_channel_account", move || {
            storage.get_channel_account(&channel_lookup, &account_lookup)
        })
        .await?;
        if let Some(record) = record {
            if record.status.trim().to_lowercase() != "active" {
                return Err(anyhow!("channel account disabled"));
            }
            return Ok(record);
        }
        if config.channels.allow_unknown_accounts {
            Ok(ChannelAccountRecord {
                channel: channel_key,
                account_id: account_key,
                config: json!({}),
                status: "active".to_string(),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
        } else {
            Err(anyhow!("channel account not found"))
        }
    }

    pub(super) async fn list_channel_bindings(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelBindingRecord>> {
        let storage = self.storage.clone();
        let channel = channel.map(|value| value.to_string());
        run_channel_db("channels.persistence.list_channel_bindings", move || {
            storage.list_channel_bindings(channel.as_deref())
        })
        .await
    }

    pub(super) async fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<crate::storage::ChannelUserBindingRecord>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        let peer_kinds = equivalent_peer_kinds(peer_kind);
        let peer_id = peer_id.to_string();
        run_channel_db("channels.persistence.get_channel_user_binding", move || {
            let mut peer_ids = vec![peer_id.clone()];
            if !peer_ids
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case("*"))
            {
                peer_ids.push("*".to_string());
            }
            for candidate_kind in &peer_kinds {
                for candidate_peer_id in &peer_ids {
                    if let Some(record) = storage.get_channel_user_binding(
                        &channel,
                        &account_id,
                        candidate_kind,
                        candidate_peer_id,
                    )? {
                        return Ok(Some(record));
                    }
                }
            }
            Ok(None)
        })
        .await
    }

    pub(super) async fn get_channel_account_owner(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<String>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        run_channel_db(
            "channels.persistence.get_channel_account_owner",
            move || {
                let (records, _) =
                    storage.list_channel_user_bindings(ListChannelUserBindingsQuery {
                        channel: Some(channel.as_str()),
                        account_id: Some(account_id.as_str()),
                        peer_kind: None,
                        peer_id: None,
                        user_id: None,
                        offset: 0,
                        limit: 1,
                    })?;
                Ok(records
                    .first()
                    .map(|record| record.user_id.trim().to_string())
                    .filter(|value| !value.is_empty()))
            },
        )
        .await
    }

    /// Admit once before slow media/model work; every response keeps this identity.
    pub(super) async fn accept_channel_turn(
        &self,
        info: &mut super::ChannelSessionInfo,
        content: &str,
    ) -> Result<()> {
        if info.accepted_turn.is_none() {
            info.accepted_turn = Some(
                self.orchestrator
                    .committer
                    .accept_turn(
                        &info.user_id,
                        &info.session_id,
                        &json!({"role": "user", "content": content}),
                    )
                    .await?,
            );
        }
        Ok(())
    }

    pub(super) async fn finish_channel_turn(
        &self,
        info: &super::ChannelSessionInfo,
        content: &str,
        status: &str,
    ) -> Result<()> {
        let accepted = info
            .accepted_turn
            .as_ref()
            .ok_or_else(|| anyhow!("channel turn not accepted"))?;
        let turn = accepted["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing channel turn identity"))?;
        let item = channel_reply_item(info, turn, content, status);
        self.orchestrator
            .committer
            .commit_item(&info.user_id, &item)
            .await?;
        self.orchestrator
            .committer
            .update_turn(
                &info.user_id,
                &info.session_id,
                turn,
                status,
                "",
                &json!({"source": "channel"}),
            )
            .await?;
        Ok(())
    }

    pub(super) async fn touch_chat_session_activity(&self, user_id: &str, session_id: &str) {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return;
        }
        let user_store = self.user_store.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let now = now_ts();
        let outcome = run_channel_db(
            "channels.persistence.touch_chat_session_activity",
            move || user_store.touch_chat_session(&user_id, &session_id, now, now),
        )
        .await;
        if let Err(err) = outcome {
            warn!(
                "touch channel chat session failed: user_id={}, session_id={}, error={err}",
                cleaned_user, cleaned_session
            );
        }
    }

    pub(super) async fn load_latest_user_message(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Option<String> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return None;
        }
        let storage = self.storage.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let history = run_channel_db("channels.persistence.load_latest_user_message", move || {
            storage.load_thread_context_items(&user_id, &session_id, 20, false)
        })
        .await
        .ok()?;
        for item in history {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            if !role.eq_ignore_ascii_case("user") {
                continue;
            }
            if let Some(text) = extract_chat_content(&item) {
                return Some(text);
            }
        }
        None
    }

    pub(super) async fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        let peer_kinds = equivalent_peer_kinds(peer_kind);
        let peer_id = peer_id.to_string();
        let thread_id = thread_id.map(str::to_string);
        run_channel_db("channels.persistence.get_channel_session", move || {
            for candidate_kind in &peer_kinds {
                if let Some(record) = storage.get_channel_session(
                    &channel,
                    &account_id,
                    candidate_kind,
                    &peer_id,
                    thread_id.as_deref(),
                )? {
                    return Ok(Some(record));
                }
            }
            Ok(None)
        })
        .await
    }

    pub(super) async fn load_pending_channel_files(
        &self,
        message: &ChannelMessage,
    ) -> Result<Vec<PendingChannelFile>> {
        let session = self
            .get_channel_session(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
                message.thread.as_ref().map(|thread| thread.id.as_str()),
            )
            .await?;
        Ok(read_pending_files_from_metadata(
            session.as_ref().and_then(|record| record.metadata.as_ref()),
        ))
    }

    pub(super) async fn save_pending_channel_files(
        &self,
        message: &ChannelMessage,
        files: &[PendingChannelFile],
    ) -> Result<()> {
        let Some(mut record) = self
            .get_channel_session(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
                message.thread.as_ref().map(|thread| thread.id.as_str()),
            )
            .await?
        else {
            return Ok(());
        };
        record.metadata = write_pending_files_to_metadata(record.metadata, files);
        let now = now_ts();
        record.updated_at = now;
        record.last_message_at = now;
        self.upsert_channel_session(&record).await
    }

    pub(super) async fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        let user_store = self.user_store.clone();
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        run_channel_db("channels.persistence.get_chat_session", move || {
            user_store.get_chat_session(&user_id, &session_id)
        })
        .await
    }

    pub(super) async fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        let storage = self.storage.clone();
        let record = record.clone();
        run_channel_db("channels.persistence.upsert_channel_session", move || {
            storage.upsert_channel_session(&record)
        })
        .await
    }

    pub(super) async fn save_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        let user_store = self.user_store.clone();
        let record = record.clone();
        run_channel_db("channels.persistence.save_chat_session", move || {
            user_store.upsert_chat_session(&record)
        })
        .await
    }

    pub(super) async fn save_media_asset(
        &self,
        channel: &str,
        account_id: &str,
        attachment: &crate::channels::types::ChannelAttachment,
    ) -> Result<()> {
        let storage = self.storage.clone();
        let record = crate::storage::MediaAssetRecord {
            asset_id: format!("asset_{}", Uuid::new_v4().simple()),
            kind: attachment.kind.clone(),
            url: attachment.url.clone(),
            mime: attachment.mime.clone(),
            size: attachment.size,
            hash: None,
            source: Some(format!("{channel}:{account_id}")),
            created_at: now_ts(),
        };
        run_channel_db("channels.persistence.save_media_asset", move || {
            storage.upsert_media_asset(&record)
        })
        .await
    }

    pub(super) async fn insert_channel_message(
        &self,
        message: &ChannelMessage,
        session_id: &str,
        raw_payload: Option<Value>,
    ) -> Result<()> {
        let record = ChannelMessageRecord {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer_kind: message.peer.kind.clone(),
            peer_id: message.peer.id.clone(),
            thread_id: message.thread.as_ref().map(|thread| thread.id.clone()),
            session_id: session_id.to_string(),
            message_id: message.message_id.clone(),
            sender_id: message.sender.as_ref().map(|sender| sender.id.clone()),
            message_type: message.message_type.clone(),
            payload: json!(message),
            raw_payload,
            created_at: now_ts(),
        };
        let storage = self.storage.clone();
        run_channel_db("channels.persistence.insert_channel_message", move || {
            storage.insert_channel_message(&record)
        })
        .await
    }

    pub(super) async fn insert_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        let storage = self.storage.clone();
        let record = record.clone();
        run_channel_db("channels.persistence.insert_outbox", move || {
            storage.enqueue_channel_outbox(&record)
        })
        .await
    }

    pub(super) async fn get_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        let storage = self.storage.clone();
        let outbox_id = outbox_id.to_string();
        run_channel_db("channels.persistence.get_outbox", move || {
            storage.get_channel_outbox(&outbox_id)
        })
        .await
    }

    pub(super) async fn list_pending_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        let storage = self.storage.clone();
        run_channel_db("channels.persistence.list_pending_outbox", move || {
            storage.list_pending_channel_outbox(limit)
        })
        .await
    }

    pub(super) async fn set_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()> {
        let storage = self.storage.clone();
        let outbox_id = params.outbox_id.to_string();
        let status = params.status.to_string();
        let last_error = params.last_error.map(str::to_string);
        let retry_count = params.retry_count;
        let retry_at = params.retry_at;
        let delivered_at = params.delivered_at;
        let updated_at = params.updated_at;
        run_channel_db("channels.persistence.set_outbox_status", move || {
            storage.update_channel_outbox_status(UpdateChannelOutboxStatusParams {
                outbox_id: &outbox_id,
                status: &status,
                retry_count,
                retry_at,
                last_error: last_error.as_deref(),
                delivered_at,
                updated_at,
            })
        })
        .await
    }

    pub(super) async fn get_agent(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        let user_store = self.user_store.clone();
        let agent_id = agent_id.to_string();
        run_channel_db("channels.persistence.get_agent", move || {
            user_store.get_user_agent_by_id(&agent_id)
        })
        .await
    }
}

// Direct channel replies have the same stable output identity as model replies.
fn channel_reply_item(
    info: &super::ChannelSessionInfo,
    turn: &str,
    content: &str,
    status: &str,
) -> Value {
    json!({"session_id": info.session_id, "turn_id": turn,
        "item_id": format!("{turn}:text-0"), "model_round": 0,
        "kind": "assistant_message", "role": "assistant", "visibility": "user",
        "status": status, "content": content, "source": "channel"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteStorage;
    use wunder_core::storage_backend::ThreadLogStore;

    #[test]
    fn channel_direct_replies_restore_one_settled_pair_per_input() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into_owned());
        let mut info = super::super::ChannelSessionInfo {
            session_id: "fixture-thread".into(),
            user_id: "fixture-user".into(),
            accepted_turn: None,
            tts_enabled: None,
            tts_voice: None,
        };
        for (input, reply, status) in [
            ("/help", "Fixture help", "completed"),
            ("Fixture upload", "Fixture file received", "completed"),
            ("Fixture busy input", "Fixture busy reply", "completed"),
            ("Fixture failed input", "Fixture failure", "failed"),
        ] {
            let accepted = db
                .accept_thread_turn(&info.user_id, &info.session_id, &json!({"content": input}))
                .unwrap();
            let turn = accepted["turn_id"].as_str().unwrap().to_string();
            info.accepted_turn = Some(accepted);
            let item = channel_reply_item(&info, &turn, reply, status);
            db.commit_thread_item(&info.user_id, &item).unwrap();
            db.update_thread_turn(
                &info.user_id,
                &info.session_id,
                &turn,
                status,
                "",
                &json!({}),
            )
            .unwrap();
        }
        let snapshot = db.thread_snapshot(&info.user_id, &info.session_id).unwrap();
        assert_eq!(snapshot["turns"].as_array().unwrap().len(), 4);
        let items = snapshot["items"].as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|item| item["kind"] == "user_message")
                .count(),
            4
        );
        let replies: Vec<_> = items
            .iter()
            .filter(|item| item["kind"] == "assistant_message")
            .collect();
        assert_eq!(replies.len(), 4);
        for item in replies {
            assert_eq!(
                item["item_id"],
                format!("{}:text-0", item["turn_id"].as_str().unwrap())
            );
            assert!(!item["payload"]["content"].as_str().unwrap().is_empty());
        }
        assert!(snapshot["turns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|turn| matches!(turn["status"].as_str(), Some("completed" | "failed"))));
    }
}
