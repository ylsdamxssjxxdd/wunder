use super::*;
use crate::storage::{
    AgentDirectoryStore, AgentRuntimeStore, BenchmarkStore, BridgeStore, ChannelDirectoryStore,
    ChannelRuntimeStore, ChatSessionStore, CloudCallRecord, CloudDeviceLogRecord,
    CloudDeviceInterlinkPatch, CloudDeviceRecord, CloudLogInsertResult, CloudStore,
    ConversationLogStore, CronStore, InterlinkApprovalRecord, InterlinkAuditRecord,
    InterlinkChannelRecord, InterlinkCommandRecord, InterlinkShadowRecord, InterlinkStore,
    ListInterlinkAuditQuery, ListInterlinkCommandsQuery,
    GatewayStore, ListCloudDeviceLogsQuery, ListCloudRecordsQuery, LogStatsStore, MediaStore,
    MemoryRecordStore, MetaStore, MonitorStore, QuotaBalanceStore, RetentionStore,
    SessionGoalStore, SessionLockStore, SessionRunStore, StorageLifecycle, TerminalTranscriptStore,
    ThreadLogStore, UserAccountStore, UserRefreshTokenRecord, UserRefreshTokenStore,
    UserWorldStore, VectorDocumentStore, WorkspaceRecord, WorkspaceStore,
};

impl StorageLifecycle for PostgresStorage {
    fn ensure_initialized(&self) -> Result<()> {
        self.ensure_initialized_impl()
    }
}

impl MetaStore for PostgresStorage {
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.get_meta_impl(key)
    }
    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.set_meta_impl(key, value)
    }
    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.list_meta_prefix_impl(prefix)
    }
    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize> {
        self.delete_meta_prefix_impl(prefix)
    }
}

impl ConversationLogStore for PostgresStorage {
    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_chat_impl(user_id, payload)
    }
    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_tool_log_impl(user_id, payload)
    }
    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_artifact_log_impl(user_id, payload)
    }
    fn load_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_artifact_logs_impl(user_id, session_id, limit)
    }
}

impl ThreadLogStore for PostgresStorage {
    fn load_subagent_context(
        &self,
        user_id: &str,
        session_id: &str,
        turns: i64,
    ) -> Result<Vec<Value>> {
        self.load_subagent_context_impl(user_id, session_id, turns)
    }
    fn upsert_thread_text_block(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<i64> {
        self.upsert_thread_text_block_impl(user_id, session_id, block)
    }
    fn thread_snapshot(&self, user_id: &str, session_id: &str) -> Result<Value> {
        self.thread_snapshot_impl(user_id, session_id)
    }
    fn list_thread_text_blocks(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_thread_text_blocks_impl(session_id, after, limit)
    }
    fn list_thread_item_blocks(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Vec<Value>> {
        self.list_thread_item_blocks_impl(
            user_id,
            session_id,
            item_id,
            from_block,
            limit,
            include_internal,
        )
    }
    fn list_thread_item_blocks_page(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        field: Option<&str>,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<(Vec<Value>, Option<i64>, bool)> {
        self.list_thread_item_blocks_page_impl(
            user_id,
            session_id,
            item_id,
            field,
            from_block,
            limit,
            include_internal,
        )
    }

    fn find_thread_turn_id(
        &self,
        user_id: &str,
        session_id: &str,
        user_turn_index: i64,
    ) -> Result<Option<String>> {
        self.find_thread_turn_id_impl(user_id, session_id, user_turn_index)
    }
    fn accept_thread_turn(&self, user_id: &str, session_id: &str, input: &Value) -> Result<Value> {
        self.accept_thread_turn_impl(user_id, session_id, input)
    }
    fn update_thread_turn(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        status: &str,
        summary: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.update_thread_turn_impl(user_id, session_id, turn_id, status, summary, payload)
    }
    fn list_thread_changes_by_session(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_thread_changes_by_session_impl(session_id, after_seq, limit)
    }
    fn latest_thread_change_seq_by_session(&self, session_id: &str) -> Result<i64> {
        self.latest_thread_change_seq_by_session_impl(session_id)
    }
    fn delete_thread_log_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_thread_log_by_session_impl(user_id, session_id)
    }

    fn append_thread_item(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_thread_item_impl(user_id, payload).map(|_| ())
    }
    fn list_thread_visible_messages(
        &self,
        user_id: &str,
        session_id: &str,
        before_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_thread_visible_messages_impl(user_id, session_id, before_seq, limit)
    }
    fn load_thread_context_items(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
        include_internal: bool,
    ) -> Result<Vec<Value>> {
        self.load_thread_context_items_impl(user_id, session_id, limit, include_internal, None)
    }
    fn load_thread_execution_context(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_thread_context_items_impl(user_id, session_id, limit, true, Some(turn_id))
    }
    fn fork_thread_log(
        &self,
        user_id: &str,
        source_session_id: &str,
        target_session_id: &str,
        through_round: i64,
    ) -> Result<()> {
        self.fork_thread_log_impl(user_id, source_session_id, target_session_id, through_round)
    }
    fn commit_thread_item(&self, user_id: &str, payload: &Value) -> Result<Option<Value>> {
        self.append_thread_item_impl(user_id, payload)
    }
    fn list_thread_turns(
        &self,
        user_id: &str,
        session_id: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_thread_turns_impl(user_id, session_id, before, limit)
    }
    fn get_thread_log_counts(
        &self,
        user_id: &str,
        session_id: &str,
        include_internal: bool,
    ) -> Result<(i64, i64)> {
        self.get_thread_log_counts_impl(user_id, session_id, include_internal)
    }
    fn latest_thread_user_round_by_session(&self, session_id: &str) -> Result<i64> {
        self.latest_thread_user_round_by_session_impl(session_id)
    }
    fn thread_turn_tails(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<std::collections::HashMap<String, crate::storage::ThreadTurnTail>> {
        self.thread_turn_tails_impl(user_id, session_ids)
    }
    fn get_thread_turn(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        after: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Option<Value>> {
        self.get_thread_turn_impl(user_id, session_id, turn_id, after, limit, include_internal)
    }
    fn get_thread_item(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        include_internal: bool,
    ) -> Result<Option<Value>> {
        self.get_thread_item_impl(user_id, session_id, item_id, include_internal)
    }
    fn set_thread_item_feedback(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        vote: &str,
    ) -> Result<Option<Value>> {
        self.set_thread_item_feedback_impl(user_id, session_id, item_id, vote)
    }
    fn list_thread_changes(
        &self,
        user_id: &str,
        session_id: &str,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_thread_changes_impl(user_id, session_id, after_seq, limit)
    }
}

impl LogStatsStore for PostgresStorage {
    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.get_user_chat_stats_impl()
    }
    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.get_user_tool_stats_impl()
    }
    fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.get_tool_usage_stats_impl(since_time, until_time)
    }
    fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.get_tool_session_usage_impl(tool, since_time, until_time)
    }
    fn get_log_usage(&self) -> Result<u64> {
        self.get_log_usage_impl()
    }
    fn delete_logs_by_time_range(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>> {
        self.delete_logs_by_time_range_impl(start_time, end_time)
    }
    fn delete_thread_logs_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_thread_logs_by_user_impl(user_id)
    }
    fn delete_tool_logs(&self, user_id: &str) -> Result<i64> {
        self.delete_tool_logs_impl(user_id)
    }
    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_tool_logs_by_session_impl(user_id, session_id)
    }
    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64> {
        self.delete_artifact_logs_impl(user_id)
    }
    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_artifact_logs_by_session_impl(user_id, session_id)
    }
    fn mark_deleted_session_log_grace(
        &self,
        user_id: &str,
        session_id: &str,
        deleted_at: f64,
    ) -> Result<()> {
        self.mark_deleted_session_log_grace_impl(user_id, session_id, deleted_at)
    }
    fn list_expired_deleted_session_log_grace(
        &self,
        cutoff: f64,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        self.list_expired_deleted_session_log_grace_impl(cutoff, limit)
    }
    fn delete_deleted_session_log_grace(&self, user_id: &str, session_id: &str) -> Result<()> {
        self.delete_deleted_session_log_grace_impl(user_id, session_id)
    }
}

