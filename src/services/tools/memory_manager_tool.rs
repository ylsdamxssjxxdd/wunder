use super::context::ToolContext;
use crate::i18n;
use crate::memory::{build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore};
use crate::services::memory_fragments::{
    MemoryFragmentInput, MemoryFragmentListOptions, MemoryFragmentStore,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

const MAX_MEMORY_RECALL_LIMIT: usize = 12;

#[derive(Debug, Deserialize)]
struct MemoryManagerArgs {
    action: String,
    #[serde(default)]
    memory_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    order: Option<String>,
}

fn normalize_memory_manager_action(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase();
    match cleaned.as_str() {
        "list" => "list".to_string(),
        "add" | "create" | "append" => "add".to_string(),
        "update" | "upsert" => "update".to_string(),
        "delete" | "remove" => "delete".to_string(),
        "clear" | "reset" => "clear".to_string(),
        "recall" | "search" | "query" | "retrieve" => "recall".to_string(),
        _ => String::new(),
    }
}

fn normalize_memory_record_id(payload: &MemoryManagerArgs) -> String {
    payload
        .memory_id
        .as_deref()
        .or(payload.id.as_deref())
        .or(payload.session_id.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_content(payload: &MemoryManagerArgs) -> String {
    payload
        .content
        .as_deref()
        .or(payload.summary.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_query(payload: &MemoryManagerArgs) -> String {
    payload
        .query
        .as_deref()
        .or(payload.content.as_deref())
        .or(payload.summary.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_list_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 200)
}

fn normalize_memory_recall_limit(limit: Option<i64>) -> usize {
    limit.unwrap_or(6).clamp(1, MAX_MEMORY_RECALL_LIMIT as i64) as usize
}

fn normalize_memory_order_desc(order: Option<&str>) -> bool {
    let cleaned = order.unwrap_or("").trim().to_lowercase();
    if cleaned.is_empty() {
        return true;
    }
    !matches!(cleaned.as_str(), "asc" | "ascending")
}

fn clear_fragment_scope(
    fragment_store: &MemoryFragmentStore,
    user_id: &str,
    agent_id: Option<&str>,
) -> i64 {
    let mut deleted = 0i64;
    loop {
        let batch = fragment_store.list_fragments(
            user_id,
            agent_id,
            MemoryFragmentListOptions {
                include_invalidated: true,
                limit: Some(200),
                ..Default::default()
            },
        );
        if batch.is_empty() {
            break;
        }
        for item in &batch {
            if fragment_store.delete_fragment(user_id, agent_id, &item.memory_id) {
                deleted += 1;
            }
        }
        if batch.len() < 200 {
            break;
        }
    }
    deleted
}

pub(crate) async fn execute_memory_manager_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: MemoryManagerArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = normalize_memory_manager_action(&payload.action);
    if action.is_empty() {
        return Err(anyhow!(i18n::t("tool.memory_manager.invalid_action")));
    }

    let agent_scope = normalize_agent_memory_scope(context.agent_id);
    let fragment_store = MemoryFragmentStore::new(context.storage.clone());

    let response = match action.as_str() {
        "list" => {
            let limit = normalize_memory_list_limit(payload.limit) as usize;
            let order_desc = normalize_memory_order_desc(payload.order.as_deref());
            let mut records = fragment_store.list_fragments(
                context.user_id,
                context.agent_id,
                MemoryFragmentListOptions {
                    query: payload.query.as_deref(),
                    include_invalidated: true,
                    limit: Some(limit),
                    ..Default::default()
                },
            );
            if !order_desc {
                records.reverse();
            }
            let items = records
                .into_iter()
                .map(|record| {
                    json!({
                        "memory_id": record.memory_id,
                        "title": record.title_l0,
                        "summary": record.summary_l1,
                        "content": record.content_l2,
                        "category": record.category,
                        "source_type": record.source_type,
                        "status": record.status,
                        "created_time_ts": record.created_at,
                        "updated_time_ts": record.updated_at,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "action": action,
                "agent_id": agent_scope,
                "count": items.len(),
                "items": items,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "add" => {
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let mut memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                memory_id = format!("mem_{}", Uuid::new_v4().simple());
            }
            let saved_record = fragment_store
                .save_fragment(
                    context.user_id,
                    context.agent_id,
                    MemoryFragmentInput {
                        memory_id: Some(memory_id.clone()),
                        source_session_id: Some(context.session_id.to_string()),
                        source_type: Some("memory_manager".to_string()),
                        category: Some("tool-note".to_string()),
                        summary_l1: Some(content.clone()),
                        content_l2: Some(content),
                        fact_key: Some(format!("tool-note::{memory_id}")),
                        pinned: Some(false),
                        invalidated: Some(false),
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to save memory fragment: {err}")))?;
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": saved_record.memory_id,
                "saved": true,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "update" => {
            let memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let updated_record = fragment_store
                .save_fragment(
                    context.user_id,
                    context.agent_id,
                    MemoryFragmentInput {
                        memory_id: Some(memory_id.clone()),
                        source_session_id: Some(context.session_id.to_string()),
                        source_type: Some("memory_manager".to_string()),
                        category: Some("tool-note".to_string()),
                        summary_l1: Some(content.clone()),
                        content_l2: Some(content),
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to update memory fragment: {err}")))?;
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": updated_record.memory_id,
                "updated": true,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "delete" => {
            let memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let fragment_deleted =
                fragment_store.delete_fragment(context.user_id, context.agent_id, &memory_id);
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let legacy_deleted =
                cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            let deleted = i64::from(fragment_deleted) + legacy_deleted;
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": memory_id,
                "deleted": deleted,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "clear" => {
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let deleted = clear_fragment_scope(&fragment_store, context.user_id, context.agent_id)
                + memory_store.clear_records(&owner_key);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "deleted": deleted,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "recall" => {
            let query = normalize_memory_query(&payload);
            let recall_limit = normalize_memory_recall_limit(payload.limit);
            let hits = fragment_store
                .recall_for_prompt(
                    Some(context.config),
                    context.user_id,
                    context.agent_id,
                    Some(context.session_id),
                    None,
                    (!query.is_empty()).then_some(query.as_str()),
                    Some(recall_limit),
                )
                .await;
            let items = hits
                .into_iter()
                .map(|hit| {
                    let fragment = hit.fragment;
                    json!({
                        "memory_id": fragment.memory_id,
                        "title": fragment.title_l0,
                        "summary": fragment.summary_l1,
                        "content": fragment.content_l2,
                        "category": fragment.category,
                        "source_type": fragment.source_type,
                        "tags": fragment.tags,
                        "entities": fragment.entities,
                        "pinned": fragment.pinned,
                        "status": fragment.status,
                        "updated_at": fragment.updated_at,
                        "score": hit.final_score,
                        "lexical_score": hit.lexical_score,
                        "semantic_score": hit.semantic_score,
                        "freshness_score": hit.freshness_score,
                        "importance_score": hit.importance_score,
                        "reason": hit.reason_json,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "action": action,
                "agent_id": agent_scope,
                "query": query,
                "count": items.len(),
                "items": items,
                "note": i18n::t("tool.memory_manager.note_recall_current_session"),
            })
        }
        _ => return Err(anyhow!(i18n::t("tool.memory_manager.invalid_action"))),
    };

    Ok(response)
}

fn cleanup_legacy_memory_record(
    memory_store: &MemoryStore,
    owner_key: &str,
    memory_id: &str,
) -> i64 {
    let cleaned = memory_id.trim();
    if cleaned.is_empty() {
        return 0;
    }
    let direct = memory_store.delete_record(owner_key, cleaned);
    if let Some(legacy_id) = cleaned.strip_prefix("legacy::") {
        return direct + memory_store.delete_record(owner_key, legacy_id);
    }
    direct
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::services::tools::context::ToolContext;
    use crate::skills::SkillRegistry;
    use crate::storage::{SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct TestHarness {
        _dir: tempfile::TempDir,
        config: Config,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        lsp_manager: Arc<LspManager>,
        a2a_store: A2aStore,
        skills: SkillRegistry,
        http: reqwest::Client,
    }

    impl TestHarness {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("memory-manager-tool.db");
            let storage: Arc<dyn StorageBackend> =
                Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
            storage.ensure_initialized().expect("init storage");
            let config = Config::default();
            let workspace = Arc::new(WorkspaceManager::new(
                dir.path().to_string_lossy().as_ref(),
                storage.clone(),
                0,
                &HashMap::new(),
            ));
            let lsp_manager = LspManager::new(workspace.clone());
            Self {
                _dir: dir,
                config,
                storage,
                workspace,
                lsp_manager,
                a2a_store: A2aStore::default(),
                skills: SkillRegistry::default(),
                http: reqwest::Client::new(),
            }
        }

        fn context<'a>(
            &'a self,
            user_id: &'a str,
            session_id: &'a str,
            agent_id: Option<&'a str>,
        ) -> ToolContext<'a> {
            ToolContext {
                user_id,
                session_id,
                workspace_id: "workspace-test",
                agent_id,
                is_admin: false,
                storage: self.storage.clone(),
                orchestrator: None,
                monitor: None,
                beeroom_realtime: None,
                workspace: self.workspace.clone(),
                lsp_manager: self.lsp_manager.clone(),
                config: &self.config,
                a2a_store: &self.a2a_store,
                skills: &self.skills,
                gateway: None,
                user_world: None,
                cron_wake_signal: None,
                user_tool_manager: None,
                user_tool_bindings: None,
                user_tool_store: None,
                request_config_overrides: None,
                allow_roots: None,
                read_roots: None,
                event_emitter: None,
                http: &self.http,
            }
        }
    }

    #[test]
    fn normalize_action_supports_recall_aliases() {
        assert_eq!(normalize_memory_manager_action("recall"), "recall");
        assert_eq!(normalize_memory_manager_action("search"), "recall");
        assert_eq!(normalize_memory_manager_action("query"), "recall");
        assert_eq!(normalize_memory_manager_action("retrieve"), "recall");
    }

    #[test]
    fn normalize_recall_limit_clamps_range() {
        assert_eq!(normalize_memory_recall_limit(None), 6);
        assert_eq!(normalize_memory_recall_limit(Some(0)), 1);
        assert_eq!(normalize_memory_recall_limit(Some(99)), 12);
    }

    #[tokio::test]
    async fn memory_manager_tool_writes_visible_fragments() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-1", Some("agent-demo"));

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "pref-reply-language",
                "content": "默认使用中文回答。"
            }),
        )
        .await
        .expect("add memory");
        assert_eq!(add.get("saved").and_then(Value::as_bool), Some(true));

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragments = fragment_store.list_fragments(
            "u1",
            Some("agent-demo"),
            MemoryFragmentListOptions::default(),
        );
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].memory_id, "pref-reply-language");
        assert_eq!(fragments[0].source_type, "memory-manager");
        assert_eq!(fragments[0].category, "tool-note");

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list memory");
        assert_eq!(listed.get("count").and_then(Value::as_u64), Some(1));

        let recalled = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "recall",
                "query": "中文回答"
            }),
        )
        .await
        .expect("recall memory");
        assert_eq!(recalled.get("count").and_then(Value::as_u64), Some(1));

        let updated = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "update",
                "memory_id": "pref-reply-language",
                "content": "默认使用简体中文回答。"
            }),
        )
        .await
        .expect("update memory");
        assert_eq!(updated.get("updated").and_then(Value::as_bool), Some(true));

        let refreshed = fragment_store
            .get_fragment("u1", Some("agent-demo"), "pref-reply-language")
            .expect("get updated fragment");
        assert_eq!(refreshed.content_l2, "默认使用简体中文回答。");

        let deleted = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "delete",
                "memory_id": "pref-reply-language"
            }),
        )
        .await
        .expect("delete memory");
        assert_eq!(deleted.get("deleted").and_then(Value::as_i64), Some(1));
        assert!(fragment_store
            .get_fragment("u1", Some("agent-demo"), "pref-reply-language")
            .is_none());
    }
}
