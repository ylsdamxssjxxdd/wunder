use crate::storage_bridge::*;
use crate::storage_records::*;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

/// Domain-scoped storage abstractions for persistent runtime data.

/// Storage backend lifecycle and schema readiness checks.
pub trait StorageLifecycle {
    fn ensure_initialized(&self) -> Result<()>;
}

/// Persistent metadata key/value store.
pub trait MetaStore {
    fn get_meta(&self, key: &str) -> Result<Option<String>>;
    fn set_meta(&self, key: &str, value: &str) -> Result<()>;
    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize>;
}

/// Chat, model-context, tool, and artifact log storage.
pub trait ConversationLogStore {
    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_model_context_entry(
        &self,
        user_id: &str,
        session_id: &str,
        payload: &Value,
    ) -> Result<()>;
    fn replace_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        payloads: &[Value],
    ) -> Result<()>;
    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_chat_history_page(
        &self,
        user_id: &str,
        session_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_chat_history_item(
        &self,
        user_id: &str,
        session_id: &str,
        history_id: i64,
    ) -> Result<Option<Value>> {
        if history_id <= 0 {
            return Ok(None);
        }
        // Reuse the indexed cursor query so SQLite and PostgreSQL keep identical ownership semantics.
        Ok(self
            .load_chat_history_page(user_id, session_id, Some(history_id.saturating_add(1)), 1)?
            .into_iter()
            .find(|item| item.get("_history_id").and_then(Value::as_i64) == Some(history_id)))
    }
    fn load_artifact_logs(&self, user_id: &str, session_id: &str, limit: i64)
        -> Result<Vec<Value>>;
    fn get_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>>;
}

/// Log usage, statistics, and cleanup storage.
pub trait LogStatsStore {
    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>>;
    fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_log_usage(&self) -> Result<u64>;
    fn delete_logs_by_time_range(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>>;
    fn delete_chat_history(&self, user_id: &str) -> Result<i64>;
    fn delete_chat_history_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_tool_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
}

/// Runtime monitor record storage.
pub trait MonitorStore {
    fn upsert_monitor_record(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records(&self) -> Result<Vec<Value>>;
    fn load_recent_monitor_records(&self, limit: i64) -> Result<Vec<Value>> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let mut records = self.load_monitor_records()?;
        records.sort_by(|left, right| {
            monitor_record_updated_time(right).total_cmp(&monitor_record_updated_time(left))
        });
        records.truncate(limit as usize);
        Ok(records)
    }
    fn load_monitor_records_by_user(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let status_set = statuses
            .unwrap_or(&[])
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<std::collections::HashSet<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut records = self
            .load_monitor_records()?
            .into_iter()
            .filter(|record| {
                record
                    .get("user_id")
                    .and_then(Value::as_str)
                    .map(|value| value.trim() == cleaned_user)
                    .unwrap_or(false)
            })
            .filter(|record| {
                if status_set.is_empty() {
                    return true;
                }
                record
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| status_set.contains(value.trim()))
                    .unwrap_or(false)
            })
            .filter(|record| {
                let Some(since) = since_time else {
                    return true;
                };
                monitor_record_updated_time(record) >= since
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            monitor_record_updated_time(right).total_cmp(&monitor_record_updated_time(left))
        });
        records.truncate(limit as usize);
        Ok(records)
    }
    fn sum_monitor_consumed_tokens_by_user(&self, user_id: &str) -> Result<i64> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        Ok(self
            .load_monitor_records()?
            .into_iter()
            .filter(|record| {
                record
                    .get("user_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.trim() == cleaned_user)
            })
            .filter_map(|record| record.get("consumed_tokens").and_then(Value::as_i64))
            .fold(0_i64, |total, value| total.saturating_add(value.max(0))))
    }
    fn delete_monitor_record(&self, session_id: &str) -> Result<()>;
    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64>;
}

/// Session concurrency lock storage.
pub trait SessionLockStore {
    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus>;
    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()>;
    fn release_session_lock(&self, session_id: &str) -> Result<()>;
    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64>;
    fn count_session_locks(&self) -> Result<i64>;
    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>>;
}