impl MonitorStore for PostgresStorage {
    fn upsert_monitor_record(&self, payload: &Value) -> Result<()> {
        self.upsert_monitor_record_impl(payload)
    }
    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>> {
        self.get_monitor_record_impl(session_id)
    }
    fn load_monitor_records_by_session_ids(&self, session_ids: &[String]) -> Result<Vec<Value>> {
        self.load_monitor_records_by_session_ids_impl(session_ids)
    }
    fn load_monitor_records(&self) -> Result<Vec<Value>> {
        self.load_monitor_records_impl()
    }
    fn load_recent_monitor_records(&self, limit: i64) -> Result<Vec<Value>> {
        self.load_recent_monitor_records_impl(limit)
    }
    fn load_monitor_records_by_user(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_monitor_records_by_user_impl(user_id, statuses, since_time, limit)
    }
    fn sum_monitor_consumed_tokens_by_user(&self, user_id: &str) -> Result<i64> {
        self.sum_monitor_consumed_tokens_by_user_impl(user_id)
    }
    fn delete_monitor_record(&self, session_id: &str) -> Result<()> {
        self.delete_monitor_record_impl(session_id)
    }
    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_monitor_records_by_user_impl(user_id)
    }
}

impl SessionLockStore for PostgresStorage {
    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.try_acquire_session_lock_impl(session_id, user_id, agent_id, ttl_s, max_sessions)
    }
    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()> {
        self.touch_session_lock_impl(session_id, ttl_s)
    }
    fn release_session_lock(&self, session_id: &str) -> Result<()> {
        self.release_session_lock_impl(session_id)
    }
    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_session_locks_by_user_impl(user_id)
    }
    fn count_session_locks(&self) -> Result<i64> {
        self.count_session_locks_impl()
    }
    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.list_session_locks_by_user_impl(user_id)
    }
}

impl AgentRuntimeStore for PostgresStorage {
    fn insert_agent_message_task(
        &self,
        record: &AgentTaskRecord,
        pending_limit: i64,
    ) -> Result<()> {
        self.insert_agent_message_task_impl(record, pending_limit)
    }
    fn claim_agent_task(&self, task_id: &str, now: f64) -> Result<bool> {
        self.claim_agent_task_impl(task_id, now)
    }
    fn promote_agent_task(&self, task_id: &str, now: f64) -> Result<bool> {
        self.promote_agent_task_impl(task_id, now)
    }
    fn reorder_agent_tasks(&self, task_ids: &[String], now: f64) -> Result<usize> {
        self.reorder_agent_tasks_impl(task_ids, now)
    }
    fn update_agent_task_queue_payload(&self, task_id: &str, payload: &Value) -> Result<bool> {
        self.update_agent_task_queue_payload_impl(task_id, payload)
    }
    fn set_session_lock_suspended(
        &self,
        session_id: &str,
        suspended: bool,
        max_active: i64,
    ) -> Result<bool> {
        self.set_session_lock_suspended_impl(session_id, suspended, max_active)
    }

    fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()> {
        self.insert_agent_task_impl(record)
    }
    fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.get_agent_task_impl(task_id)
    }
    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.list_pending_agent_tasks_impl(limit)
    }
    fn count_pending_agent_tasks(&self) -> Result<i64> {
        self.count_pending_agent_tasks_impl()
    }
    fn count_pending_agent_tasks_ahead(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64> {
        self.count_pending_agent_tasks_ahead_impl(retry_at, created_at, task_id)
    }
    fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.list_agent_tasks_by_thread_impl(thread_id, status, limit)
    }
    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.update_agent_task_status_impl(params)
    }
    fn get_max_stream_event_id(&self, session_id: &str) -> Result<i64> {
        self.get_max_stream_event_id_impl(session_id)
    }
    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()> {
        self.append_stream_event_impl(session_id, user_id, event_id, payload)
    }
    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_stream_events_impl(session_id, after_event_id, limit)
    }
    fn load_recent_stream_events(&self, session_id: &str, limit: i64) -> Result<Vec<Value>> {
        self.load_recent_stream_events_impl(session_id, limit)
    }
    fn max_session_model_round(&self, session_id: &str, user_round: i64) -> Result<i64> {
        self.max_session_model_round_impl(session_id, user_round)
    }
    fn load_session_workflow_events(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<Vec<Value>> {
        self.load_session_workflow_events_impl(session_id, from_user_round, to_user_round)
    }
    fn load_session_workflow_events_page(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_session_workflow_events_page_impl(
            session_id,
            from_user_round,
            to_user_round,
            offset,
            limit,
        )
    }
    fn count_session_workflow_events(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<i64> {
        self.count_session_workflow_events_impl(session_id, from_user_round, to_user_round)
    }
    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64> {
        self.delete_stream_events_before_impl(before_time)
    }
    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_stream_events_by_user_impl(user_id)
    }
    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64> {
        self.delete_stream_events_by_session_impl(session_id)
    }
    fn delete_stream_events_by_round(
        &self,
        session_id: &str,
        user_round: i64,
        event_types: &[&str],
    ) -> Result<i64> {
        self.delete_stream_events_by_round_impl(session_id, user_round, event_types)
    }
}

impl VectorDocumentStore for PostgresStorage {
    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.upsert_vector_document_impl(record)
    }
    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>> {
        self.get_vector_document_impl(owner_id, base_name, doc_id)
    }
    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.list_vector_document_summaries_impl(owner_id, base_name)
    }
    fn delete_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<bool> {
        self.delete_vector_document_impl(owner_id, base_name, doc_id)
    }
    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.delete_vector_documents_by_base_impl(owner_id, base_name)
    }
    fn upsert_vector_chunk_embeddings(&self, records: &[VectorChunkEmbeddingRecord]) -> Result<()> {
        self.upsert_vector_chunk_embeddings_impl(records)
    }
    fn list_vector_chunk_embeddings(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        limit: i64,
    ) -> Result<Vec<VectorChunkEmbeddingRecord>> {
        self.list_vector_chunk_embeddings_impl(owner_id, base_name, embedding_model, limit)
    }
    fn delete_vector_chunk_embedding(&self, chunk_id: &str) -> Result<bool> {
        self.delete_vector_chunk_embedding_impl(chunk_id)
    }
    fn delete_vector_chunk_embeddings_by_doc(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<i64> {
        self.delete_vector_chunk_embeddings_by_doc_impl(owner_id, base_name, doc_id)
    }
    fn delete_vector_chunk_embeddings_by_base(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<i64> {
        self.delete_vector_chunk_embeddings_by_base_impl(owner_id, base_name)
    }
}

impl MemoryRecordStore for PostgresStorage {
    fn get_memory_enabled(&self, user_id: &str) -> Result<Option<bool>> {
        self.get_memory_enabled_impl(user_id)
    }
    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()> {
        self.set_memory_enabled_impl(user_id, enabled)
    }
    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_settings_impl()
    }
    fn upsert_memory_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()> {
        self.upsert_memory_record_impl(user_id, session_id, summary, max_records, now_ts)
    }
    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_records_impl(user_id, limit, order_desc)
    }
    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.get_memory_record_stats_impl()
    }
    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_memory_record_impl(user_id, session_id)
    }
    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_records_by_user_impl(user_id)
    }
    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_settings_by_user_impl(user_id)
    }
    fn upsert_memory_task_log(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()> {
        self.upsert_memory_task_log_impl(params)
    }
    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_task_logs_impl(limit)
    }
    fn load_memory_task_log_by_task_id(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.load_memory_task_log_by_task_id_impl(task_id)
    }
    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_memory_task_log_impl(user_id, session_id)
    }
    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_task_logs_by_user_impl(user_id)
    }
    fn upsert_memory_fragment(&self, record: &MemoryFragmentRecord) -> Result<()> {
        self.upsert_memory_fragment_impl(record)
    }
    fn get_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>> {
        self.get_memory_fragment_impl(user_id, agent_id, memory_id)
    }
    fn list_memory_fragments(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>> {
        self.list_memory_fragments_impl(user_id, agent_id)
    }
    fn get_memory_fragment_embedding(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>> {
        self.get_memory_fragment_embedding_impl(
            user_id,
            agent_id,
            memory_id,
            embedding_model,
            content_hash,
        )
    }
    fn upsert_memory_fragment_embedding(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()> {
        self.upsert_memory_fragment_embedding_impl(record)
    }
    fn delete_memory_fragment_embeddings(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.delete_memory_fragment_embeddings_impl(user_id, agent_id, memory_id)
    }
    fn delete_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.delete_memory_fragment_impl(user_id, agent_id, memory_id)
    }
    fn insert_memory_hit(&self, record: &MemoryHitRecord) -> Result<()> {
        self.insert_memory_hit_impl(record)
    }
    fn list_memory_hits(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>> {
        self.list_memory_hits_impl(user_id, agent_id, session_id, limit)
    }
    fn list_memory_hit_counts(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<HashMap<String, i64>> {
        self.list_memory_hit_counts_impl(user_id, agent_id)
    }
    fn has_memory_hit_event(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool> {
        self.has_memory_hit_event_impl(
            user_id, agent_id, memory_id, session_id, round_id, query_text,
        )
    }
    fn upsert_memory_job(&self, record: &MemoryJobRecord) -> Result<()> {
        self.upsert_memory_job_impl(record)
    }
    fn list_memory_jobs(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>> {
        self.list_memory_jobs_impl(user_id, agent_id, limit)
    }
}

impl BenchmarkStore for PostgresStorage {
    fn create_benchmark_run(&self, payload: &Value) -> Result<()> {
        self.create_benchmark_run_impl(payload)
    }
    fn update_benchmark_run(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.update_benchmark_run_impl(run_id, payload)
    }
    fn upsert_benchmark_attempt(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.upsert_benchmark_attempt_impl(run_id, payload)
    }
    fn upsert_benchmark_task_aggregate(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.upsert_benchmark_task_aggregate_impl(run_id, payload)
    }
    fn load_benchmark_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.load_benchmark_runs_impl(user_id, status, model_name, since_time, until_time, limit)
    }
    fn load_benchmark_run(&self, run_id: &str) -> Result<Option<Value>> {
        self.load_benchmark_run_impl(run_id)
    }
    fn load_benchmark_attempts(&self, run_id: &str) -> Result<Vec<Value>> {
        self.load_benchmark_attempts_impl(run_id)
    }
    fn load_benchmark_task_aggregates(&self, run_id: &str) -> Result<Vec<Value>> {
        self.load_benchmark_task_aggregates_impl(run_id)
    }
    fn delete_benchmark_run(&self, run_id: &str) -> Result<i64> {
        self.delete_benchmark_run_impl(run_id)
    }
}

impl RetentionStore for PostgresStorage {
    fn cleanup_retention(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.cleanup_retention_impl(cutoff_epoch_s)
    }
}

impl UserAccountStore for PostgresStorage {
    fn update_user_account_profile(&self, record: &UserAccountRecord) -> Result<()> {
        self.update_user_account_profile_impl(record)
    }
    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()> {
        self.upsert_user_account_impl(record)
    }
    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.upsert_user_accounts_impl(records)
    }
    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_impl(user_id)
    }
    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_by_username_impl(username)
    }
    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_by_email_impl(email)
    }
    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.list_user_accounts_impl(keyword, unit_ids, offset, limit)
    }
    fn add_user_experience(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult> {
        self.add_user_experience_impl(user_id, delta, updated_at)
    }
    fn delete_user_account(&self, user_id: &str) -> Result<i64> {
        self.delete_user_account_impl(user_id)
    }
    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>> {
        self.list_org_units_impl()
    }
    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.get_org_unit_impl(unit_id)
    }
    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()> {
        self.upsert_org_unit_impl(record)
    }
    fn delete_org_unit(&self, unit_id: &str) -> Result<i64> {
        self.delete_org_unit_impl(unit_id)
    }
    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.upsert_external_link_impl(record)
    }
    fn get_external_link(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>> {
        self.get_external_link_impl(link_id)
    }
    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.list_external_links_impl(include_disabled)
    }
    fn delete_external_link(&self, link_id: &str) -> Result<i64> {
        self.delete_external_link_impl(link_id)
    }
    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()> {
        self.create_user_token_impl(record)
    }
    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>> {
        self.get_user_token_impl(token)
    }
    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.touch_user_token_impl(token, last_used_at)
    }
    fn delete_user_token(&self, token: &str) -> Result<i64> {
        self.delete_user_token_impl(token)
    }
    fn delete_user_tokens_by_family(&self, family_id: &str) -> Result<u64> {
        self.delete_user_tokens_by_family_impl(family_id)
    }
    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.upsert_user_session_scope_impl(record)
    }
    fn get_user_session_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>> {
        self.get_user_session_scope_impl(user_id, session_scope)
    }
}

impl UserRefreshTokenStore for PostgresStorage {
    fn create_user_refresh_token(&self, record: &UserRefreshTokenRecord) -> Result<()> {
        self.create_user_refresh_token_impl(record)
    }
    fn get_user_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<UserRefreshTokenRecord>> {
        self.get_user_refresh_token_impl(refresh_token)
    }
    fn revoke_refresh_token(&self, refresh_token: &str) -> Result<()> {
        self.revoke_refresh_token_impl(refresh_token)
    }
    fn revoke_refresh_token_family(&self, family_id: &str) -> Result<u64> {
        self.revoke_refresh_token_family_impl(family_id)
    }
    fn revoke_refresh_token_families_by_user_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<u64> {
        self.revoke_refresh_token_families_by_user_scope_impl(user_id, session_scope)
    }
    fn cleanup_expired_refresh_tokens(&self, cutoff: f64) -> Result<u64> {
        self.cleanup_expired_refresh_tokens_impl(cutoff)
    }
}

impl ChatSessionStore for PostgresStorage {
    fn delete_chat_sessions_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_chat_sessions_by_user_impl(user_id)
    }
    fn get_chat_session_owner(&self, session_id: &str) -> Result<Option<String>> {
        self.get_chat_session_owner_impl(session_id)
    }
    fn list_active_chat_session_ids(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<String>> {
        self.list_active_chat_session_ids_impl(user_id, session_ids)
    }
    fn insert_chat_session_if_absent(&self, record: &ChatSessionRecord) -> Result<bool> {
        self.insert_chat_session_if_absent_impl(record)
    }

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        self.upsert_chat_session_impl(record)
    }
    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        self.get_chat_session_impl(user_id, session_id)
    }
    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_impl(user_id, agent_id, parent_session_id, offset, limit)
    }
    fn count_child_chat_sessions(
        &self,
        user_id: &str,
        parent_session_ids: &[String],
    ) -> Result<Vec<(String, i64)>> {
        self.count_child_chat_sessions_impl(user_id, parent_session_ids)
    }
    fn list_chat_sessions_by_status(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_by_status_impl(
            user_id,
            agent_id,
            parent_session_id,
            status,
            offset,
            limit,
        )
    }
    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>> {
        self.list_chat_session_agent_ids_impl(user_id)
    }
    fn list_chat_sessions_by_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_by_workspace_impl(user_id, workspace_id, status, offset, limit)
    }
    fn count_chat_sessions_by_workspace(&self, user_id: &str) -> Result<Vec<(String, i64)>> {
        self.count_chat_sessions_by_workspace_impl(user_id)
    }
    fn reassign_chat_sessions_workspace(
        &self,
        user_id: &str,
        from_workspace: &str,
        to_workspace: &str,
        updated_at: f64,
    ) -> Result<u64> {
        self.reassign_chat_sessions_workspace_impl(
            user_id,
            from_workspace,
            to_workspace,
            updated_at,
        )
    }
    fn list_work_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_filtered_impl(user_id, agent_id, None, status, offset, limit, true)
    }
    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()> {
        self.update_chat_session_title_impl(user_id, session_id, title, updated_at)
    }
    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()> {
        self.touch_chat_session_impl(user_id, session_id, updated_at, last_message_at)
    }
    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_chat_session_impl(user_id, session_id)
    }
}