/// Agent thread, task queue, and stream event storage.
pub trait AgentRuntimeStore {
    fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()>;
    fn get_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<Option<AgentThreadRecord>>;
    fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64>;
    fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()>;
    fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>>;
    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>>;
    fn count_pending_agent_tasks(&self) -> Result<i64>;
    fn count_pending_agent_tasks_ahead(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64>;
    fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>>;
    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()>;
    fn get_max_stream_event_id(&self, session_id: &str) -> Result<i64>;
    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()>;
    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_recent_stream_events(&self, session_id: &str, limit: i64) -> Result<Vec<Value>>;
    fn load_session_workflow_events(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<Vec<Value>>;
    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64>;
    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64>;
    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64>;
}

/// Vector knowledge document storage.
pub trait VectorDocumentStore {
    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()>;
    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>>;
    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>>;
    fn delete_vector_document(&self, owner_id: &str, base_name: &str, doc_id: &str)
        -> Result<bool>;
    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64>;
    fn upsert_vector_chunk_embeddings(&self, records: &[VectorChunkEmbeddingRecord]) -> Result<()>;
    fn list_vector_chunk_embeddings(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        limit: i64,
    ) -> Result<Vec<VectorChunkEmbeddingRecord>>;
    fn delete_vector_chunk_embedding(&self, chunk_id: &str) -> Result<bool>;
    fn delete_vector_chunk_embeddings_by_doc(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<i64>;
    fn delete_vector_chunk_embeddings_by_base(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<i64>;
}

/// Long-term memory settings, records, fragments, hits, and jobs storage.
pub trait MemoryRecordStore {
    fn get_memory_enabled(&self, user_id: &str) -> Result<Option<bool>>;
    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()>;
    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn upsert_memory_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()>;
    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64>;
    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_task_log(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()>;
    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>>;
    fn load_memory_task_log_by_task_id(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>>;
    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64>;
    fn upsert_memory_fragment(&self, record: &MemoryFragmentRecord) -> Result<()>;
    fn get_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>>;
    fn list_memory_fragments(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>>;
    fn get_memory_fragment_embedding(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>>;
    fn upsert_memory_fragment_embedding(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()>;
    fn delete_memory_fragment_embeddings(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64>;
    fn delete_memory_fragment(&self, user_id: &str, agent_id: &str, memory_id: &str)
        -> Result<i64>;
    fn insert_memory_hit(&self, record: &MemoryHitRecord) -> Result<()>;
    fn list_memory_hits(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>>;
    fn list_memory_hit_counts(&self, user_id: &str, agent_id: &str)
        -> Result<HashMap<String, i64>>;
    fn has_memory_hit_event(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool>;
    fn upsert_memory_job(&self, record: &MemoryJobRecord) -> Result<()>;
    fn list_memory_jobs(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>>;
}

/// Benchmark run, attempt, and aggregate storage.
pub trait BenchmarkStore {
    fn create_benchmark_run(&self, payload: &Value) -> Result<()>;
    fn update_benchmark_run(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_attempt(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn upsert_benchmark_task_aggregate(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn load_benchmark_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_benchmark_run(&self, run_id: &str) -> Result<Option<Value>>;
    fn load_benchmark_attempts(&self, run_id: &str) -> Result<Vec<Value>>;
    fn load_benchmark_task_aggregates(&self, run_id: &str) -> Result<Vec<Value>>;
    fn delete_benchmark_run(&self, run_id: &str) -> Result<i64>;
}

/// Retention cleanup storage.
pub trait RetentionStore {
    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>>;
}

/// User, organization, token, external link, and session-scope storage.
pub trait UserAccountStore {
    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()>;
    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()>;
    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>>;
    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)>;
    fn add_user_experience(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult>;
    fn delete_user_account(&self, user_id: &str) -> Result<i64>;
    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>>;
    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>>;
    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()>;
    fn delete_org_unit(&self, unit_id: &str) -> Result<i64>;
    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()>;
    fn get_external_link(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>>;
    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>>;
    fn delete_external_link(&self, link_id: &str) -> Result<i64>;
    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()>;
    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>>;
    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()>;
    fn delete_user_token(&self, token: &str) -> Result<i64>;
    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()>;
    fn get_user_session_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>>;
}

/// Chat session catalog storage.
pub trait ChatSessionStore {
    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()>;
    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>>;
    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_sessions_by_status(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>>;
    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()>;
    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()>;
    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
}

/// Session goal accounting storage.
pub trait SessionGoalStore {
    fn upsert_session_goal(&self, record: &SessionGoalRecord) -> Result<()>;
    fn get_session_goal(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>>;
    fn list_session_goals(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<SessionGoalRecord>>;
    fn delete_session_goal(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn account_session_goal_usage(
        &self,
        user_id: &str,
        session_id: &str,
        tokens_delta: i64,
        time_delta_seconds: i64,
        updated_at: f64,
    ) -> Result<Option<SessionGoalRecord>>;
}

/// User-world direct and group conversation storage.
pub trait UserWorldStore {
    fn resolve_or_create_user_world_direct_conversation(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn create_user_world_group(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn get_user_world_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>>;
    fn get_user_world_member(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>>;
    fn list_user_world_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)>;
    fn list_user_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>>;
    fn send_user_world_message(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult>;
    fn mark_user_world_read(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>>;
    fn list_user_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>>;
    fn list_user_world_groups(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)>;
    fn get_user_world_group_by_id(&self, group_id: &str) -> Result<Option<UserWorldGroupRecord>>;
    fn update_user_world_group_announcement(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>>;
    fn list_user_world_member_user_ids(&self, conversation_id: &str) -> Result<Vec<String>>;
}

/// Beeroom chat message storage.
pub trait BeeroomStore {
    fn list_beeroom_chat_messages(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>>;
    #[allow(clippy::too_many_arguments)]
    fn append_beeroom_chat_message(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord>;
    fn delete_beeroom_chat_messages(&self, user_id: &str, group_id: &str) -> Result<i64>;
}

/// External channel account and binding directory storage.
pub trait ChannelDirectoryStore {
    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()>;
    fn get_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>>;
    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>>;
    fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()>;
    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>>;
    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64>;
    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()>;
    fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>>;
    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)>;
    fn delete_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64>;
}

/// External channel session, message, and outbox storage.
pub trait ChannelRuntimeStore {
    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()>;
    fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>>;
    fn list_channel_sessions(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)>;
    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()>;
    fn list_channel_messages(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>>;
    fn get_channel_message_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats>;
    fn get_channel_outbox_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats>;
    fn delete_channel_sessions(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_messages(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_outbox(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()>;
    fn get_channel_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>>;
    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>>;
    fn update_channel_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()>;
}

/// Bridge center, route, delivery log, and audit log storage.
pub trait BridgeStore {
    fn upsert_bridge_center(&self, record: &BridgeCenterRecord) -> Result<()>;
    fn get_bridge_center(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>>;
    fn get_bridge_center_by_code(&self, code: &str) -> Result<Option<BridgeCenterRecord>>;
    fn list_bridge_centers(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)>;
    fn delete_bridge_center(&self, center_id: &str) -> Result<i64>;
    fn upsert_bridge_center_account(&self, record: &BridgeCenterAccountRecord) -> Result<()>;
    fn get_bridge_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn get_bridge_center_account_by_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn list_bridge_center_accounts(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)>;
    fn delete_bridge_center_account(&self, center_account_id: &str) -> Result<i64>;
    fn delete_bridge_center_accounts_by_center(&self, center_id: &str) -> Result<i64>;
    fn upsert_bridge_user_route(&self, record: &BridgeUserRouteRecord) -> Result<()>;
    fn get_bridge_user_route(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>>;
    fn get_bridge_user_route_by_identity(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>>;
    fn list_bridge_user_routes(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)>;
    fn delete_bridge_user_route(&self, route_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center_account(&self, center_account_id: &str) -> Result<i64>;
    fn insert_bridge_delivery_log(&self, record: &BridgeDeliveryLogRecord) -> Result<()>;
    fn list_bridge_delivery_logs(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>>;
    fn delete_bridge_delivery_logs_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_delivery_logs_by_center_account(&self, center_account_id: &str)
        -> Result<i64>;
    fn insert_bridge_route_audit_log(&self, record: &BridgeRouteAuditLogRecord) -> Result<()>;
    fn list_bridge_route_audit_logs(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>>;
    fn delete_bridge_route_audit_logs_by_center(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_route_audit_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64>;
}

/// Gateway client, node, and node-token storage.
pub trait GatewayStore {
    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()>;
    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>>;
    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()>;
    fn get_gateway_node(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>>;
    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>>;
    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()>;
    fn get_gateway_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>>;
    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>>;
    fn delete_gateway_node_token(&self, token: &str) -> Result<i64>;
}

/// Media asset and speech job storage.
pub trait MediaStore {
    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()>;
    fn get_media_asset(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>>;
    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>>;
    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()>;
    fn list_pending_speech_jobs(&self, job_type: &str, limit: i64) -> Result<Vec<SpeechJobRecord>>;
}

/// Session run storage.
pub trait SessionRunStore {
    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()>;
    fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>>;
    fn list_session_runs_by_session(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_parent(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_dispatch(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
}

/// Cron job leasing and run history storage.
pub trait CronStore {
    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()>;
    fn get_cron_job(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>>;
    fn get_cron_job_by_dedupe_key(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>>;
    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>>;
    fn delete_cron_job(&self, user_id: &str, job_id: &str) -> Result<i64>;
    fn delete_cron_jobs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn reset_cron_jobs_running(&self) -> Result<()>;
    fn count_running_cron_jobs(&self, now: f64) -> Result<i64>;
    fn claim_due_cron_jobs(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>>;
    fn renew_cron_job_lease(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool>;
    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()>;
    fn list_cron_runs(&self, user_id: &str, job_id: &str, limit: i64)
        -> Result<Vec<CronRunRecord>>;
    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>>;
}

/// Agent directory, hive, team run, and team task storage.
pub trait AgentDirectoryStore {
    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>>;
    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()>;
    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>>;
    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()>;
    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()>;
    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn list_user_agents_by_hive(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>>;
    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64>;
    fn upsert_hive(&self, record: &HiveRecord) -> Result<()>;
    fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>>;
    fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>>;
    fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn move_agents_to_hive(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64>;
    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()>;
    fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>>;
    fn list_team_runs(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)>;
    fn list_team_runs_by_status(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>>;
    fn list_team_runs_by_user_and_status(
        &self,
        user_id: &str,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>>;
    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()>;
    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>>;
    fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>>;
}

/// User token balance accounting storage.
pub trait TokenBalanceStore {
    fn prepare_user_token_balance(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn consume_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn grant_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
}

/// Complete storage surface kept for existing runtime call paths.
pub trait StorageBackend:
    StorageLifecycle
    + MetaStore
    + ConversationLogStore
    + LogStatsStore
    + MonitorStore
    + SessionLockStore
    + AgentRuntimeStore
    + VectorDocumentStore
    + MemoryRecordStore
    + BenchmarkStore
    + RetentionStore
    + UserAccountStore
    + ChatSessionStore
    + SessionGoalStore
    + UserWorldStore
    + BeeroomStore
    + ChannelDirectoryStore
    + ChannelRuntimeStore
    + BridgeStore
    + GatewayStore
    + MediaStore
    + SessionRunStore
    + CronStore
    + AgentDirectoryStore
    + TokenBalanceStore
    + Send
    + Sync
{
}

impl<T> StorageBackend for T where
    T: ?Sized
        + StorageLifecycle
        + MetaStore
        + ConversationLogStore
        + LogStatsStore
        + MonitorStore
        + SessionLockStore
        + AgentRuntimeStore
        + VectorDocumentStore
        + MemoryRecordStore
        + BenchmarkStore
        + RetentionStore
        + UserAccountStore
        + ChatSessionStore
        + SessionGoalStore
        + UserWorldStore
        + BeeroomStore
        + ChannelDirectoryStore
        + ChannelRuntimeStore
        + BridgeStore
        + GatewayStore
        + MediaStore
        + SessionRunStore
        + CronStore
        + AgentDirectoryStore
        + TokenBalanceStore
        + Send
        + Sync
{
}

// Helper for sorting monitor session records by updated_time.
fn monitor_record_updated_time(record: &Value) -> f64 {
    record
        .get("updated_time")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}