impl SessionGoalStore for PostgresStorage {
    fn upsert_session_goal(&self, record: &SessionGoalRecord) -> Result<()> {
        self.upsert_session_goal_impl(record)
    }
    fn update_session_goal(
        &self,
        record: &SessionGoalRecord,
        expected_revision: i64,
    ) -> Result<bool> {
        self.update_session_goal_impl(record, expected_revision)
    }
    fn get_session_goal(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>> {
        self.get_session_goal_impl(user_id, session_id)
    }
    fn list_session_goals(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<SessionGoalRecord>> {
        self.list_session_goals_impl(user_id, session_ids)
    }
    fn delete_session_goal(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_session_goal_impl(user_id, session_id)
    }
}

impl UserWorldStore for PostgresStorage {
    fn resolve_or_create_user_world_direct_conversation(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.resolve_or_create_user_world_direct_conversation_impl(user_a, user_b, now)
    }
    fn create_user_world_group(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.create_user_world_group_impl(owner_user_id, group_name, member_user_ids, now)
    }
    fn get_user_world_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>> {
        self.get_user_world_conversation_impl(conversation_id)
    }
    fn get_user_world_member(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>> {
        self.get_user_world_member_impl(conversation_id, user_id)
    }
    fn list_user_world_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)> {
        self.list_user_world_conversations_impl(user_id, offset, limit)
    }
    fn list_user_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>> {
        self.list_user_world_messages_impl(conversation_id, before_message_id, limit)
    }
    fn send_user_world_message(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult> {
        self.send_user_world_message_impl(
            conversation_id,
            sender_user_id,
            content,
            content_type,
            client_msg_id,
            now,
        )
    }
    fn mark_user_world_read(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>> {
        self.mark_user_world_read_impl(conversation_id, user_id, last_read_message_id, now)
    }
    fn list_user_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>> {
        self.list_user_world_events_impl(conversation_id, after_event_id, limit)
    }
    fn list_user_world_groups(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)> {
        self.list_user_world_groups_impl(user_id, offset, limit)
    }
    fn get_user_world_group_by_id(&self, group_id: &str) -> Result<Option<UserWorldGroupRecord>> {
        self.get_user_world_group_by_id_impl(group_id)
    }
    fn update_user_world_group_announcement(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>> {
        self.update_user_world_group_announcement_impl(
            group_id,
            announcement,
            announcement_updated_at,
            updated_at,
        )
    }
    fn list_user_world_member_user_ids(&self, conversation_id: &str) -> Result<Vec<String>> {
        self.list_user_world_member_user_ids_impl(conversation_id)
    }
}

impl ChannelDirectoryStore for PostgresStorage {
    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.upsert_channel_account_impl(record)
    }
    fn get_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>> {
        self.get_channel_account_impl(channel, account_id)
    }
    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.list_channel_accounts_impl(channel, status)
    }
    fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_account_impl(channel, account_id)
    }
    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.upsert_channel_binding_impl(record)
    }
    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>> {
        self.list_channel_bindings_impl(channel)
    }
    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64> {
        self.delete_channel_binding_impl(binding_id)
    }
    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.upsert_channel_user_binding_impl(record)
    }
    fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>> {
        self.get_channel_user_binding_impl(channel, account_id, peer_kind, peer_id)
    }
    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.list_channel_user_bindings_impl(query)
    }
    fn delete_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64> {
        self.delete_channel_user_binding_impl(channel, account_id, peer_kind, peer_id)
    }
}

impl ChannelRuntimeStore for PostgresStorage {
    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.upsert_channel_session_impl(record)
    }
    fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        self.get_channel_session_impl(channel, account_id, peer_kind, peer_id, thread_id)
    }
    fn list_channel_sessions(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)> {
        self.list_channel_sessions_impl(channel, account_id, peer_id, session_id, offset, limit)
    }
    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.insert_channel_message_impl(record)
    }
    fn list_channel_messages(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>> {
        self.list_channel_messages_impl(channel, session_id, limit)
    }
    fn get_channel_message_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats> {
        self.get_channel_message_stats_impl(channel, account_id)
    }
    fn get_channel_outbox_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats> {
        self.get_channel_outbox_stats_impl(channel, account_id)
    }
    fn delete_channel_sessions(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_sessions_impl(channel, account_id)
    }
    fn delete_channel_messages(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_messages_impl(channel, account_id)
    }
    fn delete_channel_outbox(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_outbox_impl(channel, account_id)
    }
    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.enqueue_channel_outbox_impl(record)
    }
    fn get_channel_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        self.get_channel_outbox_impl(outbox_id)
    }
    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.list_pending_channel_outbox_impl(limit)
    }
    fn update_channel_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()> {
        self.update_channel_outbox_status_impl(params)
    }
}

impl BridgeStore for PostgresStorage {
    fn upsert_bridge_center(&self, record: &BridgeCenterRecord) -> Result<()> {
        self.upsert_bridge_center_impl(record)
    }
    fn get_bridge_center(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>> {
        self.get_bridge_center_impl(center_id)
    }
    fn get_bridge_center_by_code(&self, code: &str) -> Result<Option<BridgeCenterRecord>> {
        self.get_bridge_center_by_code_impl(code)
    }
    fn list_bridge_centers(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)> {
        self.list_bridge_centers_impl(query)
    }
    fn delete_bridge_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_center_impl(center_id)
    }
    fn upsert_bridge_center_account(&self, record: &BridgeCenterAccountRecord) -> Result<()> {
        self.upsert_bridge_center_account_impl(record)
    }
    fn get_bridge_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.get_bridge_center_account_impl(center_account_id)
    }
    fn get_bridge_center_account_by_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.get_bridge_center_account_by_channel_account_impl(channel, account_id)
    }
    fn list_bridge_center_accounts(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)> {
        self.list_bridge_center_accounts_impl(query)
    }
    fn delete_bridge_center_account(&self, center_account_id: &str) -> Result<i64> {
        self.delete_bridge_center_account_impl(center_account_id)
    }
    fn delete_bridge_center_accounts_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_center_accounts_by_center_impl(center_id)
    }
    fn upsert_bridge_user_route(&self, record: &BridgeUserRouteRecord) -> Result<()> {
        self.upsert_bridge_user_route_impl(record)
    }
    fn get_bridge_user_route(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>> {
        self.get_bridge_user_route_impl(route_id)
    }
    fn get_bridge_user_route_by_identity(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>> {
        self.get_bridge_user_route_by_identity_impl(center_account_id, external_identity_key)
    }
    fn list_bridge_user_routes(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)> {
        self.list_bridge_user_routes_impl(query)
    }
    fn delete_bridge_user_route(&self, route_id: &str) -> Result<i64> {
        self.delete_bridge_user_route_impl(route_id)
    }
    fn delete_bridge_user_routes_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_user_routes_by_center_impl(center_id)
    }
    fn delete_bridge_user_routes_by_center_account(&self, center_account_id: &str) -> Result<i64> {
        self.delete_bridge_user_routes_by_center_account_impl(center_account_id)
    }
    fn insert_bridge_delivery_log(&self, record: &BridgeDeliveryLogRecord) -> Result<()> {
        self.insert_bridge_delivery_log_impl(record)
    }
    fn list_bridge_delivery_logs(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>> {
        self.list_bridge_delivery_logs_impl(query)
    }
    fn delete_bridge_delivery_logs_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_delivery_logs_by_center_impl(center_id)
    }
    fn delete_bridge_delivery_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.delete_bridge_delivery_logs_by_center_account_impl(center_account_id)
    }
    fn insert_bridge_route_audit_log(&self, record: &BridgeRouteAuditLogRecord) -> Result<()> {
        self.insert_bridge_route_audit_log_impl(record)
    }
    fn list_bridge_route_audit_logs(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>> {
        self.list_bridge_route_audit_logs_impl(query)
    }
    fn delete_bridge_route_audit_logs_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_route_audit_logs_by_center_impl(center_id)
    }
    fn delete_bridge_route_audit_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.delete_bridge_route_audit_logs_by_center_account_impl(center_account_id)
    }
}

impl GatewayStore for PostgresStorage {
    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()> {
        self.upsert_gateway_client_impl(record)
    }
    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.list_gateway_clients_impl(status)
    }
    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.upsert_gateway_node_impl(record)
    }
    fn get_gateway_node(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>> {
        self.get_gateway_node_impl(node_id)
    }
    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.list_gateway_nodes_impl(status)
    }
    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.upsert_gateway_node_token_impl(record)
    }
    fn get_gateway_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>> {
        self.get_gateway_node_token_impl(token)
    }
    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.list_gateway_node_tokens_impl(node_id, status)
    }
    fn delete_gateway_node_token(&self, token: &str) -> Result<i64> {
        self.delete_gateway_node_token_impl(token)
    }
}

impl MediaStore for PostgresStorage {
    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()> {
        self.upsert_media_asset_impl(record)
    }
    fn get_media_asset(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>> {
        self.get_media_asset_impl(asset_id)
    }
    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.get_media_asset_by_hash_impl(hash)
    }
    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()> {
        self.upsert_speech_job_impl(record)
    }
    fn list_pending_speech_jobs(&self, job_type: &str, limit: i64) -> Result<Vec<SpeechJobRecord>> {
        self.list_pending_speech_jobs_impl(job_type, limit)
    }
}

impl SessionRunStore for PostgresStorage {
    fn touch_session_run(&self, user_id: &str, run_id: &str, now: f64) -> Result<()> {
        self.touch_session_run_impl(user_id, run_id, now)
    }
    fn interrupt_stale_session_runs(
        &self,
        user_id: &str,
        session_id: &str,
        cutoff: f64,
        now: f64,
    ) -> Result<i64> {
        self.interrupt_stale_session_runs_impl(user_id, session_id, cutoff, now)
    }
    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()> {
        self.upsert_session_run_impl(record)
    }
    fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>> {
        self.get_session_run_impl(run_id)
    }
    fn list_session_runs_by_session(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_session_impl(user_id, session_id, limit)
    }
    fn list_session_runs_by_parent(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_parent_impl(user_id, parent_session_id, limit)
    }
    fn list_session_runs_by_dispatch(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_dispatch_impl(user_id, dispatch_id, limit)
    }
}

impl CronStore for PostgresStorage {
    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()> {
        self.upsert_cron_job_impl(record)
    }
    fn get_cron_job(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>> {
        self.get_cron_job_impl(user_id, job_id)
    }
    fn get_cron_job_by_dedupe_key(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>> {
        self.get_cron_job_by_dedupe_key_impl(user_id, dedupe_key)
    }
    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>> {
        self.list_cron_jobs_impl(user_id, include_disabled)
    }
    fn delete_cron_job(&self, user_id: &str, job_id: &str) -> Result<i64> {
        self.delete_cron_job_impl(user_id, job_id)
    }
    fn delete_cron_jobs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_cron_jobs_by_session_impl(user_id, session_id)
    }
    fn reset_cron_jobs_running(&self) -> Result<()> {
        self.reset_cron_jobs_running_impl()
    }
    fn count_running_cron_jobs(&self, now: f64) -> Result<i64> {
        self.count_running_cron_jobs_impl(now)
    }
    fn claim_due_cron_jobs(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>> {
        self.claim_due_cron_jobs_impl(now, limit, runner_id, lease_expires_at)
    }
    fn renew_cron_job_lease(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool> {
        self.renew_cron_job_lease_impl(
            user_id,
            job_id,
            runner_id,
            run_token,
            heartbeat_at,
            lease_expires_at,
        )
    }
    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()> {
        self.insert_cron_run_impl(record)
    }
    fn list_cron_runs(
        &self,
        user_id: &str,
        job_id: &str,
        limit: i64,
    ) -> Result<Vec<CronRunRecord>> {
        self.list_cron_runs_impl(user_id, job_id, limit)
    }
    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        self.get_next_cron_run_at_impl(now)
    }
}

impl AgentDirectoryStore for PostgresStorage {
    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.get_user_tool_access_impl(user_id)
    }
    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()> {
        self.set_user_tool_access_impl(user_id, allowed_tools)
    }
    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>> {
        self.get_user_agent_access_impl(user_id)
    }
    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()> {
        self.set_user_agent_access_impl(user_id, allowed_agent_ids, blocked_agent_ids)
    }
    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()> {
        self.upsert_user_agent_impl(record)
    }
    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.get_user_agent_impl(user_id, agent_id)
    }
    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.get_user_agent_by_id_impl(agent_id)
    }
    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.list_user_agents_impl(user_id)
    }
    fn list_user_agents_for_users(&self, user_ids: &[String]) -> Result<Vec<UserAgentRecord>> {
        self.list_user_agents_for_users_impl(user_ids)
    }
    fn list_preset_bound_agents(
        &self,
        preset_id: &str,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<PresetBoundAgentRecord>, i64)> {
        self.list_preset_bound_agents_impl(preset_id, keyword, unit_ids, offset, limit)
    }
    fn count_preset_bound_users(&self, preset_ids: &[String]) -> Result<Vec<(String, i64)>> {
        self.count_preset_bound_users_impl(preset_ids)
    }
    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.list_shared_user_agents_impl(user_id)
    }
    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.delete_user_agent_impl(user_id, agent_id)
    }
}

impl QuotaBalanceStore for PostgresStorage {
    fn set_user_quota_balance(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        balance: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.set_user_quota_balance_impl(user_id, today, daily_grant, balance)
    }
    fn prepare_user_quota(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.prepare_user_quota_impl(user_id, today, daily_grant)
    }
    fn consume_user_quota(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.consume_user_quota_impl(user_id, today, daily_grant, amount)
    }
    fn grant_user_quota(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.grant_user_quota_impl(user_id, today, daily_grant, amount, updated_at)
    }
}

impl TerminalTranscriptStore for PostgresStorage {
    fn upsert_terminal_transcript_block(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        chunk_index: i64,
        seq: i64,
        text: &str,
    ) -> Result<()> {
        self.upsert_terminal_transcript_block_impl(
            user_id,
            session_id,
            terminal_id,
            chunk_index,
            seq,
            text,
        )
    }
    fn terminal_transcript_next_chunk(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.terminal_transcript_next_chunk_impl(user_id, session_id)
    }
    fn list_terminal_transcript_blocks(
        &self,
        user_id: &str,
        session_id: &str,
        from_chunk: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.list_terminal_transcript_blocks_impl(user_id, session_id, from_chunk, limit)
    }
    fn prune_terminal_transcript(
        &self,
        user_id: &str,
        session_id: &str,
        keep_chunks: i64,
    ) -> Result<()> {
        self.prune_terminal_transcript_impl(user_id, session_id, keep_chunks)
    }
}


impl InterlinkStore for PostgresStorage {
    fn update_cloud_device_interlink(
        &self,
        device_id: &str,
        patch: &CloudDeviceInterlinkPatch,
    ) -> Result<()> {
        self.update_cloud_device_interlink_impl(device_id, patch)
    }

    fn upsert_interlink_channel(&self, record: &InterlinkChannelRecord) -> Result<()> {
        self.upsert_interlink_channel_impl(record)
    }
    fn get_interlink_channel(&self, channel_id: &str) -> Result<Option<InterlinkChannelRecord>> {
        self.get_interlink_channel_impl(channel_id)
    }
    fn close_interlink_channel(&self, channel_id: &str, closed_reason: &str) -> Result<()> {
        self.close_interlink_channel_impl(channel_id, closed_reason)
    }
    fn list_interlink_channels(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<InterlinkChannelRecord>, i64)> {
        self.list_interlink_channels_impl(user_id, offset, limit)
    }

    fn upsert_interlink_shadow(&self, record: &InterlinkShadowRecord) -> Result<()> {
        self.upsert_interlink_shadow_impl(record)
    }
    fn get_interlink_shadow(&self, device_id: &str) -> Result<Option<InterlinkShadowRecord>> {
        self.get_interlink_shadow_impl(device_id)
    }
    fn get_interlink_shadow_revision(&self, device_id: &str) -> Result<i64> {
        self.get_interlink_shadow_revision_impl(device_id)
    }
    fn delete_interlink_shadow(&self, device_id: &str) -> Result<()> {
        self.delete_interlink_shadow_impl(device_id)
    }
    fn cleanup_interlink_shadows(&self, retention_days: u32, max_rows: i64) -> Result<u64> {
        self.cleanup_interlink_shadows_impl(retention_days, max_rows)
    }

    fn insert_interlink_command(&self, record: &InterlinkCommandRecord) -> Result<bool> {
        self.insert_interlink_command_impl(record)
    }
    fn update_interlink_command_status(
        &self,
        command_id: &str,
        status: &str,
        acked_at: Option<f64>,
        finished_at: Option<f64>,
        error_code: Option<&str>,
        error_summary: Option<&str>,
    ) -> Result<()> {
        self.update_interlink_command_status_impl(
            command_id,
            status,
            acked_at,
            finished_at,
            error_code,
            error_summary,
        )
    }
    fn set_interlink_command_approval(
        &self,
        command_id: &str,
        approval_state: &str,
    ) -> Result<()> {
        self.set_interlink_command_approval_impl(command_id, approval_state)
    }
    fn get_interlink_command(&self, command_id: &str) -> Result<Option<InterlinkCommandRecord>> {
        self.get_interlink_command_impl(command_id)
    }
    fn list_interlink_commands(
        &self,
        query: ListInterlinkCommandsQuery<'_>,
    ) -> Result<(Vec<InterlinkCommandRecord>, i64)> {
        self.list_interlink_commands_impl(query)
    }
    fn cleanup_interlink_commands(&self, retention_days: u32) -> Result<u64> {
        self.cleanup_interlink_commands_impl(retention_days)
    }

    fn insert_interlink_approval(&self, record: &InterlinkApprovalRecord) -> Result<()> {
        self.insert_interlink_approval_impl(record)
    }
    fn decide_interlink_approval(
        &self,
        approval_id: &str,
        state: &str,
        decided_by: &str,
        decided_at: f64,
    ) -> Result<()> {
        self.decide_interlink_approval_impl(approval_id, state, decided_by, decided_at)
    }
    fn get_interlink_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<InterlinkApprovalRecord>> {
        self.get_interlink_approval_impl(approval_id)
    }

    fn insert_interlink_audit(&self, record: &InterlinkAuditRecord) -> Result<()> {
        self.insert_interlink_audit_impl(record)
    }
    fn list_interlink_audit(
        &self,
        query: ListInterlinkAuditQuery<'_>,
    ) -> Result<(Vec<InterlinkAuditRecord>, i64)> {
        self.list_interlink_audit_impl(query)
    }
    fn cleanup_interlink_audit(&self, retention_days: u32) -> Result<u64> {
        self.cleanup_interlink_audit_impl(retention_days)
    }
}

impl CloudStore for PostgresStorage {
    fn find_cloud_device_by_identity(
        &self,
        user_id: &str,
        client: &str,
        name: &str,
    ) -> Result<Option<CloudDeviceRecord>> {
        self.find_cloud_device_by_identity_impl(user_id, client, name)
    }
    fn upsert_cloud_device(&self, record: &CloudDeviceRecord) -> Result<()> {
        self.upsert_cloud_device_impl(record)
    }
    fn get_cloud_device(&self, device_id: &str) -> Result<Option<CloudDeviceRecord>> {
        self.get_cloud_device_impl(device_id)
    }
    fn touch_cloud_device(&self, device_id: &str, last_seen_at: f64) -> Result<()> {
        self.touch_cloud_device_impl(device_id, last_seen_at)
    }
    fn set_cloud_device_revoked(&self, device_id: &str, revoked: bool) -> Result<()> {
        self.set_cloud_device_revoked_impl(device_id, revoked)
    }
    fn list_cloud_devices(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<CloudDeviceRecord>, i64)> {
        self.list_cloud_devices_impl(user_id, offset, limit)
    }
    fn insert_cloud_call_record(&self, record: &CloudCallRecord) -> Result<()> {
        self.insert_cloud_call_record_impl(record)
    }
    fn finalize_cloud_call_record(
        &self,
        call_id: &str,
        status: &str,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        finished_at: f64,
        error_summary: Option<&str>,
    ) -> Result<()> {
        self.finalize_cloud_call_record_impl(
            call_id,
            status,
            prompt_tokens,
            completion_tokens,
            finished_at,
            error_summary,
        )
    }
    fn list_cloud_call_records(
        &self,
        query: ListCloudRecordsQuery<'_>,
    ) -> Result<(Vec<CloudCallRecord>, i64)> {
        self.list_cloud_call_records_impl(query)
    }
    fn insert_cloud_device_logs(
        &self,
        records: &[CloudDeviceLogRecord],
    ) -> Result<CloudLogInsertResult> {
        self.insert_cloud_device_logs_impl(records)
    }
    fn latest_cloud_device_log_seq(&self, device_id: &str) -> Result<i64> {
        self.latest_cloud_device_log_seq_impl(device_id)
    }
    fn list_cloud_device_logs(
        &self,
        query: ListCloudDeviceLogsQuery<'_>,
    ) -> Result<(Vec<CloudDeviceLogRecord>, i64)> {
        self.list_cloud_device_logs_impl(query)
    }
    fn cleanup_cloud_device_logs(&self, retention_days: u32) -> Result<u64> {
        self.cleanup_cloud_device_logs_impl(retention_days)
    }
}

impl WorkspaceStore for PostgresStorage {
    fn upsert_workspace(&self, record: &WorkspaceRecord) -> Result<()> {
        self.upsert_workspace_impl(record)
    }
    fn get_workspace(&self, user_id: &str, workspace_id: &str) -> Result<Option<WorkspaceRecord>> {
        self.get_workspace_impl(user_id, workspace_id)
    }
    fn list_workspaces(&self, user_id: &str) -> Result<Vec<WorkspaceRecord>> {
        self.list_workspaces_impl(user_id)
    }
    fn find_workspace_by_root(
        &self,
        user_id: &str,
        root_path: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        self.find_workspace_by_root_impl(user_id, root_path)
    }
    fn next_workspace_sort_index(&self, user_id: &str) -> Result<i64> {
        self.next_workspace_sort_index_impl(user_id)
    }
    fn delete_workspace(&self, user_id: &str, workspace_id: &str) -> Result<i64> {
        self.delete_workspace_impl(user_id, workspace_id)
    }
}
